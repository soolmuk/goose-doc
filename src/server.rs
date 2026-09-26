use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::{info, warn};

use crate::addr;
use crate::docs::DocsRoot;

/// Live server handle. Dropping the shutdown sender stops the server.
pub struct RunningServer {
    pub addr: SocketAddr,
    /// URL clients should use, resolved away from the wildcard address.
    pub url: String,
    pub started_at: Instant,
    pub docs_version: Option<String>,
    pub docs_entries: usize,
    /// Docs root on disk, when one is in use. `None` means the embedded copy.
    pub docs_path: Option<PathBuf>,
    pub requests: Arc<AtomicU64>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl RunningServer {
    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }

    pub fn request_count(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    /// True when listening on every interface.
    pub fn serves_network(&self) -> bool {
        addr::is_any(&self.addr.ip())
    }

    /// Signal the accept loop to finish its current connections and stop.
    pub fn stop(&mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
    }

    pub fn is_running(&self) -> bool {
        self.shutdown.is_some()
    }
}

#[derive(Clone)]
struct AppState {
    /// Docs root on disk, when one is available. Preferred over the embedded
    /// copy because it also carries the assets the HTML site needs.
    root: Option<PathBuf>,
    requests: Arc<AtomicU64>,
}

/// Parse a bind address, rejecting hostnames so failures are immediate and
/// unambiguous.
pub fn parse_bind(bind: &str) -> Result<IpAddr> {
    bind.parse()
        .with_context(|| format!("\"{bind}\" is not an IP address"))
}

/// Bind and start serving. Returns once the listener is accepting.
pub async fn start(docs: &DocsRoot, bind: &str, port: u16) -> Result<RunningServer> {
    let ip = parse_bind(bind)?;

    let requests = Arc::new(AtomicU64::new(0));
    let state = AppState {
        root: docs.path_on_disk(),
        requests: Arc::clone(&requests),
    };

    let router = Router::new()
        .route("/healthz", get(healthz))
        .fallback(get(serve_file))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ))
        .with_state(state);

    let addr = SocketAddr::new(ip, port);
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    let local = listener
        .local_addr()
        .context("failed to read the bound address")?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let serve = axum::serve(listener, router).with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
        info!("shutdown requested");
    });

    tokio::spawn(async move {
        if let Err(error) = serve.await {
            warn!("server stopped: {error}");
        }
    });

    let url = client_url(&local);

    info!(
        "serving {} documentation at {url} on {} ({} pages)",
        docs.version.as_deref().unwrap_or("goose"),
        addr::describe(&local.ip()),
        docs.entries
    );

    Ok(RunningServer {
        addr: local,
        url,
        started_at: Instant::now(),
        docs_version: docs.version.clone(),
        docs_entries: docs.entries,
        docs_path: docs.path_on_disk(),
        requests,
        shutdown: Some(shutdown_tx),
    })
}

/// Build the URL a client should use.
///
/// A wildcard bind is not dialable, so substitute a concrete address: the
/// machine's LAN address when it has one, otherwise loopback. This is also the
/// value printed for `GOOSE_DOCS_ROOT`, so it must be usable as-is.
pub fn client_url(local: &SocketAddr) -> String {
    let port = local.port();
    let ip = match local.ip() {
        ip if addr::is_any(&ip) => {
            addr::preferred_display_ip().unwrap_or(IpAddr::from([127, 0, 0, 1]))
        }
        ip => ip,
    };
    format_host(ip, port)
}

fn format_host(ip: IpAddr, port: u16) -> String {
    match ip {
        IpAddr::V4(v4) => format!("http://{v4}:{port}"),
        IpAddr::V6(v6) => format!("http://[{v6}]:{port}"),
    }
}

async fn healthz() -> &'static str {
    "ok"
}

