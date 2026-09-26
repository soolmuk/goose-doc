//! End-to-end tests: start the real server on an ephemeral port and exercise it
//! over HTTP, including the offline skill contract that goose depends on.

use std::path::{Path, PathBuf};

use goose_doc::docs;
use goose_doc::server;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/docs-root")
}

fn validated_fixture() -> docs::DocsRoot {
    docs::validate(&fixture_root()).expect("fixture docs root")
}

async fn get(url: &str) -> (u16, String, String) {
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .expect("request failed");
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = response.text().await.unwrap_or_default();
    (status, content_type, body)
}

#[tokio::test]
async fn serves_a_complete_docs_root_over_http() {
    let mut server = server::start(&validated_fixture(), "127.0.0.1", 0)
        .await
        .expect("server should start");

    // The URL is what a user hands to GOOSE_DOCS_ROOT.
    let (status, content_type, body) = get(&format!("{}/goose-docs-map.md", server.url)).await;
    assert_eq!(status, 200);
    assert!(content_type.starts_with("text/plain"));
    assert!(body.contains("goose Documentation Map"));

    // Every path named in the map must be reachable, since the skill only reads
    // paths it finds there.
    for entry in docs::map_entries(&body) {
        let (status, _, _) = get(&format!("{}/{entry}", server.url)).await;
        assert_eq!(status, 200, "/{entry} should be served");
    }

    server.stop();
}

#[tokio::test]
async fn default_bind_serves_the_network_and_reports_a_dialable_url() {
    // Hosting for other machines is the whole point of the tool, so the default
    // bind must be reachable from the network without extra flags.
    let mut server = server::start(&validated_fixture(), goose_doc::addr::DEFAULT_BIND, 0)
        .await
        .expect("wildcard bind should succeed by default");

    assert!(server.serves_network());
    assert!(
        !server.url.contains("0.0.0.0"),
        "the reported url must be dialable, got {}",
        server.url
    );

    // The advertised URL actually answers.
    let (status, _, _) = get(&format!("{}/healthz", server.url)).await;
    assert_eq!(status, 200);

    server.stop();
}

#[tokio::test]
async fn ephemeral_port_is_reported_in_the_url() {
    let mut server = server::start(&validated_fixture(), "127.0.0.1", 0)
        .await
        .expect("server should start");

    assert_ne!(server.addr.port(), 0);
    assert!(server.url.ends_with(&server.addr.port().to_string()));

    server.stop();
}

#[tokio::test]
async fn stopping_releases_the_port() {
    let mut server = server::start(&validated_fixture(), "127.0.0.1", 0)
        .await
        .expect("server should start");
    let port = server.addr.port();

    server.stop();
    // Give the accept loop a moment to drop the listener.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let rebound = tokio::net::TcpListener::bind(("127.0.0.1", port)).await;
    assert!(rebound.is_ok(), "port {port} should be free after stop");
}

#[tokio::test]
async fn request_counter_tracks_served_requests() {
    let mut server = server::start(&validated_fixture(), "127.0.0.1", 0)
        .await
        .expect("server should start");

    assert_eq!(server.request_count(), 0);
    get(&format!("{}/goose-docs-map.md", server.url)).await;
    get(&format!("{}/docs/guides/offline-docs.md", server.url)).await;
    assert_eq!(server.request_count(), 2);

    server.stop();
}

#[tokio::test]
async fn local_only_bind_is_not_network_reachable() {
    let mut server = server::start(&validated_fixture(), goose_doc::addr::LOOPBACK, 0)
        .await
        .expect("server should start");

    assert!(!server.serves_network());
    assert!(server.url.starts_with("http://127.0.0.1:"));
    server.stop();
}
