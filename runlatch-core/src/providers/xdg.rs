//! XDG autostart provider (user scope).
//!
//! Manages `~/.config/autostart/*.desktop` per the freedesktop Desktop Entry and
//! Autostart specs. Disabling an entry sets `Hidden=true` (the spec's "don't
//! autostart, but keep the file" signal) rather than deleting it; enabling removes
//! the `Hidden` key. The autostart directory honors `XDG_CONFIG_HOME`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use futures::future::join_all;

use crate::desktop_file::{DESKTOP_ENTRY_GROUP, DesktopFile};
use crate::model::{AutostartEntry, Scope};
use crate::provider::AutostartProvider;

const PROVIDER_ID: &str = "xdg-autostart";

/// Provider for `$XDG_CONFIG_HOME/autostart` (default `~/.config/autostart`).
pub struct XdgAutostartProvider {
    autostart_dir: PathBuf,
}

impl XdgAutostartProvider {
    /// Build a provider rooted at the user's autostart directory, resolved from
    /// `XDG_CONFIG_HOME` (falling back to `~/.config`).
    pub fn new() -> Self {
        Self::with_autostart_dir(default_autostart_dir())
    }

    /// Build a provider rooted at an explicit autostart directory. Primarily for
    /// tests, but also usable by callers that manage a non-standard location.
    pub fn with_autostart_dir(autostart_dir: PathBuf) -> Self {
        Self { autostart_dir }
    }

    /// Path to the `.desktop` file backing `id`.
    fn entry_path(&self, id: &str) -> PathBuf {
        self.autostart_dir.join(format!("{id}.desktop"))
    }

    /// Read and parse a single entry's desktop file, erroring if it doesn't exist.
    async fn read_entry_file(&self, id: &str) -> Result<DesktopFile> {
        let path = self.entry_path(id);
        let text = tokio::fs::read_to_string(&path)
            .await
            .with_context(|| format!("no autostart entry '{id}' at {}", path.display()))?;
        Ok(DesktopFile::parse(&text))
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
        PROVIDER_ID
    }

    fn scope(&self) -> Scope {
        Scope::User
    }

    async fn is_available(&self) -> bool {
        // Available if the autostart dir exists, or its parent (the config home)
        // does so we could create it. False only when we can't resolve a config
        // home at all (e.g. no HOME and no XDG_CONFIG_HOME).
        if tokio::fs::metadata(&self.autostart_dir).await.is_ok() {
            return true;
        }
        match self.autostart_dir.parent() {
            Some(parent) => tokio::fs::metadata(parent).await.is_ok(),
            None => false,
        }
    }

    async fn entries(&self) -> Result<Vec<AutostartEntry>> {
        let mut read_dir = match tokio::fs::read_dir(&self.autostart_dir).await {
            Ok(rd) => rd,
            // A missing autostart dir simply means no entries yet.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(e).with_context(|| {
                    format!("reading autostart dir {}", self.autostart_dir.display())
                });
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

        let mut entries = Vec::new();
        for (path, read) in results {
            // Skip files we can't read or that lack a usable id, rather than
            // failing the whole listing on one bad file.
            let Ok(text) = read else { continue };
            let Some(id) = file_stem(path) else { continue };
            entries.push(entry_from_desktop(&id, &DesktopFile::parse(&text)));
        }
        Ok(entries)
    }

    async fn enable(&self, id: &str) -> Result<()> {
        let mut file = self.read_entry_file(id).await?;
        // Per the Autostart spec, presence of Hidden=true is what disables an
        // entry; enabling just removes the key.
        file.remove(DESKTOP_ENTRY_GROUP, "Hidden");
        self.write_entry_file(id, &file).await
    }

    async fn disable(&self, id: &str) -> Result<()> {
        let mut file = self.read_entry_file(id).await?;
        file.set(DESKTOP_ENTRY_GROUP, "Hidden", "true");
        self.write_entry_file(id, &file).await
    }

    async fn add(&self, entry: &AutostartEntry) -> Result<()> {
        let path = self.entry_path(&entry.id);
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            bail!("autostart entry '{}' already exists", entry.id);
        }
        tokio::fs::create_dir_all(&self.autostart_dir)
            .await
            .with_context(|| format!("creating autostart dir {}", self.autostart_dir.display()))?;

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
        self.write_entry_file(&entry.id, &file).await
    }

    async fn remove(&self, id: &str) -> Result<()> {
        let path = self.entry_path(id);
        tokio::fs::remove_file(&path)
            .await
            .with_context(|| format!("removing autostart entry '{id}' at {}", path.display()))
    }
}

impl XdgAutostartProvider {
    async fn write_entry_file(&self, id: &str, file: &DesktopFile) -> Result<()> {
        let path = self.entry_path(id);
        tokio::fs::write(&path, file.to_text())
            .await
            .with_context(|| format!("writing autostart entry '{id}' at {}", path.display()))
    }
}

/// Resolve the default autostart directory from the XDG environment.
fn default_autostart_dir() -> PathBuf {
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

/// The file stem (id) of a `.desktop` path, if it has one.
fn file_stem(path: &Path) -> Option<String> {
    path.file_stem().and_then(|s| s.to_str()).map(String::from)
}

/// Build an [`AutostartEntry`] from a parsed desktop file.
fn entry_from_desktop(id: &str, file: &DesktopFile) -> AutostartEntry {
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
        source: PROVIDER_ID.to_string(),
        scope: Scope::User,
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
}
