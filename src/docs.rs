use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
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

/// The bundle layout is `<root>/goose-docs-<version>.tar.gz.manifest.json`, or
/// a manifest placed directly in the extracted directory.
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

    dir.join("manifest.json")
        .is_file()
        .then(|| dir.join("manifest.json"))
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

/// Download, verify, and extract a bundle.
pub async fn fetch(cache_root: &Path, version: &str, base_url: &str) -> Result<DocsRoot> {
    let bare = version.trim_start_matches('v');
    let bundle_name = format!("goose-docs-{bare}.tar.gz");
    let release_tag = format!("docs-v{bare}");
    let base = format!("{}/{}", base_url.trim_end_matches('/'), release_tag);
    let bundle_url = format!("{base}/{bundle_name}");
    let manifest_url = format!("{base}/{bundle_name}.manifest.json");

    let client = reqwest::Client::builder()
        .build()
        .context("failed to build HTTP client")?;

    info!("fetching manifest from {manifest_url}");
    let manifest_text = client
        .get(&manifest_url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {manifest_url}"))?
        .error_for_status()
        .with_context(|| format!("{manifest_url} returned an error status"))?
        .text()
        .await
        .context("failed to read manifest body")?;
    let manifest: Manifest =
        serde_json::from_str(&manifest_text).context("failed to parse manifest")?;

    info!("downloading {bundle_url}");
    let bytes = client
        .get(&bundle_url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {bundle_url}"))?
        .error_for_status()
        .with_context(|| format!("{bundle_url} returned an error status"))?
        .bytes()
        .await
        .context("failed to read bundle body")?;

    let actual = hex_digest(&bytes);
    if !actual.eq_ignore_ascii_case(&manifest.sha256) {
        bail!(
            "bundle checksum mismatch for {bundle_name}: expected {}, got {actual}",
            manifest.sha256
        );
    }

    let dest = cache_root.join("bundles").join(bare);
    if dest.exists() {
        fs::remove_dir_all(&dest).with_context(|| format!("failed to clear {}", dest.display()))?;
    }
    fs::create_dir_all(&dest).with_context(|| format!("failed to create {}", dest.display()))?;

    extract_tar_gz(&bytes, &dest)?;

    // Keep the manifest inside the extracted root so the version is discoverable.
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

pub fn hex_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn extract_tar_gz(bytes: &[u8], dest: &Path) -> Result<()> {
    let decoder = flate2::read::GzDecoder::new(bytes);
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
        std::io::Write::write_all(&mut file, &buffer)?;
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
        let entries = map_entries(map);
        assert_eq!(entries, vec!["docs/b.md", "docs/guides/a.md"]);
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
            let target = dir.join("bundles").join(version);
            copy_dir(&fixture_root(), &target);
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
        assert_eq!(
            hex_digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
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
