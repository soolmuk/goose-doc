//! Rebuild when the embedded documentation changes.
//!
//! `include_dir!` expands the directory at compile time but does not declare
//! `src/embedded` as a dependency, so cargo's fingerprint does not include those
//! files. `tools/embed-docs.sh` rewrites that directory *after* a build, which
//! is exactly the case that has to be caught: `cargo build` would otherwise
//! finish in 0.1s and leave the previously embedded documentation in the binary.
//!
//! Without this, a release binary can carry docs for the wrong goose version
//! even though the bundle that was downloaded and checksum-verified is correct.

fn main() {
    println!("cargo:rerun-if-changed=src/embedded");
}
