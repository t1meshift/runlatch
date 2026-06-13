//! Command-line interface definition (clap derive).

use clap::{Parser, Subcommand, ValueEnum};

/// runlatch — a modular Linux autostart manager.
#[derive(Debug, Parser)]
#[command(name = "runlatch", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List autostart entries, grouped by source.
    List {
        /// Only show entries from this provider id (e.g. `xdg-autostart`).
        #[arg(long)]
        source: Option<String>,
        /// Emit JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Enable an entry, addressed as `<id>` or `<source>:<id>`.
    Enable {
        /// The entry to enable.
        target: String,
    },
    /// Disable an entry, addressed as `<id>` or `<source>:<id>`.
    Disable {
        /// The entry to disable.
        target: String,
    },
    /// List providers and whether each is available on this machine.
    Sources,
    /// Print a shell completion script to stdout.
    ///
    /// bash:  source <(runlatch completions bash)
    ///
    /// fish:  runlatch completions fish > ~/.config/fish/completions/runlatch.fish
    ///
    /// zsh:   runlatch completions zsh > "${fpath[1]}/_runlatch"
    Completions {
        /// The shell to generate completions for.
        shell: Shell,
    },
    /// Output plain-text completion candidates for use in shell completion scripts.
    ///
    /// Subcommands: entries (one source:id per line), sources (one provider id per line).
    /// Called internally by the scripts generated with `runlatch completions <shell>`.
    Complete {
        #[command(subcommand)]
        what: CompleteCmd,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Shell {
    Bash,
    Fish,
    Zsh,
}

/// What to complete.
#[derive(Debug, Subcommand)]
pub enum CompleteCmd {
    /// Output `source:id` for every entry — for completing enable/disable targets.
    Entries,
    /// Output available provider ids — for completing --source.
    Sources,
}
