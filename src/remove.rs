use anyhow::{bail, Context, Result};
use std::fs;

use crate::registry::{Paths, Registry};
use crate::ui::Ui;

pub fn remove(paths: &Paths, ui: &Ui, name: &str) -> Result<()> {
    let mut registry = Registry::load(&paths.registry_file)?;
    let tool = registry
        .find(name)
        .with_context(|| format!("{} is not installed", name))?
        .clone();
    let link = tool.linked_path(&paths.home);
    let expected = tool.store_path(&paths.store_dir);

    match fs::symlink_metadata(&link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let target = fs::read_link(&link)?;
            if target != expected {
                bail!(
                    "{} points to {} instead of a berth binary; refusing to remove it",
                    link.display(),
                    target.display()
                );
            }
            fs::remove_file(&link)?;
        }
        Ok(_) => bail!("{} is a regular file, not a berth symlink", link.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ui.warn(&format!(
                "symlink {} was already missing",
                paths.display(&link)
            ));
        }
        Err(error) => {
            return Err(error).with_context(|| format!("cannot inspect {}", link.display()))
        }
    }

    registry.tools.retain(|candidate| candidate.name != name);
    registry.save(&paths.registry_file)?;
    ui.success(&format!(
        "{} removed; the binary is still cached in {}",
        name,
        paths.display(&paths.cache_dir.join(name))
    ));
    Ok(())
}
