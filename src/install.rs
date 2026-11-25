use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use serde::Serialize;
use std::fs;
use std::io::{Cursor, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path};

use crate::registry::{utc_now, Config, Paths, PreviousVersion, Registry, Tool};
use crate::source::{
    checksum_of, fetch_release, pick_asset, published_checksum, verify_checksum, version_from_tag,
    Client,
};
use crate::ui::Ui;

#[derive(Debug, Clone)]
pub struct AddOptions {
    pub name: String,
    pub repo: Option<String>,
    pub tag: Option<String>,
    pub url: Option<String>,
    pub bin_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ResolvedSource {
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub tag: Option<String>,
    pub version: String,
    pub asset_name: String,
    pub url: String,
    pub bin_name: String,
    pub checksum: String,
}

fn alias_repo(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match name {
        "ghcli" => Some(("cli", "cli", "gh")),
        _ => None,
    }
}

pub fn add(paths: &Paths, config: &Config, ui: &Ui, options: &AddOptions) -> Result<()> {
    let mut registry = Registry::load(&paths.registry_file)?;
    if registry.find(&options.name).is_some() {
        bail!(
            "{} is already installed; use `berth update {}`",
            options.name,
            options.name
        );
    }

    let client = Client::new(config)?;
    let spinner = ui.step("resolving version...");
    let resolved = resolve(&client, ui, options)?;
    ui.finish_step(
        &spinner,
        &format!(
            "resolved {}@{}",
            resolved.url,
            resolved.tag.as_deref().unwrap_or(&resolved.version)
        ),
    );

    let bar = ui.download_bar(None, &format!("downloading {}", resolved.asset_name));
    let bytes = client.download(&resolved.url, &bar)?;
    bar.finish_and_clear();

    let verifying = ui.step("verifying sha256...");
    if resolved.checksum == checksum_of(&[]) {
        ui.finish_step(&verifying, "recorded sha256");
    } else {
        verify_checksum(&bytes, &resolved.checksum)
            .with_context(|| format!("checksum check failed for {}", resolved.asset_name))?;
        ui.finish_step(&verifying, "verified sha256");
    }

    let staging = paths
        .cache_dir
        .join("staging")
        .join(&options.name)
        .join(&resolved.version);
    let result = install_bytes(
        paths,
        ui,
        options,
        &resolved,
        &bytes,
        &staging,
        &mut registry,
    );
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;

    ui.success(&format!("{} {} installed", options.name, resolved.version));
    Ok(())
}

fn resolve(client: &Client, ui: &Ui, options: &AddOptions) -> Result<ResolvedSource> {
    if let Some(url) = &options.url {
        let bin_name = options
            .bin_name
            .clone()
            .or_else(|| {
                Path::new(url)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
            })
            .context("--bin-name is required to install from a URL")?;
        ui.warn("direct URLs have no published checksum; recording the computed sha256");
        return Ok(ResolvedSource {
            owner: None,
            repo: None,
            tag: options.tag.clone(),
            version: options
                .tag
                .as_deref()
                .map(version_from_tag)
                .unwrap_or_else(|| "local".to_string()),
            asset_name: Path::new(url)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("download")
                .to_string(),
            url: url.clone(),
            bin_name,
            checksum: checksum_of(&[]),
        });
    }

    let (owner, repo, default_bin) = match (&options.repo, alias_repo(&options.name)) {
        (Some(repo), _) => parse_repo(repo)?,
        (None, Some((owner, repo, bin))) => (owner.to_string(), repo.to_string(), bin.to_string()),
        (None, None) => bail!(
            "no GitHub repository known for '{}'. Use --repo owner/repo (or --url with --bin-name)",
            options.name
        ),
    };
    let release = fetch_release(client, &owner, &repo, options.tag.as_deref())?;
    let asset = pick_asset(&release, None)?;
    let bin_name = options
        .bin_name
        .clone()
        .unwrap_or_else(|| default_bin_name(&default_bin, &asset.name));
    let checksum = published_checksum(client, &release, asset).unwrap_or_else(|| checksum_of(&[]));
    if checksum == checksum_of(&[]) {
        ui.warn("release has no published sha256; recording the checksum after download");
    }
    Ok(ResolvedSource {
        owner: Some(owner),
        repo: Some(repo),
        tag: options.tag.clone().or(Some(release.tag_name.clone())),
        version: version_from_tag(&release.tag_name),
        asset_name: asset.name.clone(),
        url: asset.browser_download_url.clone(),
        bin_name,
        checksum,
    })
}

fn parse_repo(repo: &str) -> Result<(String, String, String)> {
    let mut parts = repo.split('/');
    let owner = parts
        .next()
        .filter(|part| !part.is_empty())
        .context("--repo must be owner/repo")?;
    let repo_name = parts
        .next()
        .filter(|part| !part.is_empty())
        .context("--repo must be owner/repo")?;
    if parts.next().is_some() {
        bail!("--repo must be owner/repo");
    }
    Ok((
        owner.to_string(),
        repo_name.to_string(),
        repo_name.to_string(),
    ))
}

fn default_bin_name(default_bin: &str, asset_name: &str) -> String {
    let lower = asset_name.to_ascii_lowercase();
    if !(lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".tar.xz")
        || lower.ends_with(".txz")
        || lower.ends_with(".zip"))
    {
        return Path::new(asset_name)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(default_bin)
            .to_string();
    }
    default_bin.to_string()
}

