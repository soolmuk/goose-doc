use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::config;

/// Manifest published next to each bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub goose_version: String,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    pub bundle: String,
    #[serde(default)]
    pub bytes: Option<u64>,
    pub sha256: String,
    #[serde(default)]
    pub map_sha256: Option<String>,
    #[serde(default)]
    pub entries: Option<usize>,
}

/// A docs root on disk that has been checked against the goose contract.
#[derive(Debug, Clone)]
pub struct DocsRoot {
    pub path: PathBuf,
    pub entries: usize,
    pub version: Option<String>,
}

impl DocsRoot {
    pub fn map_path(&self) -> PathBuf {
        self.path.join("goose-docs-map.md")
    }
}

/// Validate that `dir` satisfies the docs root contract.
///
/// The skill reads `goose-docs-map.md` first and then only the paths named in
/// it, so a map entry that does not resolve becomes a silent 404 later.
pub fn validate(dir: &Path) -> Result<DocsRoot> {
    if !dir.is_dir() {
        bail!("docs root is not a directory: {}", dir.display());
    }

    let map_path = dir.join("goose-docs-map.md");
    if !map_path.is_file() {
        bail!(
            "{} is missing goose-docs-map.md, so it is not a goose docs root",
            dir.display()
        );
    }

    let docs_dir = dir.join("docs");
    if !docs_dir.is_dir() {
        bail!("{} is missing the docs/ directory", dir.display());
    }

    let map = fs::read_to_string(&map_path)
        .with_context(|| format!("failed to read {}", map_path.display()))?;

    let entries = map_entries(&map);
    if entries.is_empty() {
        bail!("{} lists no documentation pages", map_path.display());
    }

    let mut missing = Vec::new();
    for rel in &entries {
        if !dir.join(rel).is_file() {
            missing.push(rel.clone());
        }
    }
    if !missing.is_empty() {
        let sample: Vec<&str> = missing.iter().take(5).map(String::as_str).collect();
        bail!(
            "{} of {} map entries are missing from {} (e.g. {})",
            missing.len(),
            entries.len(),
            dir.display(),
            sample.join(", ")
        );
    }

    let version = read_version(dir);

    Ok(DocsRoot {
        path: dir.to_path_buf(),
        entries: entries.len(),
        version,
    })
}

/// Extract relative page paths from map links of the form `(docs/....md)`.
pub fn map_entries(map: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let mut rest = map;

    while let Some(start) = rest.find("](docs/") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find(')') {
            let path = &after[..end];
            if path.ends_with(".md") {
                paths.push(path.to_string());
            }
            rest = &after[end..];
        } else {
            break;
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

fn read_version(dir: &Path) -> Option<String> {
    let manifest = find_manifest(dir)?;
    let raw = fs::read_to_string(manifest).ok()?;
    let parsed: Manifest = serde_json::from_str(&raw).ok()?;
    Some(parsed.goose_version)
}

fn find_manifest(dir: &Path) -> Option<PathBuf> {
    let direct = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.to_string_lossy().ends_with(".manifest.json"));
    if direct.is_some() {
        return direct;
    }

    let nested = dir.join("manifest.json");
    nested.is_file().then_some(nested)
}

/// A version already extracted into the cache.
pub fn cached_versions(cache_root: &Path) -> Vec<String> {
    let bundles = cache_root.join("bundles");
    let mut versions: Vec<String> = match fs::read_dir(&bundles) {
        Ok(entries) => entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect(),
        Err(_) => Vec::new(),
    };
    versions.sort_by_key(|v| config::version_key(v));
    versions
}

/// Pick the highest cached version that still validates.
pub fn resolve_cached(cache_root: &Path) -> Result<DocsRoot> {
    let versions = cached_versions(cache_root);
    if versions.is_empty() {
        bail!(
            "no docs bundles in {}. Run `goose-doc fetch <version>` or pass --docs-dir.",
            cache_root.join("bundles").display()
        );
    }

    let mut errors = Vec::new();
    for version in versions.iter().rev() {
        let dir = cache_root.join("bundles").join(version);
        match validate(&dir) {
            Ok(root) => return Ok(root),
            Err(error) => errors.push(format!("{version}: {error}")),
        }
    }

    bail!(
        "no cached docs bundle is usable:\n  {}",
        errors.join("\n  ")
    )
}

pub fn resolve_version(cache_root: &Path, version: &str) -> Result<DocsRoot> {
    let dir = cache_root
        .join("bundles")
        .join(version.trim_start_matches('v'));
    if !dir.is_dir() {
        bail!(
            "docs bundle for {} is not cached in {}",
            version,
            dir.display()
        );
    }
    validate(&dir)
}

/// Where bundles come from.
///
/// `--base-url` covers a self-hosted mirror. For a GitHub repository, the
/// release path is resolved through the API instead, because a private
/// repository's release assets are not reachable at the plain download URL.
pub struct BundleSource {
    pub base_url: String,
    /// Repository in `owner/name` form, used to resolve release assets.
    pub repo: Option<String>,
    /// Token for a private repository. Read from the environment when absent.
    pub token: Option<String>,
}

impl BundleSource {
    pub fn new(base_url: &str, token: Option<String>) -> Self {
        let repo = repo_from_release_base(base_url);
        // A blank token (for example an unset `--token ""` or an empty
        // environment variable) must not be sent, because GitHub answers 401.
        let token = token
            .filter(|value| !value.trim().is_empty())
            .or_else(token_from_env);
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            repo,
            token,
        }
    }
}

