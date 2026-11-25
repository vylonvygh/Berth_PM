use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "berth", version, about = "Manage standalone executables")]
pub struct Cli {
    #[arg(
        long,
        global = true,
        help = "Suppress spinners, progress and decorative output"
    )]
    pub quiet: bool,
    #[arg(long, global = true, help = "Disable ANSI colors")]
    pub no_color: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Install a tool from a GitHub release or a direct URL")]
    Add {
        name: String,
        #[arg(long, help = "GitHub repository as owner/repo")]
        repo: Option<String>,
        #[arg(long, help = "Pin to an exact release tag")]
        tag: Option<String>,
        #[arg(long, help = "Download a standalone executable from this URL")]
        url: Option<String>,
        #[arg(long = "bin-name", help = "Name of the executable inside the download")]
        bin_name: Option<String>,
    },
    #[command(about = "Remove a tool's symlink and registry entry")]
    Remove { name: String },
    #[command(about = "List installed tools")]
    List {
        #[arg(long, help = "Emit machine-readable JSON")]
        json: bool,
    },
    #[command(about = "Check GitHub and update a tool if a newer release exists")]
    Update { name: String },
    #[command(about = "Show details for one tool")]
    Info { name: String },
    #[command(about = "Restore the last-known-good version")]
    Rollback { name: String },
    #[command(about = "Rebuild broken symlinks and warn about missing binaries")]
    Sync,
    #[command(about = "Filter installed tools by name, owner or repository")]
    Search { query: String },
    #[command(about = "Verify that every managed symlink is intact")]
    Status,
}

impl Cli {
    pub fn machine_readable(&self) -> bool {
        matches!(&self.command, Command::List { json: true })
    }
}
