use anyhow::{bail, Context, Result};
use std::fs;

use crate::install::{replace_installed, stash_previous, Replacement};
use crate::registry::{Config, Paths, PreviousVersion, Registry};
use crate::source::{
    checksum_of, compare_versions, fetch_release, pick_asset, published_checksum, verify_checksum,
    version_from_tag, Client,
};
use crate::ui::Ui;

pub fn update(paths: &Paths, config: &Config, ui: &Ui, name: &str) -> Result<()> {
    let registry = Registry::load(&paths.registry_file)?;
    let tool = registry
        .find(name)
        .with_context(|| format!("{} is not installed", name))?
        .clone();
    if tool.source == "url" {
        bail!(
            "direct URL tools cannot be updated automatically; re-run `berth add` with a new --url"
        );
    }
    let (owner, repo) = match (&tool.owner, &tool.repo) {
        (Some(owner), Some(repo)) => (owner.clone(), repo.clone()),
        _ => bail!("registry entry for {} is missing owner/repo", name),
    };

    let client = Client::new(config)?;
    let checking = ui.step("checking GitHub for updates...");
    let pinned = tool.track == "exact";
    let release = fetch_release(
        &client,
        &owner,
        &repo,
        if pinned { tool.tag.as_deref() } else { None },
    )?;
    let version = version_from_tag(&release.tag_name);
    if compare_versions(&version, &tool.version) != std::cmp::Ordering::Greater {
        ui.finish_step(&checking, "already up to date");
        ui.result(&format!("{} {} is already up to date", name, tool.version));
        return Ok(());
    }
    ui.finish_step(&checking, &format!("{} available", version));

    let asset = pick_asset(&release, tool.asset.as_deref())?;
    let expected = published_checksum(&client, &release, asset).unwrap_or_else(|| checksum_of(&[]));
    let bar = ui.download_bar(None, &format!("downloading {}", asset.name));
    let bytes = client.download(&asset.browser_download_url, &bar)?;
    bar.finish_and_clear();

    let verifying = ui.step("verifying sha256...");
    let checksum = if expected == checksum_of(&[]) {
        let computed = checksum_of(&bytes);
        ui.finish_step(&verifying, "recorded sha256");
        computed
    } else {
        verify_checksum(&bytes, &expected)
            .with_context(|| format!("checksum check failed for {}", asset.name))?;
        ui.finish_step(&verifying, "verified sha256");
        expected
    };

    let stashing = ui.step("stashing last-known-good version...");
    let previous = stash_previous(paths, &tool)?;
    ui.finish_step(&stashing, "stashed previous version");

    let replacement = Replacement {
        version: version.clone(),
        asset_name: asset.name.clone(),
        checksum,
        tag: Some(release.tag_name.clone()),
        previous,
    };
    if let Err(error) = replace_installed(paths, ui, &tool, &replacement, &bytes) {
        let _ = crate::install::link_binary(paths, &tool, ui);
        return Err(error);
    }
    ui.success(&format!(
        "{} updated {} -> {} (rollback available)",
        name, tool.version, version
    ));
    Ok(())
}

pub fn rollback(paths: &Paths, ui: &Ui, name: &str) -> Result<()> {
    let registry = Registry::load(&paths.registry_file)?;
    let tool = registry
        .find(name)
        .with_context(|| format!("{} is not installed", name))?
        .clone();
    let last_good = tool
        .previous
        .as_ref()
        .with_context(|| format!("no last-known-good version recorded for {}", name))?
        .clone();

    let checking = ui.step("verifying cached version...");
    let cached = paths
        .cache_dir
        .join(&tool.name)
        .join(&last_good.version)
        .join(&tool.bin_name);
    let bytes = fs::read(&cached)
        .with_context(|| format!("cached binary is missing: {}", cached.display()))?;
    verify_checksum(&bytes, &last_good.checksum)?;
    ui.finish_step(&checking, "cached version verified");

    let replacement = Replacement {
        version: last_good.version.clone(),
        asset_name: tool.bin_name.clone(),
        checksum: last_good.checksum.clone(),
        tag: last_good.tag.clone(),
        previous: PreviousVersion {
            version: tool.version.clone(),
            tag: tool.tag.clone(),
            checksum: tool.checksum.clone(),
        },
    };
    replace_installed(paths, ui, &tool, &replacement, &bytes)?;
    ui.success(&format!(
        "{} rolled back {} -> {}",
        name, tool.version, last_good.version
    ));
    Ok(())
}
