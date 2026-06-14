//! XDG autostart provider (user and system scope).
//!
//! Manages `*.desktop` files per the freedesktop Desktop Entry and Autostart specs.
//! The user-scoped provider works on `$XDG_CONFIG_HOME/autostart` (default
//! `~/.config/autostart`); the system-scoped one works on `/etc/xdg/autostart`
//! (the first entry of `$XDG_CONFIG_DIRS`). Disabling an entry sets `Hidden=true`
//! (the spec's "don't autostart, but keep the file" signal) rather than deleting it;
//! enabling removes the `Hidden` key.

use std::path::{Path, PathBuf};

use std::collections::HashSet;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::join_all;

use crate::desktop_file::{DESKTOP_ENTRY_GROUP, DesktopFile};
use crate::model::{AutostartEntry, Scope};
use crate::provider::AutostartProvider;

const USER_PROVIDER_ID: &str = "xdg-autostart";
const SYSTEM_PROVIDER_ID: &str = "xdg-autostart-system";

/// Provider for XDG autostart directories, either user- or system-scoped.
///
/// Construct with [`XdgAutostartProvider::user`] for `~/.config/autostart` or
/// [`XdgAutostartProvider::system`] for the system autostart directories. The two
/// differ in their reported id, scope, and which directories they search.
///
/// A provider may search more than one directory (the system scope merges every
/// `$XDG_CONFIG_DIRS/autostart`). Directories are searched in precedence order:
/// when the same entry id appears in more than one, the first wins, and edits land
/// on whichever directory the file actually lives in.
pub struct XdgAutostartProvider {
    id: &'static str,
    scope: Scope,
    autostart_dirs: Vec<PathBuf>,
}

impl XdgAutostartProvider {
    /// Build a user-scoped provider rooted at the user's autostart directory,
    /// resolved from `XDG_CONFIG_HOME` (falling back to `~/.config`).
    pub fn user() -> Self {
        Self {
            id: USER_PROVIDER_ID,
            scope: Scope::User,
            autostart_dirs: vec![default_user_autostart_dir()],
        }
    }

    /// Build a system-scoped provider that searches every `$XDG_CONFIG_DIRS/autostart`
    /// directory (falling back to `/etc/xdg/autostart`), in precedence order.
    /// Enable/disable may require elevated privileges; listing is always read-only.
    pub fn system() -> Self {
        Self {
            id: SYSTEM_PROVIDER_ID,
            scope: Scope::System,
            autostart_dirs: default_system_autostart_dirs(),
        }
    }

    /// Build a user-scoped provider rooted at the user's autostart directory.
    /// Equivalent to [`XdgAutostartProvider::user`].
    pub fn new() -> Self {
        Self::user()
    }

    /// Build a user-scoped provider rooted at a single explicit autostart directory.
    /// Primarily for tests, but also usable by callers that manage a non-standard
    /// location.
    pub fn with_autostart_dir(autostart_dir: PathBuf) -> Self {
        Self {
            id: USER_PROVIDER_ID,
            scope: Scope::User,
            autostart_dirs: vec![autostart_dir],
        }
    }

    /// The directory new entries are created in: the highest-precedence search dir.
    fn primary_dir(&self) -> &Path {
        // Constructors always populate at least one directory.
        &self.autostart_dirs[0]
    }

    /// Path a *new* `.desktop` file for `id` would be written to.
    fn entry_path(&self, id: &str) -> PathBuf {
        self.primary_dir().join(format!("{id}.desktop"))
    }

    /// Locate an existing entry's `.desktop` file across the search dirs, in
    /// precedence order.
    async fn find_entry_path(&self, id: &str) -> Option<PathBuf> {
        let file = format!("{id}.desktop");
        for dir in &self.autostart_dirs {
            let path = dir.join(&file);
            if tokio::fs::try_exists(&path).await.unwrap_or(false) {
                return Some(path);
            }
        }
        None
    }

