#!/usr/bin/env bash
#
# Run fmt/clippy/test inside a Linux container, matching CI.
#
# Why: `cargo clippy --target x86_64-unknown-linux-gnu` does not work from
# macOS because a transitive C dependency needs a Linux cross toolchain, so
# platform-specific dead code and cfg mistakes only surface in CI. This script
# reproduces the CI environment locally instead.
#
# Requires docker. The cargo registry and target directory are kept in named
# volumes so repeat runs are fast.
#
# Usage: check-linux.sh [--release]
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v docker >/dev/null 2>&1; then
  echo "Error: docker is required" >&2
  exit 1
fi

PLATFORM="${PLATFORM:-linux/arm64}"
IMAGE="${IMAGE:-ubuntu:24.04}"

echo "==> Running Linux checks in $IMAGE ($PLATFORM)"

docker run --rm \
  --platform "$PLATFORM" \
  -v "$ROOT":/src \
  -w /src \
  -v goose-doc-cargo:/root/.cargo \
  -v goose-doc-target:/src/target \
  "$IMAGE" bash -c '
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

# The packages are installed unconditionally: ca-certificates has to be present
# before cargo can reach crates.io, and the build needs the X11/xkb libraries.
apt-get update -qq >/dev/null 2>&1
apt-get install -y -qq curl ca-certificates build-essential pkg-config \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev \
  libssl-dev >/dev/null 2>&1

if [ ! -x /root/.cargo/bin/cargo ]; then
  curl -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain 1.96.1 >/dev/null 2>&1
  /root/.cargo/bin/rustup component add rustfmt clippy >/dev/null 2>&1
fi
. "$HOME/.cargo/env"

echo "--- rustc: $(rustc --version) ---"

echo "--- cargo fmt --check ---"
cargo fmt --check

echo "--- cargo clippy --all-targets -- -D warnings ---"
cargo clippy --all-targets -- -D warnings

echo "--- cargo test ---"
cargo test 2>&1 | grep -E "test result|running" | tail -8

echo "--- smoke test ---"
cargo build --release 2>&1 | tail -2
./tools/smoke-test.sh ./target/release/goose-doc 2>&1 | tail -6
'

echo
echo "==> Linux checks passed"