async fn serve_file(State(state): State<AppState>, uri: Uri) -> Response {
    state.requests.fetch_add(1, Ordering::Relaxed);

    let decoded = match percent_decode(uri.path()) {
        Some(value) => value,
        None => return (StatusCode::BAD_REQUEST, "invalid percent encoding").into_response(),
    };

    let Some(relative) = safe_relative(&decoded) else {
        return (StatusCode::FORBIDDEN, "path escapes the docs root").into_response();
    };

    // Prefer the on-disk root: it is a superset of the embedded pages.
    if let Some(root) = &state.root {
        let mut target = root.join(&relative);
        if target.is_dir() {
            target = target.join("index.html");
        }
        match tokio::fs::read(&target).await {
            Ok(bytes) => {
                return (
                    [(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static(content_type_for(&target)),
                    )],
                    bytes,
                )
                    .into_response()
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                warn!("failed to read {}: {error}", target.display());
                return (StatusCode::INTERNAL_SERVER_ERROR, "failed to read file").into_response();
            }
        }
    }

    match crate::embedded::get(&relative) {
        Some(bytes) => (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static(content_type_for(&relative)),
            )],
            bytes,
        )
            .into_response(),
        None => not_found(&relative),
    }
}

/// Map a URL path to a path relative to the docs root, rejecting traversal.
fn safe_relative(path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in Path::new(path.trim_start_matches('/')).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            // Absolute components and `..` mean the request is not rooted here.
            _ => return None,
        }
    }
    Some(out)
}

/// Decode percent escapes. Returns `None` on malformed input.
fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }

    String::from_utf8(out).ok()
}

/// Markdown is served as plain text so the skill reads the raw source rather
/// than a rendered page.
fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("md") => "text/plain; charset=utf-8",
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("json") => "application/json",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("txt") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("pdf") => "application/pdf",
        // Unknown extensions are only useful as text; serving them as plain
        // text keeps stray binary from being interpreted by the browser.
        _ => "text/plain; charset=utf-8",
    }
}

