use anyhow::{Context, Result};
use std::fs;

use crate::install::link_binary;
use crate::registry::{Paths, Registry};
use crate::ui::Ui;

pub fn sync(paths: &Paths, ui: &Ui) -> Result<()> {
    let registry = Registry::load(&paths.registry_file)?;
    if registry.tools.is_empty() {
        ui.result("No tools installed yet. Try `berth add ghcli`");
        return Ok(());
    }
    for tool in &registry.tools {
        let target = tool.store_path(&paths.store_dir);
        if !target.is_file() {
            ui.warn(&format!(
                "{} {} has no binary at {}",
                tool.name,
                tool.version,
                paths.display(&target)
            ));
            continue;
        }
        let link = tool.linked_path(&paths.home);
        match fs::read_link(&link) {
            Ok(existing) if existing == target => {}
            Ok(existing) => {
                link_binary(paths, tool, ui)?;
                ui.success(&format!(
                    "{} relinked from {} to {}",
                    tool.name,
                    existing.display(),
                    paths.display(&link)
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                link_binary(paths, tool, ui)?;
                ui.success(&format!("{} symlink restored", tool.name));
            }
            Err(error) => {
                return Err(error).with_context(|| format!("cannot inspect {}", link.display()));
            }
        }
    }
    Ok(())
}

pub fn status(paths: &Paths, ui: &Ui) -> Result<bool> {
    let registry = Registry::load(&paths.registry_file)?;
    if registry.tools.is_empty() {
        ui.result("No tools installed yet. Try `berth add ghcli`");
        return Ok(true);
    }
    let mut healthy = true;
    for tool in &registry.tools {
        let link = tool.linked_path(&paths.home);
        let target = tool.store_path(&paths.store_dir);
        let linked_to = fs::read_link(&link).ok();
        if linked_to.as_ref() != Some(&target) {
            ui.warn(&format!(
                "{}: symlink {} is missing or points elsewhere",
                tool.name,
                paths.display(&link)
            ));
            healthy = false;
        } else if !target.is_file() {
            ui.warn(&format!(
                "{}: dangling link, binary missing at {}",
                tool.name,
                paths.display(&target)
            ));
            healthy = false;
        } else {
            ui.result(&format!(
                "{} {}: ok ({} -> {})",
                tool.name,
                tool.version,
                paths.display(&link),
                paths.display(&target)
            ));
        }
    }
    Ok(healthy)
}
