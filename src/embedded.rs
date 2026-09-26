//! Documentation embedded in the binary.
//!
//! The content of `src/embedded` is written by `tools/embed-docs.sh` and baked
//! in at compile time, which is what lets a single executable serve the
//! documentation without a download.
//!
//! What gets embedded is a goose docs root: the map, the markdown pages the
//! `goose-doc-guide` skill reads, and — when the site build is embedded — the
//! HTML pages and assets too, so the same port answers a browser with the real
//! documentation site. A downloaded bundle still takes precedence when one is
//! present, because it can also carry the blog media the binary leaves out.

use include_dir::{include_dir, Dir};
use std::path::{Component, Path, PathBuf};

static EMBEDDED: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/embedded");

/// Marker so an operator can tell whether the binary carries docs.
pub fn is_available() -> bool {
    EMBEDDED.get_file("goose-docs-map.md").is_some()
}

/// True when the embedded root also carries the built HTML site.
///
/// A docs root built with `npm run build` contains `index.html`; the minimal
/// markdown-only root does not. The panel reports which one is in use.
pub fn has_site() -> bool {
    EMBEDDED.get_file("index.html").is_some()
}

/// Version recorded by the bundle that was embedded.
pub fn version() -> Option<String> {
    let raw = EMBEDDED.get_file("manifest.json")?.contents_utf8()?;
    let parsed: serde_json::Value = serde_json::from_str(raw).ok()?;
    parsed
        .get("goose_version")
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

/// Number of pages listed in the embedded map.
pub fn entries() -> usize {
    EMBEDDED
        .get_file("goose-docs-map.md")
        .and_then(|file| file.contents_utf8())
        .map(|map| crate::docs::map_entries(map).len())
        .unwrap_or(0)
}

/// Read a file by its path relative to the docs root.
///
/// A directory request resolves to its `index.html`, which is what makes the
/// built site browsable: `/docs/getting-started/installation/` and the
/// extensionless route `/docs/getting-started/installation` both land on
/// `docs/getting-started/installation/index.html`.
///
/// Returns `None` for anything that escapes the root, mirroring the traversal
/// rules the on-disk server applies.
pub fn resolve(path: &Path) -> Option<(PathBuf, &'static [u8])> {
    let relative = safe_relative(path)?;

    let candidates: [PathBuf; 3] = [
        relative.clone(),
        relative.join("index.html"),
        // A site route that drops the suffix, e.g. `/docs/guides/logs`.
        with_html(&relative),
    ];
    for candidate in candidates {
        if candidate.as_os_str().is_empty() {
            if let Some(file) = EMBEDDED.get_file("index.html") {
                return Some((PathBuf::from("index.html"), file.contents()));
            }
            continue;
        }
        if let Some(file) = EMBEDDED.get_file(&candidate) {
            return Some((candidate, file.contents()));
        }
    }
    None
}

/// `docs/guides/logs` -> `docs/guides/logs.html`.
fn with_html(path: &Path) -> PathBuf {
    let mut candidate = path.to_path_buf();
    let name = match candidate.file_name().and_then(|n| n.to_str()) {
        Some(name) if !name.contains('.') => format!("{name}.html"),
        _ => return candidate,
    };
    candidate.set_file_name(name);
    candidate
}

/// Read a file's bytes, discarding the resolved path. Convenience for callers
/// that only need the content, such as tests.
pub fn get(path: &Path) -> Option<&'static [u8]> {
    resolve(path).map(|(_, bytes)| bytes)
}

/// List every embedded path, relative to the docs root.
pub fn paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(&EMBEDDED, &mut out);
    out.sort();
    out
}

fn collect(dir: &Dir<'_>, out: &mut Vec<PathBuf>) {
    for file in dir.files() {
        out.push(file.path().to_path_buf());
    }
    for child in dir.dirs() {
        collect(child, out);
    }
}

