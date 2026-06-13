//! runlatch CLI entry point.
//!
//! The binary owns the async runtime (`#[tokio::main]`) and `.await`s into
//! `runlatch-core`; the core never starts a runtime of its own.

#[cfg(not(target_os = "linux"))]
compile_error!("runlatch only supports Linux");

mod cli;
mod completions;
mod output;

use anyhow::{Result, anyhow, bail};
use clap::Parser;
use runlatch_core::{AutostartProvider, Registry};

use crate::cli::{Cli, Command, CompleteCmd, Shell};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let registry = Registry::with_defaults();

    match cli.command {
        Command::List { source, json } => list(&registry, source, json).await,
        Command::Enable { target } => set_enabled(&registry, &target, true).await,
        Command::Disable { target } => set_enabled(&registry, &target, false).await,
        Command::Sources => sources(&registry).await,
        Command::Completions { shell } => print_completions(shell),
        Command::Complete { what } => complete(&registry, what).await,
    }
}

/// `runlatch list` — aggregate entries, surface per-provider errors on stderr.
async fn list(registry: &Registry, source: Option<String>, json: bool) -> Result<()> {
    let mut result = registry.all_entries().await;

    if let Some(source) = &source {
        result.entries.retain(|e| &e.source == source);
    }

    // Per-provider failures go to stderr but never suppress good results.
    for err in &result.errors {
        eprintln!("warning: provider '{}' failed: {:#}", err.source, err.error);
    }

    if json {
        output::print_json(&result.entries)?;
    } else {
        output::print_tables(&result.entries);
    }
    Ok(())
}

/// `runlatch enable|disable <target>`.
async fn set_enabled(registry: &Registry, target: &str, enable: bool) -> Result<()> {
    let (provider, id) = resolve(registry, target).await?;
    if enable {
        provider.enable(&id).await?;
        println!("enabled {}:{id}", provider.id());
    } else {
        provider.disable(&id).await?;
        println!("disabled {}:{id}", provider.id());
    }
    Ok(())
}

/// `runlatch sources` — list each provider and its availability.
async fn sources(registry: &Registry) -> Result<()> {
    for provider in registry.providers() {
        let available = provider.is_available().await;
        let mark = if available {
            "available"
        } else {
            "unavailable"
        };
        println!(
            "{:<16} scope={:?} [{mark}]",
            provider.id(),
            provider.scope()
        );
    }
    Ok(())
}

/// `runlatch completions <shell>` — print the completion script to stdout.
fn print_completions(shell: Shell) -> Result<()> {
    let script = match shell {
        Shell::Bash => completions::BASH,
        Shell::Fish => completions::FISH,
        Shell::Zsh => completions::ZSH,
    };
    print!("{}", script.trim_start_matches('\n'));
    Ok(())
}

/// `runlatch complete <what>` — plain-text candidates for shell completion scripts.
async fn complete(registry: &Registry, what: CompleteCmd) -> Result<()> {
    match what {
        CompleteCmd::Entries => {
            // Output `source:id` for every entry across all available providers.
            // Errors are silently suppressed — completions should never stderr.
            for provider in registry.available().await {
                if let Ok(entries) = provider.entries().await {
                    for entry in entries {
                        println!("{}:{}", entry.source, entry.id);
                    }
                }
            }
        }
        CompleteCmd::Sources => {
            for provider in registry.available().await {
                println!("{}", provider.id());
            }
        }
    }
    Ok(())
}

/// Resolve an entry address to a concrete provider and id.
///
/// Accepts `<source>:<id>` to target a provider directly, or a bare `<id>` that is
/// looked up across all available providers — unambiguous matches dispatch, an
/// ambiguous one errors with the qualified candidates.
async fn resolve<'r>(
    registry: &'r Registry,
    target: &str,
) -> Result<(&'r dyn AutostartProvider, String)> {
    // Source-qualified form: only treat the prefix as a source if it actually
    // names a known provider (ids themselves may contain ':').
    if let Some((maybe_source, rest)) = target.split_once(':')
        && let Some(provider) = registry.find_provider(maybe_source)
    {
        return Ok((provider, rest.to_string()));
    }

    // Bare id: search available providers for a matching entry id.
    let mut matches: Vec<&dyn AutostartProvider> = Vec::new();
    for provider in registry.available().await {
        match provider.entries().await {
            Ok(entries) => {
                if entries.iter().any(|e| e.id == target) {
                    matches.push(provider);
                }
            }
            Err(err) => {
                eprintln!("warning: provider '{}' failed: {:#}", provider.id(), err);
            }
        }
    }

    match matches.as_slice() {
        [] => Err(anyhow!("no autostart entry with id '{target}' found")),
        [provider] => Ok((*provider, target.to_string())),
        many => {
            let candidates = many
                .iter()
                .map(|p| format!("{}:{target}", p.id()))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("'{target}' is ambiguous across providers; qualify it as one of: {candidates}");
        }
    }
}
