//! goose-doc: serve a goose docs root so `goose-doc-guide` can read it without
//! network access.
//!
//! The docs root contract is `<root>/goose-docs-map.md` plus `<root>/docs/**`,
//! where every path listed in the map resolves relative to the root.

pub mod addr;
pub mod cli;
pub mod config;
pub mod docs;
pub mod panel;
pub mod server;
pub mod settings;
