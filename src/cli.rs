use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::addr::{DEFAULT_BIND, LOOPBACK};
use crate::config::DEFAULT_PORT;

/// Where `goose-doc fetch` looks for bundles. Override with --base-url when
/// bundles are mirrored elsewhere.
pub const DEFAULT_FETCH_BASE_URL: &str =
    "https://github.com/soolmuk/goose-doc/releases/download";

#[derive(Parser, Debug)]
#[command(
    name = "goose-doc",
    version,
    about = "Serve goose documentation for the goose-doc-guide skill"
)]
pub struct Cli {
    /// Address to listen on. Defaults to every interface so other machines can
    /// reach the docs server; use --local-only to restrict to this machine.
    #[arg(long, default_value = DEFAULT_BIND)]
    pub bind: Option<String>,

    /// Listen on 127.0.0.1 only. Shorthand for --bind 127.0.0.1.
    #[arg(long, conflicts_with = "bind")]
    pub local_only: bool,

    /// Port to listen on. 0 picks a free port.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,

    /// Serve a docs root from disk instead of a downloaded bundle.
    #[arg(long, conflicts_with = "docs_version")]
    pub docs_dir: Option<PathBuf>,

    /// Serve a specific bundle version, e.g. 1.52.0.
    #[arg(long)]
    pub docs_version: Option<String>,

    /// Override the bundle cache directory.
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,

    /// Serve without the panel (servers and containers).
    #[arg(long)]
    pub headless: bool,

    /// Open a browser once the server is listening.
    #[arg(long)]
    pub open: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    /// Bind address after applying --local-only.
    pub fn bind_addr(&self) -> String {
        if self.local_only {
            LOOPBACK.to_string()
        } else {
            self.bind
                .clone()
                .unwrap_or_else(|| DEFAULT_BIND.to_string())
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Download and extract a docs bundle into the cache.
    Fetch {
        /// goose release tag or version, e.g. v1.52.0 or 1.52.0.
        version: String,
        /// Base URL holding the bundle and its manifest.
        #[arg(long, default_value = DEFAULT_FETCH_BASE_URL)]
        base_url: String,
        /// Override the bundle cache directory.
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
    /// Show what goose-doc would serve and whether the docs root is valid.
    Doctor {
        /// Docs root to inspect. Defaults to the resolved cached bundle.
        #[arg(long)]
        docs_dir: Option<PathBuf>,
        /// Bundle version to inspect.
        #[arg(long)]
        docs_version: Option<String>,
        /// Override the bundle cache directory.
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
    /// List the addresses this machine can serve on.
    Addresses,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn port_defaults_to_10650() {
        let cli = Cli::parse_from(["goose-doc"]);
        assert_eq!(cli.port, DEFAULT_PORT);
        assert_eq!(cli.port, 10650);
    }

    #[test]
    fn bind_defaults_to_every_interface() {
        let cli = Cli::parse_from(["goose-doc"]);
        assert_eq!(cli.bind_addr(), "0.0.0.0");
    }

    #[test]
    fn local_only_switches_to_loopback() {
        let cli = Cli::parse_from(["goose-doc", "--local-only"]);
        assert_eq!(cli.bind_addr(), "127.0.0.1");
    }

    #[test]
    fn explicit_bind_wins_when_not_local_only() {
        let cli = Cli::parse_from(["goose-doc", "--bind", "192.168.1.5"]);
        assert_eq!(cli.bind_addr(), "192.168.1.5");
    }

    #[test]
    fn local_only_and_bind_conflict() {
        let parsed = Cli::try_parse_from(["goose-doc", "--local-only", "--bind", "1.2.3.4"]);
        assert!(parsed.is_err());
    }

    #[test]
    fn docs_dir_and_version_conflict() {
        let parsed = Cli::try_parse_from([
            "goose-doc",
            "--docs-dir",
            "/tmp/docs",
            "--docs-version",
            "1.52.0",
        ]);
        assert!(parsed.is_err());
    }

    #[test]
    fn fetch_takes_a_version() {
        let cli = Cli::parse_from(["goose-doc", "fetch", "v1.52.0"]);
        match cli.command {
            Some(Command::Fetch { version, .. }) => assert_eq!(version, "v1.52.0"),
            other => panic!("expected fetch, got {other:?}"),
        }
    }

    #[test]
    fn addresses_command_parses() {
        let cli = Cli::parse_from(["goose-doc", "addresses"]);
        assert!(matches!(cli.command, Some(Command::Addresses)));
    }

    #[test]
    fn allow_remote_is_gone() {
        // Serving the network is the default now, so the old opt-in flag must
        // not exist.
        let parsed = Cli::try_parse_from(["goose-doc", "--allow-remote"]);
        assert!(parsed.is_err());
    }
}
