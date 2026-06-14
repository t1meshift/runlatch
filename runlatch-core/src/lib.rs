#![doc = include_str!("../README.md")]
//!
//! ## Module map
//!
//! - [`model`] — the provider-agnostic [`AutostartEntry`] and [`Scope`].
//! - [`provider`] — the [`AutostartProvider`] trait and why it is `async`.
//! - [`providers`] — the built-in [`XdgAutostartProvider`] (user and system) and
//!   [`SystemdProvider`] (user and system).
//! - [`registry`] — the aggregating [`Registry`].
//! - [`desktop_file`] — an order-preserving `.desktop` reader/writer.

#[cfg(not(target_os = "linux"))]
compile_error!("runlatch only supports Linux");

pub mod desktop_file;
pub mod model;
pub mod provider;
pub mod providers;
pub mod registry;

pub use model::{AutostartEntry, Scope};
pub use provider::AutostartProvider;
pub use providers::{SystemdProvider, XdgAutostartProvider};
pub use registry::{AggregateResult, ProviderError, Registry};
