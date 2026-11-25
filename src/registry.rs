use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub sources: Sources,
    #[serde(default)]
    pub paths: PathConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sources {
    #[serde(default)]
    pub gh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathConfig {
    #[serde(default = "default_bin_dir")]
    pub bin_dir: String,
    #[serde(default = "default_store_dir")]
    pub store_dir: String,
    #[serde(default = "default_registry_dir")]
    pub registry_dir: String,
}

impl Default for PathConfig {
    fn default() -> Self {
        Self {
            bin_dir: default_bin_dir(),
            store_dir: default_store_dir(),
            registry_dir: default_registry_dir(),
        }
    }
}

fn default_bin_dir() -> String {
    "~/.local/bin".to_string()
}

fn default_store_dir() -> String {
    "~/.local/share/berth/bin".to_string()
}

fn default_registry_dir() -> String {
    "~/.local/share/berth".to_string()
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub home: PathBuf,
    pub bin_dir: PathBuf,
    pub store_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub registry_file: PathBuf,
    pub config_file: PathBuf,
}

pub fn load_config(path: &Path) -> Result<Config> {
    match fs::read_to_string(path) {
        Ok(text) => {
            toml::from_str(&text).with_context(|| format!("invalid config at {}", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

impl Paths {
    pub fn from_env() -> Result<Self> {
        let home = PathBuf::from(
            env::var("HOME").context("HOME is not set; cannot resolve berth's XDG paths")?,
        );
        let config_file = home.join(".config/berth/config.toml");
        let config = load_config(&config_file)?;

        let registry_dir = expand_tilde(&config.paths.registry_dir, &home);
        let store_dir = expand_tilde(&config.paths.store_dir, &home);
        let bin_dir = expand_tilde(&config.paths.bin_dir, &home);
        Ok(Self {
            home,
            bin_dir,
            store_dir,
            cache_dir: registry_dir.join("cache"),
            registry_file: registry_dir.join("registry.toml"),
            config_file,
        })
    }

    pub fn display(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(relative) => format!("~/{}", relative.display()),
            Err(_) => path.display().to_string(),
        }
    }
}

pub fn expand_tilde(value: &str, home: &Path) -> PathBuf {
    if value == "~" {
        return home.to_path_buf();
    }
    match value.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(value),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub tools: Vec<Tool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub version: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default = "default_track", skip_serializing_if = "is_latest_track")]
    pub track: String,
    #[serde(rename = "bin-name")]
    pub bin_name: String,
    pub checksum: String,
    pub linked: String,
    pub updated: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<PreviousVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviousVersion {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    pub checksum: String,
}

fn default_track() -> String {
    "latest".to_string()
}

fn is_latest_track(track: &str) -> bool {
    track == "latest"
}

impl Tool {
    pub fn store_path(&self, store_dir: &Path) -> PathBuf {
        store_dir
            .join(&self.name)
            .join(&self.version)
            .join(&self.bin_name)
    }

    pub fn linked_path(&self, home: &Path) -> PathBuf {
        expand_tilde(&self.linked, home)
    }
}

impl Registry {
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .with_context(|| format!("invalid registry at {}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("cannot serialize registry")?;
        let staging = path.with_extension("toml.tmp");
        fs::write(&staging, text).with_context(|| format!("cannot write {}", staging.display()))?;
        fs::rename(&staging, path).with_context(|| format!("cannot replace {}", path.display()))?;
        Ok(())
    }

    pub fn find(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|tool| tool.name == name)
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Tool> {
        self.tools.iter_mut().find(|tool| tool.name == name)
    }

    pub fn upsert(&mut self, tool: Tool) {
        if let Some(existing) = self.find_mut(&tool.name) {
            *existing = tool;
        } else {
            self.tools.push(tool);
        }
    }
}

pub fn utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year,
        month,
        day,
        day_seconds / 3600,
        (day_seconds % 3600) / 60,
        day_seconds % 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (year + i64::from(month <= 2), month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tool() -> Tool {
        Tool {
            name: "ghcli".into(),
            version: "2.43.0".into(),
            source: "github".into(),
            owner: Some("cli".into()),
            repo: Some("cli".into()),
            tag: Some("v2.43.0".into()),
            track: "latest".into(),
            bin_name: "gh".into(),
            checksum: "sha256:abc".into(),
            linked: "~/.local/bin/gh".into(),
            updated: utc_now(),
            previous: None,
            url: None,
            asset: None,
        }
    }

    #[test]
    fn expands_tilde_only_when_prefixed() {
        let home = Path::new("/home/tester");
        assert_eq!(expand_tilde("~/.local/bin", home), home.join(".local/bin"));
        assert_eq!(expand_tilde("/opt/bin", home), PathBuf::from("/opt/bin"));
        assert_eq!(expand_tilde("~", home), home);
    }

    #[test]
    fn registry_roundtrip_preserves_spec_fields() {
        let tool = sample_tool();
        let registry = Registry {
            tools: vec![tool.clone()],
        };
        let text = toml::to_string_pretty(&registry).unwrap();
        assert!(text.contains("[[tools]]"));
        assert!(text.contains("bin-name = \"gh\""));
        let decoded: Registry = toml::from_str(&text).unwrap();
        assert_eq!(decoded.tools[0].bin_name, tool.bin_name);
        assert_eq!(decoded.tools[0].track, "latest");
    }

    #[test]
    fn linked_path_expands_home() {
        let tool = sample_tool();
        assert_eq!(
            tool.linked_path(Path::new("/home/tester")),
            PathBuf::from("/home/tester/.local/bin/gh")
        );
    }

    #[test]
    fn timestamp_is_rfc3339_utc() {
        let stamp = utc_now();
        assert_eq!(stamp.len(), 20);
        assert!(stamp.ends_with('Z'));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }
}
