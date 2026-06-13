//! The provider registry: the single entry point callers use to work with every
//! available autostart backend at once.

use crate::model::AutostartEntry;
use crate::provider::AutostartProvider;
use crate::providers::{SystemdProvider, XdgAutostartProvider};

/// An error raised by a single provider during aggregation, tagged with its source.
///
/// Aggregation never fails as a whole: a provider that errors contributes a
/// `ProviderError` here while every other provider's good results are still
/// returned.
#[derive(Debug)]
pub struct ProviderError {
    /// The id of the provider that failed.
    pub source: &'static str,
    /// The underlying error.
    pub error: anyhow::Error,
}

/// The result of aggregating entries across all available providers.
#[derive(Debug, Default)]
pub struct AggregateResult {
    /// All entries successfully gathered, across providers.
    pub entries: Vec<AutostartEntry>,
    /// Per-provider failures, if any. Non-empty does not mean `entries` is empty.
    pub errors: Vec<ProviderError>,
}

/// A collection of autostart providers.
///
/// Construct with [`Registry::with_defaults`] for the built-in providers, or
/// [`Registry::new`] to inject a caller-supplied set (the extension point for
/// external crates that ship their own providers).
pub struct Registry {
    providers: Vec<Box<dyn AutostartProvider>>,
}

impl Registry {
    /// Build a registry from an explicit list of providers.
    pub fn new(providers: Vec<Box<dyn AutostartProvider>>) -> Self {
        Self { providers }
    }

    /// Build a registry with the crate's built-in providers: XDG autostart and
    /// systemd user units.
    pub fn with_defaults() -> Self {
        Self::new(vec![
            Box::new(XdgAutostartProvider::new()),
            Box::new(SystemdProvider::user()),
            Box::new(SystemdProvider::system()),
        ])
    }

    /// All registered providers, regardless of availability.
    pub fn providers(&self) -> &[Box<dyn AutostartProvider>] {
        &self.providers
    }

    /// The providers whose [`is_available`](AutostartProvider::is_available) probe
    /// currently returns `true`.
    pub async fn available(&self) -> Vec<&dyn AutostartProvider> {
        let mut out = Vec::new();
        for provider in &self.providers {
            if provider.is_available().await {
                out.push(provider.as_ref());
            }
        }
        out
    }

    /// Look up a registered provider by its id.
    pub fn find_provider(&self, id: &str) -> Option<&dyn AutostartProvider> {
        self.providers
            .iter()
            .map(|p| p.as_ref())
            .find(|p| p.id() == id)
    }

    /// Aggregate entries across every available provider.
    ///
    /// A failure from one provider is collected into
    /// [`AggregateResult::errors`] and never aborts aggregation of the others.
    pub async fn all_entries(&self) -> AggregateResult {
        let mut result = AggregateResult::default();
        for provider in self.available().await {
            match provider.entries().await {
                Ok(entries) => result.entries.extend(entries),
                Err(error) => result.errors.push(ProviderError {
                    source: provider.id(),
                    error,
                }),
            }
        }
        result
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::with_defaults()
    }
}
