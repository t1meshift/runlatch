# runlatch-core

The data model, provider abstraction, built-in providers, and registry that power
[runlatch](https://crates.io/crates/runlatch), a modular Linux autostart manager.

Linux autostart is spread across systemd units, XDG `.desktop` files, and DE-specific
session configs. This crate unifies them behind one trait, [`AutostartProvider`], aggregated
by a [`Registry`]. New backends (OpenRC, KDE, GNOME, …) are added by implementing the trait —
no changes to the core.

## Quick start

Enumerate every autostart entry on the machine across all built-in providers:

```rust,no_run
use runlatch_core::Registry;

# async fn run() -> anyhow::Result<()> {
let registry = Registry::with_defaults();
let result = registry.all_entries().await;

for entry in &result.entries {
    let mark = if entry.enabled { "●" } else { "○" };
    println!("{mark} {}:{}  {}", entry.source, entry.id, entry.command);
}

// Aggregation never fails as a whole: a provider that errored is reported here
// while every other provider's entries are still returned above.
for err in &result.errors {
    eprintln!("warning: provider '{}' failed: {:#}", err.source, err.error);
}
# Ok(())
# }
```

The crate is runtime-agnostic — it only ever `.await`s, so the binary brings its own runtime
(see [Architecture](#architecture)).

## Writing a provider

A backend is any type implementing [`AutostartProvider`]. It must be `Send + Sync` so the
registry can hold it as a `Box<dyn AutostartProvider>` and share it across tasks. Implement the
seven methods, then hand an instance to [`Registry::new`]:

```rust,no_run
use anyhow::Result;
use async_trait::async_trait;
use runlatch_core::{AutostartEntry, AutostartProvider, Registry, Scope};

struct MyProvider;

#[async_trait]
impl AutostartProvider for MyProvider {
    /// Stable id, also the left side of a `source:id` address.
    fn id(&self) -> &'static str { "my-provider" }

    fn scope(&self) -> Scope { Scope::User }

    /// Real runtime detection. Return `false` (never panic) when the backend
    /// isn't present — the registry then silently skips this provider.
    async fn is_available(&self) -> bool { true }

    async fn entries(&self) -> Result<Vec<AutostartEntry>> { Ok(vec![]) }
    async fn enable(&self, _id: &str) -> Result<()> { Ok(()) }
    async fn disable(&self, _id: &str) -> Result<()> { Ok(()) }
    async fn add(&self, _entry: &AutostartEntry) -> Result<()> { Ok(()) }
    async fn remove(&self, _id: &str) -> Result<()> { Ok(()) }
}

# async fn run() {
// Mix your provider in with the built-ins, or run it standalone.
let registry = Registry::new(vec![Box::new(MyProvider)]);
let _ = registry.all_entries().await;
# }
```

Each [`AutostartEntry`] you return is tagged with your `id` as its `source`, so callers can
address it unambiguously as `my-provider:<entry-id>`.

## Architecture

**Async on purpose.** The I/O methods are `async` because probing and querying some backends
(notably systemd over D-Bus) can block for a noticeable time. Keeping the core `async` and
runtime-agnostic means it only ever `.await`s — it never spins up a runtime or `block_on`s — so a
GUI front-end can drive it without pinning its render thread on a slow bus call.

**Aggregation never fails as a whole.** [`Registry::all_entries`] collects a failing provider's
error into [`AggregateResult::errors`] while still returning every other provider's good results.
One broken backend never blanks the listing.

**Order-preserving `.desktop` editing.** The XDG provider toggles entries by setting/removing the
`Hidden` key via [`DesktopFile`], which edits a single key while leaving comments, ordering, and
unrelated keys byte-for-byte intact.

## License

Licensed under the [MIT](../LICENSE-MIT) license.

[`AutostartProvider`]: https://docs.rs/runlatch-core/latest/runlatch_core/provider/trait.AutostartProvider.html
[`Registry`]: https://docs.rs/runlatch-core/latest/runlatch_core/registry/struct.Registry.html
[`Registry::new`]: https://docs.rs/runlatch-core/latest/runlatch_core/registry/struct.Registry.html#method.new
[`Registry::all_entries`]: https://docs.rs/runlatch-core/latest/runlatch_core/registry/struct.Registry.html#method.all_entries
[`AutostartEntry`]: https://docs.rs/runlatch-core/latest/runlatch_core/model/struct.AutostartEntry.html
[`AggregateResult::errors`]: https://docs.rs/runlatch-core/latest/runlatch_core/registry/struct.AggregateResult.html#structfield.errors
[`DesktopFile`]: https://docs.rs/runlatch-core/latest/runlatch_core/desktop_file/struct.DesktopFile.html