fn safe_relative(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_binary_carries_documentation() {
        // If this fails, run tools/embed-docs.sh before building.
        assert!(
            is_available(),
            "no embedded docs; run tools/embed-docs.sh <bundle.tar.gz>"
        );
    }

    #[test]
    fn the_embedded_map_resolves_completely() {
        let map = EMBEDDED
            .get_file("goose-docs-map.md")
            .and_then(|file| file.contents_utf8())
            .expect("embedded map");

        let entries = crate::docs::map_entries(map);
        assert!(!entries.is_empty(), "embedded map lists no pages");

        for entry in &entries {
            assert!(
                get(Path::new(entry)).is_some(),
                "embedded map names {entry}, which is not embedded"
            );
        }
    }

    #[test]
    fn map_is_readable_at_the_root() {
        let contents = get(Path::new("goose-docs-map.md")).expect("map");
        let text = std::str::from_utf8(contents).expect("utf-8");
        assert!(text.contains("goose Documentation Map"));
    }

    #[test]
    fn traversal_is_refused() {
        assert!(get(Path::new("../Cargo.toml")).is_none());
        assert!(get(Path::new("/etc/passwd")).is_none());
    }

    #[test]
    fn missing_paths_are_none() {
        assert!(get(Path::new("docs/guides/does-not-exist.md")).is_none());
    }

    #[test]
    fn version_and_entries_are_reported() {
        // Both come from the embedded bundle, so they must be present.
        assert!(version().is_some(), "embedded bundle has no version");
        assert!(entries() > 0, "embedded bundle reports no pages");
    }

    #[test]
    fn a_page_the_map_names_is_readable() {
        let map = EMBEDDED
            .get_file("goose-docs-map.md")
            .and_then(|file| file.contents_utf8())
            .expect("embedded map");
        let first = crate::docs::map_entries(map).remove(0);
        let bytes = get(Path::new(&first)).expect("first map entry");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn the_site_is_browsable_when_it_is_embedded() {
        if !has_site() {
            // A markdown-only root is a valid configuration; nothing to check.
            return;
        }

        // The root and an extensionless directory route both resolve to HTML.
        let root = get(Path::new("")).expect("root index");
        assert!(String::from_utf8_lossy(root).contains("<html"));

        let page = get(Path::new("docs/getting-started/installation")).expect("installation page");
        assert!(String::from_utf8_lossy(page).contains("<html"));
    }

    /// The embedded payload must stay bounded: the site carries the pages and
    /// their assets, but never the blog media or videos that make the published
    /// bundle hundreds of megabytes.
    #[test]
    fn embedded_docs_stay_bounded() {
        let total: usize = paths()
            .iter()
            .filter_map(|path| get(path))
            .map(<[u8]>::len)
            .sum();

        assert!(
            total < 120 * 1024 * 1024,
            "embedded docs are {total} bytes; the full 344 MB site bundle was \
             embedded instead of a trimmed one; run tools/build-site-lite.py"
        );
    }

    #[test]
    fn a_directory_request_resolves_to_its_index() {
        if !has_site() {
            return;
        }
        // The built site is addressed without a file name, exactly as a browser
        // requests it.
        for route in [
            "docs/getting-started/installation",
            "docs/getting-started/installation/",
        ] {
            let (resolved, bytes) = resolve(Path::new(route)).expect(route);
            assert!(
                resolved.ends_with("index.html"),
                "got {}",
                resolved.display()
            );
            assert!(String::from_utf8_lossy(bytes).contains("<html"));
        }
    }

    #[test]
    fn a_site_route_without_a_suffix_resolves_to_html() {
        if !has_site() {
            return;
        }
        // Docusaurus serves `/docs/category/guides` from `.html`; the built site
        // contains both that file and its `index.html` directory.
        let (resolved, _) = resolve(Path::new("docs/guides/logs")).expect("logs page");
        assert!(
            resolved.ends_with("index.html") || resolved.ends_with("logs.html"),
            "got {}",
            resolved.display()
        );
    }

    #[test]
    fn paths_include_the_map_and_pages() {
        let paths = paths();
        assert!(paths.iter().any(|p| p.ends_with("goose-docs-map.md")));
        assert!(paths
            .iter()
            .any(|p| p.to_string_lossy().starts_with("docs/")));
    }
}
