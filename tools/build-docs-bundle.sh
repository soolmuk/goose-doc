#!/usr/bin/env bash
#
# Build a goose docs root bundle from a goose release tag.
#
# Produces a docs root that satisfies the goose-doc-guide contract:
#   <root>/goose-docs-map.md + <root>/docs/**
#
# Requires: git, node, npm, jq, tar, sha256sum/shasum
# Usage: build-docs-bundle.sh <goose-version> [--out-dir DIR] [--keep-src]
#
set -euo pipefail

GOOSE_REPO_URL="${GOOSE_REPO_URL:-https://github.com/aaif-goose/goose}"

usage() {
  cat <<'EOF'
Usage: build-docs-bundle.sh <goose-version> [options]

  <goose-version>   goose release tag or bare version (e.g. v1.52.0 or 1.52.0)

Options:
  --out-dir DIR     Output directory for the bundle (default: ./out)
  --work-dir DIR    Working directory for the goose checkout (default: mktemp)
  --keep-src        Reuse an existing checkout in --work-dir instead of re-cloning
  -h, --help        Show this help
EOF
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

VERSION=""
OUT_DIR="./out"
WORK_DIR=""
KEEP_SRC=0

while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --work-dir) WORK_DIR="$2"; shift 2 ;;
    --keep-src) KEEP_SRC=1; shift ;;
    -*) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    *) VERSION="$1"; shift ;;
  esac
done

if [ -z "$VERSION" ]; then
  echo "Error: goose version is required" >&2
  usage >&2
  exit 2
fi

# Accept both "1.52.0" and "v1.52.0".
case "$VERSION" in
  v*) TAG="$VERSION"; BARE_VERSION="${VERSION#v}" ;;
  *)  TAG="v$VERSION"; BARE_VERSION="$VERSION" ;;
esac

for tool in git node npm jq tar; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Error: '$tool' is required" >&2; exit 1; }
done

mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"

if [ -z "$WORK_DIR" ]; then
  WORK_DIR="$(mktemp -d)"
  trap 'rm -rf "$WORK_DIR"' EXIT
else
  mkdir -p "$WORK_DIR"
fi
SRC_DIR="$WORK_DIR/goose"

echo "==> goose docs bundle: $TAG"
echo "    work dir: $WORK_DIR"
echo "    out dir:  $OUT_DIR"

# ---------------------------------------------------------------------------
# 1) Check out the exact release tag
# ---------------------------------------------------------------------------
if [ "$KEEP_SRC" = "1" ] && [ -d "$SRC_DIR/.git" ]; then
  echo "==> Reusing existing checkout at $SRC_DIR"
  git -C "$SRC_DIR" fetch --depth 1 --tags origin "refs/tags/$TAG:refs/tags/$TAG" 2>/dev/null || true
  git -C "$SRC_DIR" checkout --quiet "$TAG"
else
  echo "==> Cloning $GOOSE_REPO_URL at $TAG"
  rm -rf "$SRC_DIR"
  # A shallow clone carries only the default branch, so the tag is fetched
  # explicitly. Relying on `checkout <tag>` alone fails with "pathspec did not
  # match" because the tag is not in the shallow history.
  git clone --quiet --depth 1 --no-checkout "$GOOSE_REPO_URL" "$SRC_DIR"
  git -C "$SRC_DIR" fetch --quiet --depth 1 origin "refs/tags/$TAG:refs/tags/$TAG"
  git -C "$SRC_DIR" checkout --quiet "$TAG"
fi

ACTUAL_TAG="$(git -C "$SRC_DIR" describe --tags --exact-match 2>/dev/null || echo "$TAG")"
COMMIT="$(git -C "$SRC_DIR" rev-parse HEAD)"

