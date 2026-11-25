use anyhow::Result;
use clap::Parser;
use colored::Colorize;

mod cli;
mod install;
mod registry;
mod remove;
mod source;
mod sync;
mod ui;
mod update;

use cli::{Cli, Command};
use install::AddOptions;
use registry::{load_config, Paths, Registry};
use ui::Ui;

fn main() {
    let cli = Cli::parse();
    let ui = Ui::new(cli.quiet, cli.no_color || cli.machine_readable());
    if let Err(error) = run(&cli, &ui) {
        ui.error(&format!("{:#}", error));
        std::process::exit(1);
    }
}

fn run(cli: &Cli, ui: &Ui) -> Result<()> {
    let paths = Paths::from_env()?;
    let config = load_config(&paths.config_file)?;

    match &cli.command {
        Command::Add {
            name,
            repo,
            tag,
            url,
            bin_name,
        } => install::add(
            &paths,
            &config,
            ui,
            &AddOptions {
                name: name.clone(),
                repo: repo.clone(),
                tag: tag.clone(),
                url: url.clone(),
                bin_name: bin_name.clone(),
            },
        ),
        Command::Remove { name } => remove::remove(&paths, ui, name),
        Command::List { json } => list(&paths, ui, *json),
        Command::Update { name } => update::update(&paths, &config, ui, name),
        Command::Info { name } => info(&paths, ui, name),
        Command::Rollback { name } => update::rollback(&paths, ui, name),
        Command::Sync => sync::sync(&paths, ui),
        Command::Search { query } => search(&paths, ui, query),
        Command::Status => {
            if !sync::status(&paths, ui)? {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

fn list(paths: &Paths, ui: &Ui, json: bool) -> Result<()> {
    let registry = Registry::load(&paths.registry_file)?;
    if json {
        let rows: Vec<_> = registry
            .tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name,
                    "version": tool.version,
                    "source": tool.source,
                    "owner": tool.owner,
                    "repo": tool.repo,
                    "bin_name": tool.bin_name,
                    "linked": tool.linked,
                    "checksum": tool.checksum,
                })
            })
            .collect();
        ui.result(&serde_json::to_string(&rows)?);
        return Ok(());
    }
    if registry.tools.is_empty() {
        ui.result("No tools installed yet. Try `berth add ghcli`");
        return Ok(());
    }
    ui.result(
        &format!(
            "{:<16} {:<10} {:<10} {}",
            "NAME", "VERSION", "SOURCE", "SYMLINK"
        )
        .bold(),
    );
    for tool in &registry.tools {
        ui.result(&format!(
            "{:<16} {:<10} {:<10} {}",
            tool.name, tool.version, tool.source, tool.linked
        ));
    }
    Ok(())
}

fn info(paths: &Paths, ui: &Ui, name: &str) -> Result<()> {
    let registry = Registry::load(&paths.registry_file)?;
    let tool = registry
        .find(name)
        .ok_or_else(|| anyhow::anyhow!("{} is not installed", name))?;
    ui.detail("name", &tool.name);
    ui.detail("version", &tool.version);
    ui.detail("source", &tool.source);
    let repository = match (&tool.owner, &tool.repo) {
        (Some(owner), Some(repo)) => format!("github.com/{}/{}", owner, repo),
        _ => tool.url.clone().unwrap_or_else(|| "-".to_string()),
    };
    ui.detail("source url", &repository);
    ui.detail("binary", &paths.display(&tool.store_path(&paths.store_dir)));
    ui.detail("symlink", &tool.linked);
    ui.detail("checksum", &tool.checksum);
    ui.detail("updated", &tool.updated);
    let last_good = tool
        .previous
        .as_ref()
        .map(|previous| previous.version.clone())
        .unwrap_or_else(|| "-".to_string());
    ui.detail("last good", &last_good);
    Ok(())
}

fn search(paths: &Paths, ui: &Ui, query: &str) -> Result<()> {
    let registry = Registry::load(&paths.registry_file)?;
    let needle = query.to_ascii_lowercase();
    let matches: Vec<_> = registry
        .tools
        .iter()
        .filter(|tool| {
            tool.name.to_ascii_lowercase().contains(&needle)
                || tool
                    .repo
                    .as_deref()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .contains(&needle)
                || tool
                    .owner
                    .as_deref()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .contains(&needle)
        })
        .collect();
    if matches.is_empty() {
        ui.result(&format!("No installed tools match '{}'", query));
        return Ok(());
    }
    for tool in matches {
        ui.result(&format!(
            "{:<16} {:<10} {}",
            tool.name, tool.version, tool.linked
        ));
    }
    Ok(())
}
