//! Panel state, kept free of any egui dependency so it can be tested without a
//! window. The egui layer in `app` only renders this and forwards input.

use std::path::PathBuf;
use std::time::Duration;

use crate::addr;
use crate::config;
use crate::docs;
use crate::server::{self, RunningServer};
use crate::settings::Settings;

pub mod app;

/// What the user asked for, independent of whether it worked.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Start,
    Stop,
}

/// Server status plus the numbers the panel displays.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub url: String,
    pub listening: String,
    pub docs_version: Option<String>,
    pub docs_pages: usize,
    pub docs_path: PathBuf,
    pub reach: Reach,
    pub uptime: Duration,
    pub requests: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Bound to every interface, so the network can reach it.
    Network,
    /// Bound to loopback only.
    ThisMachineOnly,
    /// Bound to one specific address.
    Single,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PanelState {
    Idle,
    Running(Status),
    Stopped { reason: Option<String> },
    Failed(String),
}

impl PanelState {
    pub fn is_running(&self) -> bool {
        matches!(self, PanelState::Running(_))
    }
}

/// Validate the fields a user can edit, returning messages for the panel.
///
/// Checking here keeps bad input from ever reaching a bind attempt, where the
/// OS error would be less specific.
pub fn validate(settings: &Settings) -> Result<(), String> {
    if settings.bind.trim().is_empty() {
        return Err("Bind address is required".to_string());
    }
    if settings.bind.parse::<std::net::IpAddr>().is_err() {
        return Err(format!(
            "\"{}\" is not an IP address. Use 0.0.0.0 for all interfaces.",
            settings.bind
        ));
    }
    // Any u16 port is valid: 0 means "pick a free port".
    Ok(())
}

/// Addresses offered in the panel's dropdown, wildcard first.
pub fn bind_choices() -> Vec<String> {
    addr::bind_candidates()
}

/// Best `GOOSE_DOCS_ROOT` value to show for a running server.
///
/// The advertised URL already substitutes a dialable address for the wildcard,
/// so this is what a user should paste on the client machine.
pub fn goose_docs_root(status: &Status) -> String {
    status.url.clone()
}

/// The full status line a client should use.
pub fn client_hint(status: &Status) -> String {
    format!("GOOSE_DOCS_ROOT={}", goose_docs_root(status))
}

/// Owns the running server and turns commands into state transitions.
pub struct Panel {
    pub settings: Settings,
    pub state: PanelState,
    pub(crate) settings_path: PathBuf,
    cache_root: PathBuf,
    server: Option<RunningServer>,
}

impl Panel {
    pub fn new(settings: Settings, settings_path: PathBuf, cache_root: PathBuf) -> Self {
        Self {
            settings,
            state: PanelState::Idle,
            settings_path,
            cache_root,
            server: None,
        }
    }

    pub fn cache_root(&self) -> &PathBuf {
        &self.cache_root
    }

    /// Resolve the docs root the current settings point at.
    pub fn docs_root(&self) -> anyhow::Result<docs::DocsRoot> {
        resolve_docs(&self.settings, &self.cache_root)
    }

    pub fn apply(&mut self, command: Command) {
        match command {
            Command::Start => self.start(),
            Command::Stop => self.stop(),
        }
    }

    fn start(&mut self) {
        if self.server.is_some() {
            return;
        }
        if let Err(message) = validate(&self.settings) {
            self.state = PanelState::Failed(message);
            return;
        }

        let root = match self.docs_root() {
            Ok(root) => root,
            Err(error) => {
                self.state = PanelState::Failed(error.to_string());
                return;
            }
        };

        let runtime = match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle,
            Err(_) => {
                self.state = PanelState::Failed(
                    "no async runtime available to start the server".to_string(),
                );
                return;
            }
        };

        let started = runtime.block_on(server::start(
            &root,
            &self.settings.bind,
            self.settings.port,
        ));

        match started {
            Ok(running) => {
                self.state = PanelState::Running(status_of(&running));
                self.server = Some(running);
                // Persist only after a successful start, so a typo does not
                // become the next launch's default.
                let _ = self.settings.save(&self.settings_path);
            }
            Err(error) => self.state = PanelState::Failed(error.to_string()),
        }
    }

    fn stop(&mut self) {
        if let Some(mut running) = self.server.take() {
            running.stop();
        }
        self.state = PanelState::Stopped { reason: None };
    }

    /// Refresh the numbers the panel shows while a server runs.
    pub fn tick(&mut self) {
        if let Some(running) = &self.server {
            if running.is_running() {
                self.state = PanelState::Running(status_of(running));
            }
        }
    }

    /// Stop the server if one is running, for example on window close.
    pub fn shutdown(&mut self) {
        if self.server.is_some() {
            self.stop();
        }
    }
}

fn status_of(running: &RunningServer) -> Status {
    Status {
        url: running.url.clone(),
        listening: running.addr.to_string(),
        docs_version: running.docs_version.clone(),
        docs_pages: running.docs_entries,
        docs_path: running.docs_path.clone(),
        reach: if addr::is_any(&running.addr.ip()) {
            Reach::Network
        } else if running.addr.ip().is_loopback() {
            Reach::ThisMachineOnly
        } else {
            Reach::Single
        },
        uptime: running.uptime(),
        requests: running.request_count(),
    }
}

fn resolve_docs(
    settings: &Settings,
    cache_root: &std::path::Path,
) -> anyhow::Result<docs::DocsRoot> {
    match (&settings.docs_dir, &settings.docs_version) {
        (Some(dir), _) => docs::validate(dir),
        (None, Some(version)) => docs::resolve_version(cache_root, version),
        (None, None) => docs::resolve_cached(cache_root),
    }
}