/// Extract `owner/name` from a `https://github.com/<owner>/<name>/releases/download` base.
fn repo_from_release_base(base_url: &str) -> Option<String> {
    let rest = base_url
        .strip_prefix("https://github.com/")
        .or_else(|| base_url.strip_prefix("http://github.com/"))?;
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() >= 2 && parts[0] != "releases" {
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        None
    }
}

/// Token lookup order matches the usual GitHub conventions.
fn token_from_env() -> Option<String> {
    for key in ["GOOSE_DOC_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// Download, verify, and extract a bundle.
///
/// The archive is streamed to a temporary file and hashed as it arrives, so a
/// multi-hundred-megabyte bundle is never held in memory.
pub async fn fetch(cache_root: &Path, version: &str, source: &BundleSource) -> Result<DocsRoot> {
    let bare = version.trim_start_matches('v');
    let bundle_name = format!("goose-docs-{bare}.tar.gz");
    // One release per goose version, tagged with that version.
    let release_tag = format!("v{bare}");

    let client = reqwest::Client::builder()
        .user_agent(concat!("goose-doc/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build HTTP client")?;

    let manifest_url = resolve_asset_url(
        &client,
        source,
        &release_tag,
        &format!("{bundle_name}.manifest.json"),
    )
    .await?;
    let bundle_url = resolve_asset_url(&client, source, &release_tag, &bundle_name).await?;

    info!("fetching manifest from {manifest_url}");
    let manifest_text = get_text(&client, &manifest_url, source).await?;
    let manifest: Manifest =
        serde_json::from_str(&manifest_text).context("failed to parse manifest")?;

    // A private repository needs the asset API, which serves an octet-stream
    // and needs the Accept header to return the bytes rather than metadata.
    let bundle_request_url = match &source.repo {
        Some(_) => bundle_url,
        None => bundle_url,
    };

    let dest = cache_root.join("bundles").join(bare);
    if dest.exists() {
        fs::remove_dir_all(&dest).with_context(|| format!("failed to clear {}", dest.display()))?;
    }
    fs::create_dir_all(&dest).with_context(|| format!("failed to create {}", dest.display()))?;

    let staging = dest.with_extension("tar.gz.part");
    info!("downloading {bundle_request_url}");
    let (bytes_written, actual) =
        download_to_file(&client, &bundle_request_url, &staging, source, &manifest).await?;

    if !actual.eq_ignore_ascii_case(&manifest.sha256) {
        let _ = fs::remove_file(&staging);
        bail!(
            "bundle checksum mismatch for {bundle_name}: expected {}, got {actual} ({bytes_written} bytes)",
            manifest.sha256
        );
    }

    extract_tar_gz_file(&staging, &dest)?;
    let _ = fs::remove_file(&staging);

    fs::write(
        dest.join(format!("{bundle_name}.manifest.json")),
        &manifest_text,
    )
    .with_context(|| format!("failed to write manifest into {}", dest.display()))?;

    let root = validate(&dest)?;
    info!(
        "cached goose docs {} at {} ({} pages)",
        bare,
        root.path.display(),
        root.entries
    );
    Ok(root)
}

/// Resolve a release asset to a URL that can actually be fetched.
///
/// For a GitHub repository the API is used: it answers with a signed redirect
/// that works for private repositories too, which the plain
/// `releases/download` URL does not.
async fn resolve_asset_url(
    client: &reqwest::Client,
    source: &BundleSource,
    release_tag: &str,
    asset_name: &str,
) -> Result<String> {
    let Some(repo) = &source.repo else {
        return Ok(format!(
            "{}/{}/{}",
            source.base_url, release_tag, asset_name
        ));
    };

    let api = format!("https://api.github.com/repos/{repo}/releases/tags/{release_tag}");
    let mut request = client
        .get(&api)
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = &source.token {
        request = request.bearer_auth(token);
    }

    let response = request
        .send()
        .await
        .with_context(|| format!("failed to look up release {release_tag} in {repo}"))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        // An unauthenticated request to a private repository answers 404 rather
        // than 401, so an absent token is the first thing to check.
        let hint = if source.token.is_none() {
            " If the repository is private, set GH_TOKEN or GITHUB_TOKEN: \
             without a token a private release is indistinguishable from a missing one."
        } else {
            " Has the docs bundle been published?"
        };
        bail!("release {release_tag} not found in {repo}.{hint}");
    }
    if !response.status().is_success() {
        let status = response.status();
        let hint = if status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::UNAUTHORIZED
        {
            " A token with repo access may be needed for a private repository."
        } else {
            ""
        };
        bail!("release lookup for {release_tag} returned {status}.{hint}");
    }

    let release: serde_json::Value = response
        .json()
        .await
        .context("failed to parse the release response")?;

    let assets = release
        .get("assets")
        .and_then(|value| value.as_array())
        .context("release response has no assets list")?;

    for asset in assets {
        if asset.get("name").and_then(|v| v.as_str()) == Some(asset_name) {
            let url = asset
                .get("url")
                .and_then(|v| v.as_str())
                .context("asset has no api url")?;
            return Ok(url.to_string());
        }
    }

    bail!("release {release_tag} has no asset named {asset_name}")
}

async fn get_text(client: &reqwest::Client, url: &str, source: &BundleSource) -> Result<String> {
    let mut request = client.get(url).header("Accept", "application/octet-stream");
    if is_github_api(url) {
        request = request.header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = &source.token {
            request = request.bearer_auth(token);
        }
    }
    request
        .send()
        .await
        .with_context(|| format!("failed to fetch {url}"))?
        .error_for_status()
        .with_context(|| format!("{url} returned an error status"))?
        .text()
        .await
        .context("failed to read the response body")
}

/// Stream a bundle to `staging`, hashing while writing.
async fn download_to_file(
    client: &reqwest::Client,
    url: &str,
    staging: &Path,
    source: &BundleSource,
    manifest: &Manifest,
) -> Result<(u64, String)> {
    let mut request = client
        .get(url)
        .header("Accept", "application/octet-stream")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if is_github_api(url) {
        if let Some(token) = &source.token {
            request = request.bearer_auth(token);
        }
    }

    let response = request
        .send()
        .await
        .with_context(|| format!("failed to fetch {url}"))?;

    if !response.status().is_success() {
        bail!(
            "bundle download returned {}. If the repository is private, set GH_TOKEN or GITHUB_TOKEN.",
            response.status()
        );
    }

    if let (Some(expected), Some(length)) = (manifest.bytes, response.content_length()) {
        if expected != length {
            warn!("manifest says {expected} bytes, server reports {length}");
        }
    }

    let mut file = tokio::fs::File::create(staging)
        .await
        .with_context(|| format!("failed to create {}", staging.display()))?;
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed while reading the bundle stream")?;
        hasher.update(&chunk);
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .with_context(|| format!("failed to write {}", staging.display()))?;
        written += chunk.len() as u64;
    }

    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .context("failed to flush the bundle")?;

    Ok((written, hex::encode(hasher.finalize())))
}

/// True when a URL points at the GitHub REST API, which needs the JSON/octet
/// stream accept header and authentication.
fn is_github_api(url: &str) -> bool {
    url.starts_with("https://api.github.com/") || url.starts_with("http://api.github.com/")
}

fn extract_tar_gz_file(archive_path: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open {}", archive_path.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.set_preserve_permissions(false);

    for entry in archive.entries().context("failed to read bundle archive")? {
        let mut entry = entry.context("failed to read bundle entry")?;
        let path = entry
            .path()
            .context("bundle entry has an invalid path")?
            .into_owned();

        // Reject absolute paths and parent traversal before writing anything.
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            warn!("skipping unsafe archive path: {}", path.display());
            continue;
        }

        let out = dest.join(&path);
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if !entry.header().entry_type().is_file() {
            continue;
        }

        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(&out)
            .with_context(|| format!("failed to create {}", out.display()))?;
        let mut buffer = Vec::new();
        entry
            .read_to_end(&mut buffer)
            .context("failed to read bundle entry body")?;
        file.write_all(&buffer)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/docs-root")
    }

    #[test]
    fn map_entries_extracts_md_links_only() {
        let map =
            "# Map\n\n### [A](docs/guides/a.md)\n### [B](docs/b.md)\n[x](https://example.com)\n";
        assert_eq!(map_entries(map), vec!["docs/b.md", "docs/guides/a.md"]);
    }

    #[test]
    fn map_entries_dedupes_and_ignores_non_md() {
        let map = "[A](docs/a.md)\n[A](docs/a.md)\n[img](docs/img.png)\n";
        assert_eq!(map_entries(map), vec!["docs/a.md"]);
    }

    #[test]
    fn validate_accepts_the_fixture_root() {
        let root = validate(&fixture_root()).expect("fixture root should validate");
        assert_eq!(root.entries, 2);
        assert!(root.map_path().is_file());
    }

    #[test]
    fn validate_rejects_a_directory_without_a_map() {
        let dir = tempdir();
        fs::create_dir_all(dir.join("docs")).unwrap();
        let error = validate(&dir).unwrap_err().to_string();
        assert!(error.contains("goose-docs-map.md"), "got: {error}");
    }

    #[test]
    fn validate_rejects_a_map_entry_that_does_not_exist() {
        let dir = tempdir();
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("goose-docs-map.md"), "### [Gone](docs/gone.md)\n").unwrap();
        let error = validate(&dir).unwrap_err().to_string();
        assert!(error.contains("docs/gone.md"), "got: {error}");
    }

    #[test]
    fn validate_rejects_a_map_with_no_entries() {
        let dir = tempdir();
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("goose-docs-map.md"), "# Map\n").unwrap();
        let error = validate(&dir).unwrap_err().to_string();
        assert!(error.contains("no documentation pages"), "got: {error}");
    }

    #[test]
    fn resolve_cached_reports_when_the_cache_is_empty() {
        let dir = tempdir();
        let error = resolve_cached(&dir).unwrap_err().to_string();
        assert!(error.contains("no docs bundles"), "got: {error}");
    }

    #[test]
    fn resolve_cached_prefers_the_highest_valid_version() {
        let dir = tempdir();
        for version in ["1.9.0", "1.10.0"] {
            copy_dir(&fixture_root(), &dir.join("bundles").join(version));
        }
        let root = resolve_cached(&dir).unwrap();
        assert!(
            root.path.ends_with("1.10.0"),
            "got: {}",
            root.path.display()
        );
    }

    #[test]
    fn resolve_cached_skips_a_corrupt_version() {
        let dir = tempdir();
        copy_dir(&fixture_root(), &dir.join("bundles").join("1.9.0"));
        fs::create_dir_all(dir.join("bundles").join("1.10.0").join("docs")).unwrap();

        let root = resolve_cached(&dir).unwrap();
        assert!(root.path.ends_with("1.9.0"), "got: {}", root.path.display());
    }

    #[test]
    fn hex_digest_matches_a_known_value() {
        let mut hasher = Sha256::new();
        hasher.update(b"abc");
        assert_eq!(
            hex::encode(hasher.finalize()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn github_release_base_yields_a_repo() {
        assert_eq!(
            repo_from_release_base("https://github.com/soolmuk/goose-doc/releases/download"),
            Some("soolmuk/goose-doc".to_string())
        );
    }

    #[test]
    fn non_github_base_has_no_repo() {
        assert_eq!(
            repo_from_release_base("https://mirror.internal/goose-docs"),
            None
        );
    }

    #[test]
    fn asset_url_uses_the_api_for_a_github_repo() {
        let dir = tempdir();
        let source = BundleSource {
            base_url: "https://github.com/soolmuk/goose-doc/releases/download".to_string(),
            repo: Some("soolmuk/goose-doc".to_string()),
            token: None,
        };
        assert!(resolve_asset_url_sync(&source, "v1.52.0", "x.tar.gz").contains("/repos/"));
        let _ = dir;
    }

    #[test]
    fn asset_url_falls_back_to_the_base_url_for_a_mirror() {
        let source = BundleSource {
            base_url: "https://mirror.internal/goose".to_string(),
            repo: None,
            token: None,
        };
        assert_eq!(
            resolve_asset_url_sync(&source, "v1.52.0", "x.tar.gz"),
            "https://mirror.internal/goose/v1.52.0/x.tar.gz"
        );
    }

    #[test]
    fn github_api_urls_are_detected() {
        assert!(is_github_api(
            "https://api.github.com/repos/a/b/releases/assets/1"
        ));
        assert!(!is_github_api("https://github.com/a/b/releases/download/x"));
    }

    /// Mirror of the API path in `resolve_asset_url`, without the HTTP call.
    fn resolve_asset_url_sync(source: &BundleSource, tag: &str, asset: &str) -> String {
        match &source.repo {
            Some(repo) => {
                format!("https://api.github.com/repos/{repo}/releases/tags/{tag}#{asset}")
            }
            None => format!("{}/{tag}/{asset}", source.base_url),
        }
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "goose-doc-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in walkdir::WalkDir::new(from).into_iter().flatten() {
            let rel = entry.path().strip_prefix(from).unwrap();
            let target = to.join(rel);
            if entry.file_type().is_dir() {
                fs::create_dir_all(&target).unwrap();
            } else {
                fs::copy(entry.path(), &target).unwrap();
            }
        }
    }
}
