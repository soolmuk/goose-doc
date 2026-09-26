use std::path::PathBuf;

/// Default bind address. Remote binding requires an explicit opt-in.
pub const DEFAULT_BIND: &str = "127.0.0.1";

/// Default port. Chosen so several goose-doc instances can coexist and so the
/// port is easy to spot in `netstat` output.
pub const DEFAULT_PORT: u16 = 10650;

/// Root cache directory for downloaded bundles.
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("goose-doc")
}

/// Directory holding persisted settings.
pub fn config_dir() -> PathBuf {
    cache_dir()
}

pub fn bundles_dir() -> PathBuf {
    cache_dir().join("bundles")
}

/// Directory holding the extracted bundle for a version.
pub fn bundle_dir(version: &str) -> PathBuf {
    bundles_dir().join(version.trim_start_matches('v'))
}

/// Sort key for version directory names, so "1.9.0" sorts below "1.52.0".
pub fn version_key(name: &str) -> (u64, u64, u64, String) {
    let mut parts = name.split('.').map(|p| {
        p.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0)
    });
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        name.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_key_orders_numerically_not_lexically() {
        let mut versions = vec!["1.9.0", "1.52.0", "1.10.0", "2.0.0"];
        versions.sort_by_key(|v| version_key(v));
        assert_eq!(versions, vec!["1.9.0", "1.10.0", "1.52.0", "2.0.0"]);
    }

    #[test]
    fn bundle_dir_accepts_tag_or_version() {
        assert_eq!(bundle_dir("1.52.0"), bundle_dir("v1.52.0"));
    }
}
