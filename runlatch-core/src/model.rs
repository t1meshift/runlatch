//! Provider-agnostic data model shared across all autostart providers.

use serde::Serialize;

/// Whether an autostart entry applies to the current user or the whole system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// User-level autostart (e.g. `~/.config/autostart`, systemd `--user` units).
    User,
    /// System-level autostart (e.g. `/etc/xdg/autostart`, systemd system units).
    System,
}

/// A single autostart item, normalized across providers.
///
/// `id` is stable and unique *within a provider*; pair it with [`AutostartEntry::source`]
/// (the provider id) to address an entry unambiguously across the whole registry.
///
/// ```
/// use runlatch_core::{AutostartEntry, Scope};
///
/// let entry = AutostartEntry {
///     id: "redshift".into(),
///     display_name: "Redshift".into(),
///     description: None,
///     command: "redshift-gtk".into(),
///     icon: None,
///     enabled: true,
///     source: "xdg-autostart".into(),
///     scope: Scope::User,
/// };
/// assert_eq!(format!("{}:{}", entry.source, entry.id), "xdg-autostart:redshift");
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct AutostartEntry {
    /// Stable, unique within a provider (e.g. a `.desktop` file stem or a unit name).
    pub id: String,
    /// Human-readable name for display.
    pub display_name: String,
    /// Optional longer description.
    pub description: Option<String>,
    /// The command that runs at startup.
    pub command: String,
    /// Optional icon name or path.
    pub icon: Option<String>,
    /// Whether the entry is currently enabled.
    pub enabled: bool,
    /// The id of the provider that produced this entry, e.g. `"systemd-user"`.
    pub source: String,
    /// Whether the entry is user- or system-scoped.
    pub scope: Scope,
}
