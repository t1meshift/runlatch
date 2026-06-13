//! systemd provider for both user (session bus) and system (system bus) units.
//!
//! A single [`SystemdProvider`] struct handles both scopes — the only differences
//! are which D-Bus connection to open and which id/scope to report. Use
//! [`SystemdProvider::user()`] and [`SystemdProvider::system()`] to construct.
//!
//! Unit file metadata (Description, ExecStart / timer schedule) is read directly
//! from disk using the paths returned by `ListUnitFiles`.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use futures::future::join_all;
use zbus::{Connection, proxy, zvariant::OwnedObjectPath};

use crate::desktop_file::DesktopFile;
use crate::model::{AutostartEntry, Scope};
use crate::provider::AutostartProvider;

/// Unit-file states with an `[Install]` section that can actually be
/// enabled/disabled. Everything else (`static`, `alias`, `generated`, `transient`,
/// `bad`) is excluded from the autostart view.
const ENABLEABLE_STATES: &[&str] = &[
    "enabled",
    "enabled-runtime",
    "disabled",
    "linked",
    "linked-runtime",
    "masked",
    "masked-runtime",
    "indirect",
];

/// Which D-Bus bus — and therefore which systemd instance — the provider talks to.
#[derive(Debug, Clone, Copy)]
enum Bus {
    /// `org.freedesktop.systemd1` on the **session** bus — user units.
    Session,
    /// `org.freedesktop.systemd1` on the **system** bus — system units.
    System,
}

/// A systemd "unit file change": `(change_type, file_path, source)`.
type UnitFileChange = (String, String, String);

#[proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait Manager {
    fn list_unit_files(&self) -> zbus::Result<Vec<(String, String)>>;
    fn enable_unit_files(
        &self,
        files: &[&str],
        runtime: bool,
        force: bool,
    ) -> zbus::Result<(bool, Vec<UnitFileChange>)>;
    fn disable_unit_files(
        &self,
        files: &[&str],
        runtime: bool,
    ) -> zbus::Result<Vec<UnitFileChange>>;
    fn start_unit(&self, name: &str, mode: &str) -> zbus::Result<OwnedObjectPath>;
}

/// Provider for systemd units, covering either the user or system scope.
pub struct SystemdProvider {
    bus: Bus,
}

impl SystemdProvider {
    /// Provider for `--user` units (session bus).
    pub fn user() -> Self {
        Self { bus: Bus::Session }
    }

    /// Provider for system units (system bus). Enable/disable may require
    /// polkit elevation; listing is always read-only.
    pub fn system() -> Self {
        Self { bus: Bus::System }
    }

    async fn connect(&self) -> Result<Connection> {
        match self.bus {
            Bus::Session => Connection::session()
                .await
                .context("connecting to the session bus"),
            Bus::System => Connection::system()
                .await
                .context("connecting to the system bus"),
        }
    }

    async fn manager(&self) -> Result<ManagerProxy<'static>> {
        let conn = self.connect().await?;
        ManagerProxy::new(&conn)
            .await
            .context("building systemd1 manager proxy")
    }
}

#[async_trait]
impl AutostartProvider for SystemdProvider {
    fn id(&self) -> &'static str {
        match self.bus {
            Bus::Session => "systemd-user",
            Bus::System => "systemd-system",
        }
    }

    fn scope(&self) -> Scope {
        match self.bus {
            Bus::Session => Scope::User,
            Bus::System => Scope::System,
        }
    }

    async fn is_available(&self) -> bool {
        self.manager().await.is_ok()
    }

    async fn entries(&self) -> Result<Vec<AutostartEntry>> {
        let manager = self.manager().await?;
        let unit_files = manager
            .list_unit_files()
            .await
            .context("listing systemd unit files")?;

        let enableable: Vec<(String, String)> = unit_files
            .into_iter()
            .filter(|(_, state)| ENABLEABLE_STATES.contains(&state.as_str()))
            .collect();

        let reads = enableable
            .iter()
            .map(|(path, _)| async move { tokio::fs::read_to_string(path).await.ok() });
        let file_texts = join_all(reads).await;

        let source = self.id().to_string();
        let scope = self.scope();
        let entries = enableable
            .iter()
            .zip(file_texts)
            .map(|((path, state), text)| {
                let name = unit_name(path);
                let enabled = matches!(state.as_str(), "enabled" | "enabled-runtime");
                let (description, command) = text
                    .as_deref()
                    .map(unit_file_meta)
                    .unwrap_or_default();
                AutostartEntry {
                    id: name.clone(),
                    display_name: description.clone().unwrap_or_else(|| name.clone()),
                    description,
                    command: command.unwrap_or_default(),
                    icon: None,
                    enabled,
                    source: source.clone(),
                    scope,
                }
            })
            .collect();

        Ok(entries)
    }

    async fn enable(&self, id: &str) -> Result<()> {
        self.manager()
            .await?
            .enable_unit_files(&[id], false, true)
            .await
            .with_context(|| format!("enabling unit '{id}'"))?;
        Ok(())
    }

    async fn disable(&self, id: &str) -> Result<()> {
        self.manager()
            .await?
            .disable_unit_files(&[id], false)
            .await
            .with_context(|| format!("disabling unit '{id}'"))?;
        Ok(())
    }

    async fn add(&self, _entry: &AutostartEntry) -> Result<()> {
        bail!("adding units is not supported for {} in this pass", self.id());
    }

    async fn remove(&self, _id: &str) -> Result<()> {
        bail!("removing units is not supported for {} in this pass", self.id());
    }
}

fn unit_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// Extract `Description` from `[Unit]` and the primary command from `[Service]`
/// or schedule from `[Timer]`.
fn unit_file_meta(text: &str) -> (Option<String>, Option<String>) {
    let file = DesktopFile::parse(text);
    let description = file
        .get("Unit", "Description")
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    // Services: ExecStart.
    // Timers: no ExecStart — show the schedule (OnCalendar is most readable).
    // Sockets: show the listen address so the row isn't blank.
    let command = file
        .get("Service", "ExecStart")
        .or_else(|| file.get("Timer", "OnCalendar"))
        .or_else(|| file.get("Timer", "OnBootSec"))
        .or_else(|| file.get("Timer", "OnStartupSec"))
        .or_else(|| file.get("Timer", "OnUnitActiveSec"))
        .or_else(|| file.get("Timer", "OnActiveSec"))
        .or_else(|| file.get("Socket", "ListenStream"))
        .or_else(|| file.get("Socket", "ListenDatagram"))
        .or_else(|| file.get("Socket", "ListenSequentialPacket"))
        .or_else(|| file.get("Socket", "ListenFIFO"))
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    (description, command)
}
