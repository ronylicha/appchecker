use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "appchecker")]
#[command(
    about = "Monitor multiple git repositories, track dependency updates, and receive notifications"
)]
#[command(version)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "~/.config/appchecker/config.toml")]
    pub config: PathBuf,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Add a project directory to monitor
    Add {
        /// Path to the project directory
        path: PathBuf,

        /// Optional name for the project (defaults to directory name)
        #[arg(short, long)]
        name: Option<String>,

        /// Tags for grouping (comma-separated)
        #[arg(short, long)]
        tags: Option<String>,

        /// Pin this project to top of list
        #[arg(long)]
        pin: bool,
    },

    /// Remove a project from monitoring
    Remove {
        /// Project name or path to remove
        project: String,
    },

    /// List all monitored projects
    List {
        /// Show detailed information
        #[arg(short, long)]
        detailed: bool,

        /// Filter by tag
        #[arg(short, long)]
        tag: Option<String>,
    },

    /// Run a one-time check of all projects
    Check {
        /// Only check specific project
        #[arg(short, long)]
        project: Option<String>,

        /// Output format: text, json
        #[arg(short, long, default_value = "text")]
        format: String,
    },

    /// Auto-discover projects in configured directories
    Discover {
        /// Override search paths (comma-separated)
        #[arg(short, long)]
        paths: Option<String>,

        /// Maximum directory depth (defaults to the configured value)
        #[arg(short, long)]
        depth: Option<usize>,

        /// Dry run - show what would be added without adding
        #[arg(long)]
        dry_run: bool,
    },

    /// Start the daemon for background monitoring and notifications
    Daemon {
        /// Run in foreground (don't daemonize)
        #[arg(long)]
        foreground: bool,

        /// Scan interval in seconds (defaults to the configured value)
        #[arg(short, long)]
        interval: Option<u64>,
    },

    /// Show current configuration
    Config {
        /// Reset to defaults
        #[arg(long)]
        reset: bool,

        /// Show config file path only
        #[arg(long)]
        path: bool,
    },

    /// Launch the native desktop dashboard
    Gui,

    /// Launch the interactive TUI dashboard
    Tui,
}
