use anyhow::{bail, Context, Result};
use indicatif::ProgressBar;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::env;
use std::io::Read;
use std::time::Duration;
use ureq::Error as HttpError;

use crate::registry::Config;

const API_BASE: &str = "https://api.github.com";
const MAX_DOWNLOAD: usize = 1024 * 1024 * 1024;

pub struct Client {
    agent: ureq::Agent,
    token: Option<String>,
    api_base: String,
}

impl Client {
    pub fn new(config: &Config) -> Result<Self> {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .user_agent("berth")
            .build()
            .into();
        let token = config.sources.gh_token.trim();
        let token = if token.is_empty() {
            None
        } else {
            Some(token.to_string())
        };
        let api_base = env::var("BERTH_GITHUB_API_BASE")
            .unwrap_or_else(|_| API_BASE.to_string())
            .trim_end_matches('/')
            .to_string();
        Ok(Self {
            agent,
            token,
            api_base,
        })
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    fn get_bytes(&self, url: &str, authorized: bool) -> Result<Vec<u8>> {
        let mut request = self.agent.get(url);
        if authorized {
            if let Some(token) = &self.token {
                request = request.header("Authorization", format!("Bearer {}", token));
            }
        }
        let mut response = match request.call() {
            Ok(response) => response,
            Err(HttpError::StatusCode(401 | 403)) => {
                bail!("github rate limit exceeded. Add gh_token to ~/.config/berth/config.toml or wait a few min.")
            }
            Err(HttpError::StatusCode(status)) => {
                bail!("request to {} failed with HTTP {}", url, status)
            }
            Err(error) => return Err(error).with_context(|| format!("request to {} failed", url)),
        };
        response
            .body_mut()
            .with_config()
            .limit(16 * 1024 * 1024)
            .read_to_vec()
            .with_context(|| format!("cannot read response from {}", url))
    }

    pub fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let bytes = self.get_bytes(url, true)?;
        serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON from {}", url))
    }

    pub fn download(&self, url: &str, bar: &ProgressBar) -> Result<Vec<u8>> {
        let response = self
            .agent
            .get(url)
            .call()
            .with_context(|| format!("cannot download {}", url))?;
        let length = response.body().content_length();
        if let Some(len) = length {
            bar.set_length(len);
        }
        let mut reader = response.into_body().into_reader();
        let mut bytes = Vec::with_capacity(length.unwrap_or(0) as usize);
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            if bytes.len() + read > MAX_DOWNLOAD {
                bail!("download exceeds the 1 GiB safety limit: {}", url);
            }
            bytes.extend_from_slice(&buffer[..read]);
            bar.inc(read as u64);
        }
        Ok(bytes)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub digest: Option<String>,
}

pub fn fetch_release(
    client: &Client,
    owner: &str,
    repo: &str,
    tag: Option<&str>,
) -> Result<Release> {
    let path = match tag {
        Some(tag) => format!("repos/{}/{}/releases/tags/{}", owner, repo, tag),
        None => format!("repos/{}/{}/releases/latest", owner, repo),
    };
    client
        .get_json::<Release>(&format!("{}/{}", client.api_base(), path))
        .with_context(|| format!("cannot resolve github.com/{}/{}", owner, repo))
}

pub fn version_from_tag(tag: &str) -> String {
    tag.trim_start_matches(['v', 'V']).to_string()
}