fn install_bytes(
    paths: &Paths,
    ui: &Ui,
    options: &AddOptions,
    resolved: &ResolvedSource,
    bytes: &[u8],
    staging: &Path,
    registry: &mut Registry,
) -> Result<()> {
    if staging.exists() {
        fs::remove_dir_all(staging)?;
    }
    fs::create_dir_all(staging)?;
    extract_binary(bytes, &resolved.asset_name, &resolved.bin_name, staging)?;

    let version_dir = paths.store_dir.join(&options.name).join(&resolved.version);
    fs::create_dir_all(&version_dir)?;
    let final_binary = version_dir.join(&resolved.bin_name);
    fs::rename(staging.join(&resolved.bin_name), &final_binary)?;
    let installed_bytes = fs::read(&final_binary)
        .with_context(|| format!("cannot read {}", final_binary.display()))?;
    let installed_checksum = checksum_of(&installed_bytes);

    let link = paths.bin_dir.join(&resolved.bin_name);
    let tool = Tool {
        name: options.name.clone(),
        version: resolved.version.clone(),
        source: if options.url.is_some() {
            "url"
        } else {
            "github"
        }
        .to_string(),
        owner: resolved.owner.clone(),
        repo: resolved.repo.clone(),
        tag: resolved.tag.clone(),
        track: if options.tag.is_some() {
            "exact".to_string()
        } else {
            "latest".to_string()
        },
        bin_name: resolved.bin_name.clone(),
        checksum: installed_checksum,
        linked: paths.display(&link),
        updated: utc_now(),
        previous: None,
        url: options.url.clone(),
        asset: Some(resolved.asset_name.clone()),
    };
    link_binary(paths, &tool, ui)?;
    registry.upsert(tool);
    registry.save(&paths.registry_file)?;
    Ok(())
}

pub struct Replacement {
    pub version: String,
    pub asset_name: String,
    pub checksum: String,
    pub tag: Option<String>,
    pub previous: PreviousVersion,
}

pub fn replace_installed(
    paths: &Paths,
    ui: &Ui,
    tool: &Tool,
    replacement: &Replacement,
    bytes: &[u8],
) -> Result<()> {
    let staging = paths
        .cache_dir
        .join("staging")
        .join(&tool.name)
        .join(&replacement.version);
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    verify_checksum(bytes, &replacement.checksum)?;
    extract_binary(bytes, &replacement.asset_name, &tool.bin_name, &staging)?;

    let version_dir = paths.store_dir.join(&tool.name).join(&replacement.version);
    fs::create_dir_all(&version_dir)?;
    let installed_binary = version_dir.join(&tool.bin_name);
    fs::rename(staging.join(&tool.bin_name), &installed_binary)?;
    let installed_bytes = fs::read(&installed_binary)
        .with_context(|| format!("cannot read {}", installed_binary.display()))?;
    let installed_checksum = checksum_of(&installed_bytes);

    let mut new_tool = tool.clone();
    new_tool.version = replacement.version.clone();
    new_tool.checksum = installed_checksum;
    new_tool.tag = replacement.tag.clone();
    new_tool.asset = Some(replacement.asset_name.clone());
    new_tool.previous = Some(replacement.previous.clone());
    new_tool.updated = utc_now();

    link_binary(paths, &new_tool, ui)?;
    let mut registry = Registry::load(&paths.registry_file)?;
    registry.upsert(new_tool);
    if let Err(error) = registry.save(&paths.registry_file) {
        let _ = link_binary(paths, tool, ui);
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

pub fn extract_binary(
    bytes: &[u8],
    asset_name: &str,
    bin_name: &str,
    dest_dir: &Path,
) -> Result<()> {
    let lower = asset_name.to_ascii_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        let decoder = GzDecoder::new(bytes);
        return extract_tar(decoder, asset_name, bin_name, dest_dir);
    }
    if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
        let decoder = xz2::read::XzDecoder::new(bytes);
        return extract_tar(decoder, asset_name, bin_name, dest_dir);
    }
    if lower.ends_with(".zip") {
        return extract_zip(bytes, asset_name, bin_name, dest_dir);
    }
    write_binary(bytes, &dest_dir.join(bin_name))
}

