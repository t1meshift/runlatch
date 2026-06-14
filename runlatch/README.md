# runlatch

A unified view and control surface for Linux autostart entries.

Linux autostart is fragmented: systemd user units, XDG `~/.config/autostart/*.desktop`
files, and DE-specific session configs each manage a slice of "things that run when you
log in", with no single place to see or toggle them all. **runlatch** lists, enables, and
disables entries across every source from one command.

This is the command-line front-end. The provider abstraction and built-in backends live in
[`runlatch-core`](https://crates.io/crates/runlatch-core).

## Installation

```sh
cargo install runlatch
```

## Usage

```sh
runlatch list                            # all entries, grouped by source
runlatch list --source xdg-autostart    # entries from one source only
runlatch list --scope user              # only user- (or system-) scoped entries
runlatch list --json                    # machine-readable output

runlatch enable redshift                # enable by id (unambiguous bare id)
runlatch disable xdg-autostart:redshift # or qualify with source:id

runlatch sources                        # list available sources on this machine
```

`list` shows a table per source. Enabled entries are marked `●`, disabled `○`.

**Entry addressing.** `enable` and `disable` accept either a bare `<id>` (resolved
across all sources — rejected with suggestions if ambiguous) or a fully qualified
`<source>:<id>` to target a specific source directly.

## Sources

| Source id              | What it manages                                        |
| ---------------------- | ------------------------------------------------------ |
| `xdg-autostart`        | `~/.config/autostart/*.desktop` files                  |
| `xdg-autostart-system` | `$XDG_CONFIG_DIRS/autostart/*.desktop` files (default `/etc/xdg/autostart`; enable/disable may need elevation) |
| `systemd-user`         | systemd user units (requires an active session bus)    |
| `systemd-system`       | systemd system units (listing read-only; enable/disable may need elevation) |

Sources that are not available on the current machine (e.g. no session bus) are
silently skipped; errors in one source never suppress results from the others.

## Shell completions

```sh
# bash — add to ~/.bashrc
source <(runlatch completions bash)

# fish
runlatch completions fish > ~/.config/fish/completions/runlatch.fish

# zsh
runlatch completions zsh > "${fpath[1]}/_runlatch"
```

Completions are dynamic: `enable` and `disable` tab-complete the actual entries on
your machine, and `list --source` tab-completes available source ids.

## License

Licensed under the [MIT](../LICENSE-MIT) license.