pub fn compare_versions(left: &str, right: &str) -> Ordering {
    let left = numeric_parts(left);
    let right = numeric_parts(right);
    for index in 0..left.len().max(right.len()) {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        match a.cmp(&b) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

fn numeric_parts(version: &str) -> Vec<u64> {
    version
        .trim_start_matches(['v', 'V'])
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

pub fn pick_asset<'a>(release: &'a Release, preferred: Option<&str>) -> Result<&'a Asset> {
    let mut candidates: Vec<&Asset> = release
        .assets
        .iter()
        .filter(|asset| !is_checksum_asset(&asset.name))
        .collect();
    if let Some(preferred) = preferred {
        if let Some(asset) = candidates
            .iter()
            .find(|asset| asset.name == preferred)
            .copied()
        {
            return Ok(asset);
        }
    }
    if candidates.is_empty() {
        bail!("release {} has no downloadable assets", release.tag_name);
    }
    if candidates.len() == 1 {
        return Ok(candidates[0]);
    }
    candidates.sort_by_key(|asset| std::cmp::Reverse(asset_score(&asset.name)));
    if asset_score(&candidates[0].name) <= 0 {
        let names = candidates
            .iter()
            .map(|asset| asset.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "cannot pick a linux x86_64 asset from {}. Pass --bin-name with --url, or choose a tool with a clear linux asset",
            names
        );
    }
    Ok(candidates[0])
}

fn is_checksum_asset(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("checksum")
        || lower.ends_with(".sha256")
        || lower.ends_with(".sha512")
        || lower.ends_with(".sig")
        || lower.ends_with(".asc")
}

fn asset_score(name: &str) -> i32 {
    let lower = name.to_ascii_lowercase();
    let mut score = 0;
    if lower.contains("linux") {
        score += 4;
    }
    if ["x86_64", "amd64", "x64"]
        .iter()
        .any(|arch| lower.contains(arch))
    {
        score += 3;
    }
    if lower.contains("gnu") {
        score += 1;
    }
    if [
        "windows", "darwin", "macos", "aarch64", "arm64", "i686", "armv7",
    ]
    .iter()
    .any(|bad| lower.contains(bad))
    {
        score -= 10;
    }
    score
}

pub fn checksum_of(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

pub fn verify_checksum(bytes: &[u8], expected: &str) -> Result<()> {
    let actual = checksum_of(bytes);
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        bail!("sha256 mismatch: expected {}, got {}", expected, actual)
    }
}

pub fn published_checksum(client: &Client, release: &Release, asset: &Asset) -> Option<String> {
    if let Some(digest) = &asset.digest {
        if let Some(hex) = digest.strip_prefix("sha256:") {
            return Some(format!("sha256:{}", hex));
        }
    }
    for candidate in &release.assets {
        let lower = candidate.name.to_ascii_lowercase();
        if !(lower.contains("checksum")
            || lower.contains("sha256sum")
            || lower.ends_with(".sha256"))
        {
            continue;
        }
        let text = match client.get_bytes(&candidate.browser_download_url, false) {
            Ok(text) => String::from_utf8_lossy(&text).to_string(),
            Err(_) => continue,
        };
        for line in text.lines() {
            let mut fields = line.split_whitespace();
            let Some(digest) = fields.next() else {
                continue;
            };
            if !digest.bytes().all(|b| b.is_ascii_hexdigit()) || digest.len() != 64 {
                continue;
            }
            let filename = fields.next().unwrap_or("").trim_start_matches('*');
            if filename == asset.name {
                return Some(format!("sha256:{}", digest));
            }
            if filename.is_empty() && line.contains(&asset.name) {
                return Some(format!("sha256:{}", digest));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_with_optional_v_prefix() {
        assert_eq!(compare_versions("v2.43.0", "2.42.9"), Ordering::Greater);
        assert_eq!(compare_versions("1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.10.0", "1.9.9"), Ordering::Greater);
        assert_eq!(compare_versions("v2.0.0", "2.0.1"), Ordering::Less);
    }

    #[test]
    fn checksum_roundtrip_and_mismatch() {
        let bytes = b"berth";
        let sum = checksum_of(bytes);
        assert!(verify_checksum(bytes, &sum).is_ok());
        assert!(verify_checksum(bytes, "sha256:deadbeef").is_err());
    }

    #[test]
    fn picks_linux_asset_over_other_platforms() {
        let release = Release {
            tag_name: "v1.0.0".into(),
            assets: vec![
                Asset {
                    name: "tool-windows-amd64.zip".into(),
                    browser_download_url: "http://example/windows".into(),
                    digest: None,
                },
                Asset {
                    name: "tool-linux-x86_64.tar.gz".into(),
                    browser_download_url: "http://example/linux".into(),
                    digest: None,
                },
                Asset {
                    name: "checksums.txt".into(),
                    browser_download_url: "http://example/checksums".into(),
                    digest: None,
                },
            ],
        };
        let picked = pick_asset(&release, None).unwrap();
        assert_eq!(picked.name, "tool-linux-x86_64.tar.gz");
    }

    #[test]
    fn rejects_ambiguous_assets() {
        let release = Release {
            tag_name: "v1.0.0".into(),
            assets: vec![
                Asset {
                    name: "alpha.bin".into(),
                    browser_download_url: "http://example/a".into(),
                    digest: None,
                },
                Asset {
                    name: "beta.bin".into(),
                    browser_download_url: "http://example/b".into(),
                    digest: None,
                },
            ],
        };
        assert!(pick_asset(&release, None).is_err());
    }
}