/// 404 with a short body. Deliberately no SPA fallback: a missing doc path must
/// stay a 404 so the skill sees the failure instead of a rendered shell.
fn not_found(relative: &Path) -> Response {
    let body = format!("not found: /{}\n", relative.display());
    (StatusCode::NOT_FOUND, body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs;

    #[test]
    fn parse_bind_accepts_addresses_and_rejects_hostnames() {
        assert!(parse_bind("0.0.0.0").is_ok());
        assert!(parse_bind("::").is_ok());
        assert!(parse_bind("127.0.0.1").is_ok());
        let error = parse_bind("localhost").unwrap_err().to_string();
        assert!(error.contains("not an IP address"), "got: {error}");
    }

    #[test]
    fn wildcard_bind_resolves_to_a_dialable_url() {
        let local: SocketAddr = "0.0.0.0:10650".parse().unwrap();
        let url = client_url(&local);
        assert!(!url.contains("0.0.0.0"), "wildcard leaked into url: {url}");
        assert!(url.contains(":10650"));
        assert!(url.starts_with("http://"));
    }

    #[test]
    fn explicit_bind_is_kept_in_the_url() {
        let local: SocketAddr = "192.168.1.5:10650".parse().unwrap();
        assert_eq!(client_url(&local), "http://192.168.1.5:10650");
    }

    #[test]
    fn loopback_bind_stays_loopback() {
        let local: SocketAddr = "127.0.0.1:10650".parse().unwrap();
        assert_eq!(client_url(&local), "http://127.0.0.1:10650");
    }

    #[test]
    fn ipv6_urls_are_bracketed() {
        let local: SocketAddr = "[::1]:10650".parse().unwrap();
        assert_eq!(client_url(&local), "http://[::1]:10650");
    }

    #[test]
    fn safe_relative_rejects_traversal() {
        assert!(safe_relative("/../../etc/passwd").is_none());
        assert!(safe_relative("/docs/../../etc/passwd").is_none());
    }

    #[test]
    fn safe_relative_strips_leading_slash_and_dot() {
        assert_eq!(
            safe_relative("/docs/guides/a.md").unwrap(),
            PathBuf::from("docs/guides/a.md")
        );
        assert_eq!(
            safe_relative("/./docs/a.md").unwrap(),
            PathBuf::from("docs/a.md")
        );
    }

    #[test]
    fn percent_decode_handles_spaces_and_malformed_input() {
        assert_eq!(percent_decode("/a%20b.md").unwrap(), "/a b.md");
        assert!(percent_decode("/a%2").is_none());
        assert!(percent_decode("/a%zz").is_none());
    }

    #[test]
    fn content_type_marks_markdown_as_plain_text() {
        assert_eq!(
            content_type_for(Path::new("x/goose-docs-map.md")),
            "text/plain; charset=utf-8"
        );
        assert_eq!(
            content_type_for(Path::new("x/index.html")),
            "text/html; charset=utf-8"
        );
    }

    fn fixture_root() -> DocsRoot {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/docs-root");
        docs::validate(&path).expect("fixture root")
    }

    async fn spawn_fixture() -> RunningServer {
        start(&fixture_root(), "127.0.0.1", 0).await.expect("start")
    }

    async fn body_of(server: &RunningServer, path: &str) -> (u16, String) {
        let url = format!("{}{}", server.url, path);
        let response = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .expect("request");
        let status = response.status().as_u16();
        let text = response.text().await.expect("body");
        (status, text)
    }

    #[tokio::test]
    async fn serves_the_docs_map_at_the_root() {
        let mut server = spawn_fixture().await;
        let (status, body) = body_of(&server, "/goose-docs-map.md").await;
        assert_eq!(status, 200);
        assert!(body.contains("goose Documentation Map"));

        let (status, body) = body_of(&server, "/docs/guides/offline-docs.md").await;
        assert_eq!(status, 200);
        assert!(body.contains("docs root"));

        server.stop();
    }

    #[tokio::test]
    async fn serves_every_map_entry() {
        let mut server = spawn_fixture().await;
        let (_, map) = body_of(&server, "/goose-docs-map.md").await;
        let entries = docs::map_entries(&map);
        assert!(!entries.is_empty());

        for entry in entries {
            let (status, _) = body_of(&server, &format!("/{entry}")).await;
            assert_eq!(status, 200, "/{entry} should be served");
        }

        server.stop();
    }

    #[tokio::test]
    async fn missing_paths_are_404_without_a_spa_fallback() {
        let mut server = spawn_fixture().await;
        let (status, body) = body_of(&server, "/docs/guides/does-not-exist.md").await;
        assert_eq!(status, 404);
        assert!(!body.contains("<html"), "404 must not fall back to HTML");

        server.stop();
    }

    #[tokio::test]
    async fn traversal_attempts_are_refused() {
        let mut server = spawn_fixture().await;
        let (status, _) = body_of(&server, "/../Cargo.toml").await;
        assert!(status == 403 || status == 404, "got {status}");

        server.stop();
    }

    #[tokio::test]
    async fn healthz_reports_ok() {
        let mut server = spawn_fixture().await;
        let (status, body) = body_of(&server, "/healthz").await;
        assert_eq!(status, 200);
        assert_eq!(body, "ok");

        server.stop();
    }

    #[tokio::test]
    async fn port_zero_reports_the_chosen_port() {
        let mut server = spawn_fixture().await;
        assert_ne!(server.addr.port(), 0);
        assert!(server.url.contains(&server.addr.port().to_string()));
        server.stop();
    }

    #[tokio::test]
    async fn wildcard_bind_is_reachable_and_reported_as_network() {
        let mut server = start(&fixture_root(), "0.0.0.0", 0)
            .await
            .expect("wildcard bind should work by default");
        assert!(server.serves_network());
        // The reported URL must be dialable, not the wildcard address.
        assert!(!server.url.contains("0.0.0.0"));
        server.stop();
    }
}