fn extract_tar<R: Read>(
    reader: R,
    asset_name: &str,
    bin_name: &str,
    dest_dir: &Path,
) -> Result<()> {
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path()?.into_owned();
        if !safe_member(&path, bin_name) {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        return write_binary(&bytes, &dest_dir.join(bin_name));
    }
    bail!(
        "archive {} contains no safe '{}' file",
        asset_name,
        bin_name
    )
}

fn extract_zip(bytes: &[u8], asset_name: &str, bin_name: &str, dest_dir: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .with_context(|| format!("cannot read zip archive {}", asset_name))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("cannot read entry {} in {}", index, asset_name))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name()?;
        if !safe_member(Path::new(name.as_ref()), bin_name) {
            continue;
        }
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents)?;
        return write_binary(&contents, &dest_dir.join(bin_name));
    }
    bail!(
        "archive {} contains no safe '{}' file",
        asset_name,
        bin_name
    )
}

fn safe_member(path: &Path, bin_name: &str) -> bool {
    if path.is_absolute() || path.as_os_str().to_string_lossy().contains('\\') {
        return false;
    }
    let mut valid = true;
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            _ => valid = false,
        }
    }
    valid && path.file_name().and_then(|name| name.to_str()) == Some(bin_name)
}

fn write_binary(bytes: &[u8], dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(dest, bytes).with_context(|| format!("cannot write {}", dest.display()))?;
    fs::set_permissions(dest, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("cannot make {} executable", dest.display()))?;
    Ok(())
}

pub fn link_binary(paths: &Paths, tool: &Tool, ui: &Ui) -> Result<()> {
    let link = tool.linked_path(&paths.home);
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let target = tool.store_path(&paths.store_dir);
    if !target.is_file() {
        bail!("binary is missing: {}", target.display());
    }
    let staging = link.with_file_name(format!(
        ".{}.berth-tmp",
        link.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("link")
    ));
    let _ = fs::remove_file(&staging);
    std::os::unix::fs::symlink(&target, &staging)
        .with_context(|| format!("cannot create staging symlink {}", staging.display()))?;
    fs::rename(&staging, &link)
        .with_context(|| format!("cannot install symlink {}", link.display()))?;
    ui.step(format!(
        "symlinking {}  ->  {}",
        paths.display(&link),
        paths.display(&target)
    ))
    .finish_and_clear();
    Ok(())
}

pub fn stash_previous(paths: &Paths, tool: &Tool) -> Result<PreviousVersion> {
    let current = tool.store_path(&paths.store_dir);
    let bytes = fs::read(&current).with_context(|| format!("cannot read {}", current.display()))?;
    verify_checksum(&bytes, &tool.checksum)?;
    let cached = paths
        .cache_dir
        .join(&tool.name)
        .join(&tool.version)
        .join(&tool.bin_name);
    if let Some(parent) = cached.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&current, &cached).with_context(|| format!("cannot cache {}", current.display()))?;
    Ok(PreviousVersion {
        version: tool.version.clone(),
        tag: tool.tag.clone(),
        checksum: tool.checksum.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar_gz_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        for (name, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
            header.set_cksum();
            builder.append(&header, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    use flate2::{write::GzEncoder, Compression};

    #[test]
    fn extracts_only_matching_binary_from_tar() {
        let dir = tempfile::tempdir().unwrap();
        let archive = tar_gz_with(&[("tool/README.md", b"docs"), ("tool/bin/mytool", b"binary")]);
        extract_binary(&archive, "tool-linux-x86_64.tar.gz", "mytool", dir.path()).unwrap();
        assert_eq!(fs::read(dir.path().join("mytool")).unwrap(), b"binary");
        assert!(!dir.path().join("README.md").exists());
    }

    #[test]
    fn rejects_path_traversal_in_tar() {
        let dir = tempfile::tempdir().unwrap();
        let archive = tar_gz_with(&[("../../evil", b"bad"), ("docs/mytool", b"ok")]);
        assert!(extract_binary(&archive, "tool.tar.gz", "evil", dir.path()).is_err());
        assert!(!dir.path().parent().unwrap().join("evil").exists());
    }

    #[test]
    fn rejects_plain_path_with_parent_component() {
        assert!(!safe_member(Path::new("../mytool"), "mytool"));
        assert!(!safe_member(Path::new("/abs/mytool"), "mytool"));
        assert!(!safe_member(Path::new("dir\\mytool"), "mytool"));
        assert!(safe_member(Path::new("pkg/bin/mytool"), "mytool"));
    }

    #[test]
    fn writes_executable_direct_binary() {
        let dir = tempfile::tempdir().unwrap();
        extract_binary(b"#!/bin/sh\n", "mytool", "mytool", dir.path()).unwrap();
        let path = dir.path().join("mytool");
        let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }
}