    /// Read and parse an existing entry's desktop file, returning its path too so
    /// edits can be written back in place. Errors if no such entry exists.
    async fn read_entry_file(&self, id: &str) -> Result<(PathBuf, DesktopFile)> {
        let path = self
            .find_entry_path(id)
            .await
            .ok_or_else(|| anyhow!("no autostart entry '{id}' in {}", self.dirs_display()))?;
        let text = tokio::fs::read_to_string(&path)
            .await
            .with_context(|| format!("reading autostart entry '{id}' at {}", path.display()))?;
        Ok((path, DesktopFile::parse(&text)))
    }

    /// Comma-separated list of the search dirs, for error messages.
    fn dirs_display(&self) -> String {
        self.autostart_dirs
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Default for XdgAutostartProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AutostartProvider for XdgAutostartProvider {
    fn id(&self) -> &'static str {
        self.id
    }

    fn scope(&self) -> Scope {
        self.scope
    }

    async fn is_available(&self) -> bool {
        // Available if any search dir exists, or its parent (the config home) does
        // so we could create it. False only when none can be resolved at all (e.g.
        // no HOME and no XDG_CONFIG_HOME).
        for dir in &self.autostart_dirs {
            if tokio::fs::metadata(dir).await.is_ok() {
                return true;
            }
            if let Some(parent) = dir.parent()
                && tokio::fs::metadata(parent).await.is_ok()
            {
                return true;
            }
        }
        false
    }

    async fn entries(&self) -> Result<Vec<AutostartEntry>> {
        // Merge across every search dir; earlier dirs take precedence, so an id
        // already seen in a higher-precedence dir shadows later ones.
        let mut seen = HashSet::new();
        let mut entries = Vec::new();

        for dir in &self.autostart_dirs {
            let mut read_dir = match tokio::fs::read_dir(dir).await {
                Ok(rd) => rd,
                // A missing autostart dir simply contributes no entries.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(e)
                        .with_context(|| format!("reading autostart dir {}", dir.display()));
                }
            };

            let mut paths = Vec::new();
            while let Some(dent) = read_dir.next_entry().await? {
                let path = dent.path();
                if path.extension().and_then(|e| e.to_str()) == Some("desktop") {
                    paths.push(path);
                }
            }

            // Read every file concurrently so a large autostart dir doesn't serialize.
            let reads = paths
                .iter()
                .map(|p| async move { (p, tokio::fs::read_to_string(p).await) });
            let results = join_all(reads).await;

            for (path, read) in results {
                // Skip files we can't read or that lack a usable id, rather than
                // failing the whole listing on one bad file.
                let Ok(text) = read else { continue };
                let Some(id) = file_stem(path) else { continue };
                if !seen.insert(id.clone()) {
                    continue;
                }
                entries.push(entry_from_desktop(
                    &id,
                    &DesktopFile::parse(&text),
                    self.id,
                    self.scope,
                ));
            }
        }
        Ok(entries)
    }

    async fn enable(&self, id: &str) -> Result<()> {
        let (path, mut file) = self.read_entry_file(id).await?;
        // Per the Autostart spec, presence of Hidden=true is what disables an
        // entry; enabling just removes the key.
        file.remove(DESKTOP_ENTRY_GROUP, "Hidden");
        self.write_entry_file(id, &path, &file).await
    }

    async fn disable(&self, id: &str) -> Result<()> {
        let (path, mut file) = self.read_entry_file(id).await?;
        file.set(DESKTOP_ENTRY_GROUP, "Hidden", "true");
        self.write_entry_file(id, &path, &file).await
    }

