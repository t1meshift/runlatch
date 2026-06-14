# runlatch

A modular Linux autostart manager — one view and control surface for entries scattered across
systemd units, XDG `.desktop` files, and DE-specific session configs.

Linux autostart is fragmented: each subsystem manages its own slice of "things that run when you
log in", with no single place to see or toggle them all. runlatch unifies them behind a trait-based
provider system, so listing, enabling, and disabling works the same across every source — and new
backends are added without touching the core.

## Workspace

| Crate | Type | What it is |
| ----- | ---- | ---------- |
| [`runlatch`](runlatch/README.md) | CLI | The command-line tool — `cargo install runlatch`. See its README for usage, sources, and shell completions. |
| [`runlatch-core`](runlatch-core/README.md) | Library | The data model, the `AutostartProvider` trait, built-in providers, and the aggregating `Registry`. The extension point for new backends. |

[![crates.io](https://img.shields.io/crates/v/runlatch.svg)](https://crates.io/crates/runlatch)
[![runlatch-core](https://img.shields.io/crates/v/runlatch-core.svg?label=runlatch-core)](https://crates.io/crates/runlatch-core)
[![docs.rs](https://img.shields.io/docsrs/runlatch-core?label=docs.rs)](https://docs.rs/runlatch-core)

## License

Licensed under the [MIT](LICENSE-MIT) license.
