//! Documentation embedded in the binary.
//!
//! The content of `src/embedded` is written by `tools/embed-docs.sh` and baked
//! in at compile time, which is what lets a single executable serve the
//! documentation without a download.
//!
//! Only the pages the skill reads are embedded (the map plus the files it
//! names), so the binary stays small. A downloaded site bundle, which also
//! carries the blog images and videos needed to browse the HTML site, takes
//! precedence when one is present.

use include_dir::{include_dir, Dir};
use std::path::{Component, Path, PathBuf};

static EMBEDDED: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/embedded");

/// Marker so an operator can tell whether the binary carries docs.
pub fn is_available() -> bool {
    EMBEDDED.get_file("goose-docs-map.md").is_some()
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
/// Returns `None` for anything that escapes the root, mirrors the traversal
/// rules the on-disk server applies.
pub fn get(path: &Path) -> Option<&'static [u8]> {
    let relative = safe_relative(path)?;
    EMBEDDED.get_file(&relative).map(|file| file.contents())
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
    if out.as_os_str().is_empty() {
        return None;
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

    /// The binary must stay small, so the embedded docs must stay small. This
    /// catches accidentally embedding the full site bundle (hundreds of
    /// megabytes of blog media) instead of the lean one.
    #[test]
    fn embedded_docs_stay_small() {
        let total: usize = paths()
            .iter()
            .filter_map(|path| get(path))
            .map(<[u8]>::len)
            .sum();

        assert!(
            total < 4 * 1024 * 1024,
            "embedded docs are {total} bytes, which means the full site bundle \
             was embedded instead of the lean one; check that embed-docs.sh was \
             given a *-lean.tar.gz"
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