    async fn add(&self, entry: &AutostartEntry) -> Result<()> {
        if self.find_entry_path(&entry.id).await.is_some() {
            bail!("autostart entry '{}' already exists", entry.id);
        }
        let dir = self.primary_dir();
        tokio::fs::create_dir_all(dir)
            .await
            .with_context(|| format!("creating autostart dir {}", dir.display()))?;
        let path = self.entry_path(&entry.id);

        let mut file = DesktopFile::default();
        file.set(DESKTOP_ENTRY_GROUP, "Type", "Application");
        file.set(DESKTOP_ENTRY_GROUP, "Name", &entry.display_name);
        file.set(DESKTOP_ENTRY_GROUP, "Exec", &entry.command);
        if let Some(icon) = &entry.icon {
            file.set(DESKTOP_ENTRY_GROUP, "Icon", icon);
        }
        if let Some(desc) = &entry.description {
            file.set(DESKTOP_ENTRY_GROUP, "Comment", desc);
        }
        if !entry.enabled {
            file.set(DESKTOP_ENTRY_GROUP, "Hidden", "true");
        }
        self.write_entry_file(&entry.id, &path, &file).await
    }

    async fn remove(&self, id: &str) -> Result<()> {
        let path = self
            .find_entry_path(id)
            .await
            .ok_or_else(|| anyhow!("no autostart entry '{id}' in {}", self.dirs_display()))?;
        tokio::fs::remove_file(&path).await.map_err(|e| {
            with_sudo_hint(
                e,
                &format!("removing autostart entry '{id}' at {}", path.display()),
            )
        })
    }
}

impl XdgAutostartProvider {
    /// Write `file` back to `path`, mapping permission errors to a sudo hint.
    async fn write_entry_file(&self, id: &str, path: &Path, file: &DesktopFile) -> Result<()> {
        tokio::fs::write(path, file.to_text()).await.map_err(|e| {
            with_sudo_hint(
                e,
                &format!("writing autostart entry '{id}' at {}", path.display()),
            )
        })
    }
}

/// Wrap an I/O error from a write/remove with `context`, appending a hint about
/// elevated privileges when the failure is `PermissionDenied`. System autostart
/// entries live under `/etc/xdg/autostart`, which is only writable by root.
fn with_sudo_hint(error: std::io::Error, context: &str) -> anyhow::Error {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        anyhow::Error::new(error).context(format!(
            "{context} — system autostart entries require elevated privileges (try re-running with sudo)"
        ))
    } else {
        anyhow::Error::new(error).context(context.to_string())
    }
}

/// Resolve the default user autostart directory from the XDG environment.
fn default_user_autostart_dir() -> PathBuf {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            home.join(".config")
        });
    config_home.join("autostart")
}

/// Resolve the default system autostart directories: every non-empty entry of
/// `XDG_CONFIG_DIRS` (falling back to `/etc/xdg`), each joined with `autostart`,
/// in precedence order. Always returns at least one directory.
fn default_system_autostart_dirs() -> Vec<PathBuf> {
    let raw = std::env::var_os("XDG_CONFIG_DIRS")
        .and_then(|d| d.into_string().ok())
        .unwrap_or_default();
    let mut dirs: Vec<PathBuf> = raw
        .split(':')
        .filter(|p| !p.is_empty())
        .map(|p| PathBuf::from(p).join("autostart"))
        .collect();
    if dirs.is_empty() {
        dirs.push(PathBuf::from("/etc/xdg/autostart"));
    }
    dirs
}

/// The file stem (id) of a `.desktop` path, if it has one.
fn file_stem(path: &Path) -> Option<String> {
    path.file_stem().and_then(|s| s.to_str()).map(String::from)
}