# ---------------------------------------------------------------------------
# 2) Generate the ACP reference page
#
# docs/gdk/acp/index.md links to docs/gdk/acp/reference.md, which is generated
# and not committed. Without it the build fails on onBrokenLinks: "throw".
# The upstream helper resolves the version via GitHub's "releases/latest",
# which can point at a different tag than the one we checked out, so the
# version is pinned explicitly here.
# ---------------------------------------------------------------------------
echo "==> Generating ACP reference for $ACTUAL_TAG"
node "$SRC_DIR/documentation/scripts/generate-acp-docs.js" \
  "$SRC_DIR/crates/goose/acp-schema.json" \
  "$SRC_DIR/crates/goose/acp-meta.json" \
  "$SRC_DIR/documentation/docs/gdk/acp/reference.md" \
  "$ACTUAL_TAG"

# ---------------------------------------------------------------------------
# 3) Build the documentation site
#
# `npm run build` runs generate-docs-map.js (writes static/goose-docs-map.md,
# copied to the build root) and then docusaurus build, whose markdown-export
# plugin writes build/docs/**. The build root is therefore a complete docs root.
# ---------------------------------------------------------------------------
echo "==> Installing documentation dependencies"
npm --prefix "$SRC_DIR/documentation" ci --no-audit --no-fund

echo "==> Building documentation site"
npm --prefix "$SRC_DIR/documentation" run build

BUILD_DIR="$SRC_DIR/documentation/build"

echo "==> Verifying docs root contract"
"$SRC_DIR/documentation/scripts/verify-build.sh" "$BUILD_DIR" 2>/dev/null || {
  echo "Error: goose-docs-map.md missing from build output" >&2
  exit 1
}

[ -d "$BUILD_DIR/docs" ] || { echo "Error: build/docs missing" >&2; exit 1; }

# Every path listed in the map must resolve inside the build root. The skill
# reads only paths named in the map, so a mismatch here means silent 404s.
MAP="$BUILD_DIR/goose-docs-map.md"
ENTRY_COUNT=0
MISSING=0
while read -r rel; do
  [ -n "$rel" ] || continue
  ENTRY_COUNT=$((ENTRY_COUNT + 1))
  if [ ! -f "$BUILD_DIR/$rel" ]; then
    echo "  missing: $rel" >&2
    MISSING=$((MISSING + 1))
  fi
done <<EOF
$(grep -oE '\(docs/[^)]+\.md\)' "$MAP" | tr -d '()' | sort -u)
EOF

if [ "$MISSING" -gt 0 ]; then
  echo "Error: $MISSING map entries missing from build output" >&2
  exit 1
fi
echo "    map entries: $ENTRY_COUNT (all present)"

# ---------------------------------------------------------------------------
# 4) Package
# ---------------------------------------------------------------------------
BUNDLE_NAME="goose-docs-$BARE_VERSION.tar.gz"
BUNDLE_PATH="$OUT_DIR/$BUNDLE_NAME"

echo "==> Packaging $BUNDLE_NAME"
tar czf "$BUNDLE_PATH" -C "$BUILD_DIR" .

BUNDLE_SHA="$(sha256_of "$BUNDLE_PATH")"
MAP_SHA="$(sha256_of "$MAP")"
BUNDLE_BYTES="$(wc -c < "$BUNDLE_PATH" | tr -d ' ')"

jq -n \
  --arg goose_version "$BARE_VERSION" \
  --arg tag "$ACTUAL_TAG" \
  --arg commit "$COMMIT" \
  --arg generated_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg bundle "$BUNDLE_NAME" \
  --arg sha256 "$BUNDLE_SHA" \
  --arg map_sha256 "$MAP_SHA" \
  --argjson bytes "$BUNDLE_BYTES" \
  --argjson entries "$ENTRY_COUNT" \
  '{
    goose_version: $goose_version,
    tag: $tag,
    commit: $commit,
    generated_at: $generated_at,
    variant: "site",
    bundle: $bundle,
    bytes: $bytes,
    sha256: $sha256,
    map_sha256: $map_sha256,
    entries: $entries
  }' > "$BUNDLE_PATH.manifest.json"

echo
echo "==> Done"
echo "    bundle:   $BUNDLE_PATH ($(echo "$BUNDLE_BYTES" | awk '{printf "%.1f MB", $1/1048576}'))"
echo "    manifest: $BUNDLE_PATH.manifest.json"
echo "    sha256:   $BUNDLE_SHA"
