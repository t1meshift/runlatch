#[cfg(not(target_os = "linux"))]
compile_error!("runlatch only supports Linux");

//! `runlatch-core` — the data model, provider abstraction, built-in providers, and
//! registry that power [runlatch](https://github.com/sh1ftr/runlatch), a modular
//! Linux autostart manager.
//!
//! Linux autostart is spread across systemd units, XDG `.desktop` files, and
//! DE-specific session configs. This crate unifies them behind one trait,
//! [`AutostartProvider`], aggregated by a [`Registry`]. New backends (OpenRC, KDE,
//! GNOME, …) are added by implementing the trait — no changes to the core.
//!
//! See [`provider`] for why the trait is `async`, and the crate README for a
//! walkthrough on writing a provider.

pub mod desktop_file;
pub mod model;
pub mod provider;
pub mod providers;
pub mod registry;

pub use model::{AutostartEntry, Scope};
pub use provider::AutostartProvider;
pub use providers::{SystemdProvider, XdgAutostartProvider};
pub use registry::{AggregateResult, ProviderError, Registry};