/// Build an [`AutostartEntry`] from a parsed desktop file, tagged with the
/// owning provider's `source` id and `scope`.
fn entry_from_desktop(id: &str, file: &DesktopFile, source: &str, scope: Scope) -> AutostartEntry {
    let get = |key: &str| file.get(DESKTOP_ENTRY_GROUP, key).map(str::to_string);
    let hidden = file
        .get(DESKTOP_ENTRY_GROUP, "Hidden")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    AutostartEntry {
        id: id.to_string(),
        display_name: get("Name").unwrap_or_else(|| id.to_string()),
        description: get("Comment"),
        command: get("Exec").unwrap_or_default(),
        icon: get("Icon"),
        enabled: !hidden,
        source: source.to_string(),
        scope,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A provider backed by a fresh tempdir, plus the dir guard to keep it alive.
    fn provider() -> (XdgAutostartProvider, TempDir) {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("autostart");
        std::fs::create_dir_all(&dir).unwrap();
        (XdgAutostartProvider::with_autostart_dir(dir), tmp)
    }

    /// A system-scoped provider backed by a fresh tempdir.
    fn system_provider() -> (XdgAutostartProvider, TempDir) {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("autostart");
        std::fs::create_dir_all(&dir).unwrap();
        let p = XdgAutostartProvider {
            id: SYSTEM_PROVIDER_ID,
            scope: Scope::System,
            autostart_dirs: vec![dir],
        };
        (p, tmp)
    }

    async fn write_desktop(p: &XdgAutostartProvider, id: &str, body: &str) {
        tokio::fs::write(p.entry_path(id), body).await.unwrap();
    }

    #[tokio::test]
    async fn parses_and_lists_entry() {
        let (p, _guard) = provider();
        write_desktop(
            &p,
            "example",
            "[Desktop Entry]\nType=Application\nName=Example App\nComment=Does things\nExec=example --run\nIcon=example-icon\n",
        )
        .await;

        let entries = p.entries().await.unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.id, "example");
        assert_eq!(e.display_name, "Example App");
        assert_eq!(e.description.as_deref(), Some("Does things"));
        assert_eq!(e.command, "example --run");
        assert_eq!(e.icon.as_deref(), Some("example-icon"));
        assert_eq!(e.source, "xdg-autostart");
        assert_eq!(e.scope, Scope::User);
        assert!(e.enabled);
    }

    #[tokio::test]
    async fn disable_sets_hidden_true() {
        let (p, _guard) = provider();
        write_desktop(
            &p,
            "example",
            "[Desktop Entry]\nName=Example\nExec=example\n",
        )
        .await;

        p.disable("example").await.unwrap();

        let text = tokio::fs::read_to_string(p.entry_path("example"))
            .await
            .unwrap();
        assert!(text.contains("Hidden=true"), "file was: {text}");
        // The other keys survive.
        assert!(text.contains("Name=Example"));

        let entries = p.entries().await.unwrap();
        assert!(!entries[0].enabled);
    }

    #[tokio::test]
    async fn enable_removes_hidden_key() {
        let (p, _guard) = provider();
        write_desktop(
            &p,
            "example",
            "[Desktop Entry]\nName=Example\nExec=example\nHidden=true\n",
        )
        .await;

        // Sanity: it starts disabled.
        assert!(!p.entries().await.unwrap()[0].enabled);

        p.enable("example").await.unwrap();

        let text = tokio::fs::read_to_string(p.entry_path("example"))
            .await
            .unwrap();
        assert!(
            !text.contains("Hidden"),
            "Hidden key should be gone: {text}"
        );
        assert!(p.entries().await.unwrap()[0].enabled);
    }

    #[tokio::test]
    async fn add_then_remove_round_trip() {
        let (p, _guard) = provider();
        let entry = AutostartEntry {
            id: "newthing".into(),
            display_name: "New Thing".into(),
            description: None,
            command: "newthing --start".into(),
            icon: None,
            enabled: true,
            source: "xdg-autostart".into(),
            scope: Scope::User,
        };

        p.add(&entry).await.unwrap();
        assert!(p.entry_path("newthing").exists());
        let listed = p.entries().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].command, "newthing --start");

        // Adding again is an error.
        assert!(p.add(&entry).await.is_err());

        p.remove("newthing").await.unwrap();
        assert!(!p.entry_path("newthing").exists());
        assert!(p.entries().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_dir_lists_empty() {
        let tmp = TempDir::new().unwrap();
        // Point at a dir that doesn't exist.
        let p = XdgAutostartProvider::with_autostart_dir(tmp.path().join("nope"));
        assert!(p.entries().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn system_provider_tags_source_and_scope() {
        let (p, _guard) = system_provider();
        assert_eq!(p.id(), "xdg-autostart-system");
        assert_eq!(p.scope(), Scope::System);

        write_desktop(
            &p,
            "example",
            "[Desktop Entry]\nName=Example\nExec=example\n",
        )
        .await;

        let entries = p.entries().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, "xdg-autostart-system");
        assert_eq!(entries[0].scope, Scope::System);
    }

    #[tokio::test]
    async fn merges_multiple_dirs_with_first_wins_precedence() {
        let tmp = TempDir::new().unwrap();
        let high = tmp.path().join("high");
        let low = tmp.path().join("low");
        std::fs::create_dir_all(&high).unwrap();
        std::fs::create_dir_all(&low).unwrap();

        // `shared` exists in both dirs; `low_only` exists only in the lower one.
        std::fs::write(
            high.join("shared.desktop"),
            "[Desktop Entry]\nName=High\nExec=high\n",
        )
        .unwrap();
        std::fs::write(
            low.join("shared.desktop"),
            "[Desktop Entry]\nName=Low\nExec=low\n",
        )
        .unwrap();
        std::fs::write(
            low.join("low_only.desktop"),
            "[Desktop Entry]\nName=LowOnly\nExec=lowonly\n",
        )
        .unwrap();

        // `high` is listed first, so it takes precedence for `shared`.
        let p = XdgAutostartProvider {
            id: SYSTEM_PROVIDER_ID,
            scope: Scope::System,
            autostart_dirs: vec![high, low],
        };

        let entries = p.entries().await.unwrap();
        assert_eq!(entries.len(), 2);
        let shared = entries.iter().find(|e| e.id == "shared").unwrap();
        assert_eq!(shared.display_name, "High");
        assert!(entries.iter().any(|e| e.id == "low_only"));
    }

    #[tokio::test]
    async fn disable_writes_to_dir_where_entry_lives() {
        let tmp = TempDir::new().unwrap();
        let high = tmp.path().join("high");
        let low = tmp.path().join("low");
        std::fs::create_dir_all(&high).unwrap();
        std::fs::create_dir_all(&low).unwrap();
        std::fs::write(
            low.join("only_low.desktop"),
            "[Desktop Entry]\nName=OnlyLow\nExec=onlylow\n",
        )
        .unwrap();

        let p = XdgAutostartProvider {
            id: SYSTEM_PROVIDER_ID,
            scope: Scope::System,
            autostart_dirs: vec![high.clone(), low.clone()],
        };

        p.disable("only_low").await.unwrap();
        // The edit landed on the lower dir (where the file is), not the primary.
        let text = std::fs::read_to_string(low.join("only_low.desktop")).unwrap();
        assert!(text.contains("Hidden=true"), "file was: {text}");
        assert!(!high.join("only_low.desktop").exists());
    }

    #[test]
    fn sudo_hint_only_on_permission_denied() {
        use std::io::{Error, ErrorKind};

        let denied = with_sudo_hint(Error::from(ErrorKind::PermissionDenied), "writing foo");
        let msg = format!("{denied:#}");
        assert!(msg.contains("writing foo"), "got: {msg}");
        assert!(msg.contains("sudo"), "got: {msg}");

        let not_found = with_sudo_hint(Error::from(ErrorKind::NotFound), "writing foo");
        let msg = format!("{not_found:#}");
        assert!(msg.contains("writing foo"), "got: {msg}");
        assert!(!msg.contains("sudo"), "got: {msg}");
    }
}
