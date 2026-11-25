use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

const DOWNLOAD_TEMPLATE: &str =
    "    {spinner:.cyan} {msg} {wide_bar:.cyan/blue} {percent}%  {bytes}/{total_bytes} @ {bytes_per_sec}";

pub struct Ui {
    quiet: bool,
}

impl Ui {
    pub fn new(quiet: bool, no_color: bool) -> Self {
        colored::control::set_override(!no_color);
        Self { quiet }
    }

    pub fn step(&self, message: impl Into<String>) -> ProgressBar {
        if self.quiet {
            return ProgressBar::hidden();
        }
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::with_template("    {spinner:.cyan} {msg}")
                .expect("valid spinner template"),
        );
        bar.set_message(message.into());
        bar.enable_steady_tick(Duration::from_millis(80));
        bar
    }

    pub fn finish_step(&self, bar: &ProgressBar, message: &str) {
        if !self.quiet {
            bar.finish_with_message(message.to_string());
        }
    }

    pub fn download_bar(&self, length: Option<u64>, message: &str) -> ProgressBar {
        if self.quiet {
            return ProgressBar::hidden();
        }
        let bar = match length {
            Some(len) => ProgressBar::new(len),
            None => ProgressBar::new_spinner(),
        };
        bar.set_style(
            ProgressStyle::with_template(DOWNLOAD_TEMPLATE).expect("valid download template"),
        );
        bar.set_message(message.to_string());
        bar
    }

    pub fn success(&self, message: &str) {
        if !self.quiet {
            println!("{} {}", "✓".green(), message);
        }
    }

    pub fn result(&self, message: &str) {
        println!("{}", message);
    }

    pub fn detail(&self, label: &str, value: &str) {
        println!("{:<11} {}", label, value);
    }

    pub fn warn(&self, message: &str) {
        eprintln!("{} {}", "!".yellow(), message);
    }

    pub fn error(&self, message: &str) {
        eprintln!("{} {}", "✗".red(), message);
    }
}