/// Port used before any settings exist.
pub fn default_port() -> u16 {
    config::DEFAULT_PORT
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings::default()
    }

    #[test]
    fn default_settings_bind_all_interfaces() {
        let status_settings = settings();
        assert_eq!(status_settings.bind, "0.0.0.0");
        assert!(validate(&status_settings).is_ok());
    }

    #[test]
    fn loopback_is_allowed_but_flagged_as_local() {
        let mut s = settings();
        s.bind = "127.0.0.1".to_string();
        assert!(validate(&s).is_ok());
    }

    #[test]
    fn hostnames_are_rejected_with_a_helpful_message() {
        let mut s = settings();
        s.bind = "localhost".to_string();
        let error = validate(&s).unwrap_err();
        assert!(error.contains("not an IP address"), "got: {error}");
        assert!(error.contains("0.0.0.0"), "should suggest the wildcard");
    }

    #[test]
    fn empty_bind_is_rejected() {
        let mut s = settings();
        s.bind = "   ".to_string();
        assert!(validate(&s).is_err());
    }

    #[test]
    fn port_zero_is_accepted() {
        let mut s = settings();
        s.port = 0;
        assert!(validate(&s).is_ok());
    }

    #[test]
    fn bind_choices_put_the_wildcard_first() {
        let choices = bind_choices();
        assert_eq!(choices.first().map(String::as_str), Some("0.0.0.0"));
    }

    #[test]
    fn client_hint_is_a_pasteable_env_var() {
        let status = Status {
            url: "http://192.168.1.5:10650".to_string(),
            listening: "0.0.0.0:10650".to_string(),
            docs_version: Some("1.52.0".to_string()),
            docs_pages: 61,
            docs_path: PathBuf::from("/opt/goose-docs"),
            reach: Reach::Network,
            uptime: Duration::from_secs(5),
            requests: 3,
        };
        assert_eq!(
            client_hint(&status),
            "GOOSE_DOCS_ROOT=http://192.168.1.5:10650"
        );
    }

    fn fixture_root() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/docs-root")
    }

    fn temp_settings_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("goose-doc-panel-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.yaml")
    }

    fn panel_with_fixture(name: &str) -> Panel {
        let mut s = settings();
        s.docs_dir = Some(fixture_root());
        s.port = 0;
        Panel::new(s, temp_settings_path(name), std::env::temp_dir())
    }

    #[test]
    fn panel_starts_idle() {
        let panel = panel_with_fixture("idle");
        assert_eq!(panel.state, PanelState::Idle);
        assert!(!panel.state.is_running());
    }

    #[test]
    fn start_then_stop_moves_through_states() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _guard = runtime.enter();

        let mut panel = panel_with_fixture("startstop");
        panel.apply(Command::Start);

        match &panel.state {
            PanelState::Running(status) => {
                assert!(status.url.starts_with("http://"));
                assert!(!status.url.contains("0.0.0.0"));
                assert_eq!(status.docs_pages, 2);
                assert_eq!(status.reach, Reach::Network);
            }
            other => panic!("expected running, got {other:?}"),
        }

        panel.apply(Command::Stop);
        assert!(matches!(panel.state, PanelState::Stopped { .. }));
        assert!(!panel.state.is_running());
    }

    #[test]
    fn start_persists_settings() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _guard = runtime.enter();

        let mut panel = panel_with_fixture("persist");
        let path = panel.settings_path.clone();
        panel.apply(Command::Start);

        assert!(path.is_file(), "settings should be written after a start");
        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded.docs_dir, Some(fixture_root()));
        panel.apply(Command::Stop);
    }

    #[test]
    fn invalid_bind_fails_without_starting() {
        let mut panel = panel_with_fixture("badbind");
        panel.settings.bind = "localhost".to_string();
        panel.apply(Command::Start);

        match &panel.state {
            PanelState::Failed(message) => assert!(message.contains("not an IP address")),
            other => panic!("expected failed, got {other:?}"),
        }
    }

    #[test]
    fn missing_docs_root_fails_with_guidance() {
        let mut s = settings();
        s.docs_dir = Some(PathBuf::from("/nonexistent/goose-docs"));
        let mut panel = Panel::new(s, temp_settings_path("nodocs"), std::env::temp_dir());

        panel.apply(Command::Start);
        match &panel.state {
            PanelState::Failed(message) => {
                assert!(message.contains("not a directory") || message.contains("missing"))
            }
            other => panic!("expected failed, got {other:?}"),
        }
    }

    #[test]
    fn start_is_idempotent() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _guard = runtime.enter();

        let mut panel = panel_with_fixture("idempotent");
        panel.apply(Command::Start);
        let first = panel.state.clone();
        panel.apply(Command::Start);
        assert_eq!(panel.state, first, "a second start must not rebind");

        panel.apply(Command::Stop);
    }

    #[test]
    fn shutdown_stops_a_running_server() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _guard = runtime.enter();

        let mut panel = panel_with_fixture("shutdown");
        panel.apply(Command::Start);
        assert!(panel.state.is_running());

        panel.shutdown();
        assert!(!panel.state.is_running());
    }

    #[test]
    fn tick_updates_request_count() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _guard = runtime.enter();

        let mut panel = panel_with_fixture("tick");
        panel.apply(Command::Start);

        let url = match &panel.state {
            PanelState::Running(status) => status.url.clone(),
            other => panic!("expected running, got {other:?}"),
        };

        runtime.block_on(async {
            let _ = reqwest::get(format!("{url}/goose-docs-map.md")).await;
        });

        panel.tick();
        match &panel.state {
            PanelState::Running(status) => assert_eq!(status.requests, 1),
            other => panic!("expected running, got {other:?}"),
        }

        panel.apply(Command::Stop);
    }
}
