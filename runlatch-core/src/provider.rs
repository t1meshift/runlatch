//! The provider abstraction.
//!
//! Every autostart backend (XDG `.desktop`, systemd user units, and future ones
//! such as OpenRC or DE session restore) implements [`AutostartProvider`]. The
//! [`crate::registry::Registry`] aggregates providers behind this single trait.
//!
//! ## Why async
//!
//! The I/O methods are `async` on purpose: probing and querying some backends
//! (notably systemd over D-Bus) can block for a noticeable time. Keeping the core
//! `async` and runtime-agnostic means it only ever `await`s — it never spins up a
//! runtime or `block_on`s — so a GUI front-end can drive it without pinning its
//! render thread on a slow bus call. Binaries bring their own runtime.

use anyhow::Result;
use async_trait::async_trait;

use crate::model::{AutostartEntry, Scope};

/// A backend that can enumerate and manage autostart entries.
///
/// Implementors must be `Send + Sync` so the registry can hold them as
/// `Box<dyn AutostartProvider>` and share them across tasks.
#[async_trait]
pub trait AutostartProvider: Send + Sync {
    /// Stable provider id, e.g. `"xdg-autostart"`. Used as the `source` on entries
    /// and as the left side of a `source:id` address.
    fn id(&self) -> &'static str;

    /// The scope this provider manages.
    fn scope(&self) -> Scope;

    /// Real runtime detection: returns `false` when the underlying subsystem,
    /// paths, or bus aren't present. Providers that report `false` are silently
    /// skipped by the registry. Must never panic.
    async fn is_available(&self) -> bool;

    /// Enumerate the provider's autostart entries, each tagged with this
    /// provider's id as `source`.
    async fn entries(&self) -> Result<Vec<AutostartEntry>>;

    /// Enable the entry with the given id.
    async fn enable(&self, id: &str) -> Result<()>;

    /// Disable the entry with the given id (without destroying it where possible).
    async fn disable(&self, id: &str) -> Result<()>;

    /// Create a new autostart entry.
    async fn add(&self, entry: &AutostartEntry) -> Result<()>;

    /// Remove the entry with the given id entirely.
    async fn remove(&self, id: &str) -> Result<()>;
}
