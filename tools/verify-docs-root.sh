#!/usr/bin/env bash
#
# Verify that a bundle is a valid goose docs root and that goose-doc-guide can
# read it offline. Run after build-docs-bundle.sh (P1 smoke check).
#
# Usage: verify-docs-root.sh <docs-root-dir> | --bundle <tar.gz> [--offline]
#
#   --offline   After structural checks, run `goose run` with GOOSE_DOCS_ROOT
#               pointing at the extracted bundle to prove the skill reads it.
#
set -euo pipefail

MODE="dir"
TARGET=""
OFFLINE=0
GOOSE_BIN="${GOOSE_BIN:-goose}"

while [ $# -gt 0 ]; do
  case "$1" in
    --bundle) MODE="bundle"; TARGET="$2"; shift 2 ;;
    --offline) OFFLINE=1; shift ;;
    -h|--help)
      sed -n '2,10p' "$0"
      exit 0
      ;;
    *) TARGET="$1"; shift ;;
  esac
done

if [ -z "$TARGET" ]; then
  echo "Usage: verify-docs-root.sh <docs-root-dir> | --bundle <tar.gz> [--offline]" >&2
  exit 2
fi

CLEANUP=""
if [ "$MODE" = "bundle" ]; then
  [ -f "$TARGET" ] || { echo "Error: bundle not found: $TARGET" >&2; exit 1; }
  EXTRACT_DIR="$(mktemp -d)"
  CLEANUP="$EXTRACT_DIR"
  echo "==> Extracting $TARGET"
  tar xzf "$TARGET" -C "$EXTRACT_DIR"
  ROOT="$EXTRACT_DIR"
else
  [ -d "$TARGET" ] || { echo "Error: not a directory: $TARGET" >&2; exit 1; }
  ROOT="$TARGET"
fi

if [ -n "$CLEANUP" ]; then
  trap 'rm -rf "$CLEANUP"' EXIT
fi

FAIL=0
check() {
  if [ "$2" = "1" ]; then
    echo "  ok   $1"
  else
    echo "  FAIL $1"
    FAIL=$((FAIL + 1))
  fi
}

echo "==> Docs root contract"
[ -f "$ROOT/goose-docs-map.md" ] && check "goose-docs-map.md present" 1 || check "goose-docs-map.md present" 0
[ -d "$ROOT/docs" ] && check "docs/ present" 1 || check "docs/ present" 0

if [ -f "$ROOT/goose-docs-map.md" ]; then
  MISSING=0
  TOTAL=0
  while read -r rel; do
    [ -n "$rel" ] || continue
    TOTAL=$((TOTAL + 1))
    [ -f "$ROOT/$rel" ] || { echo "       missing: $rel"; MISSING=$((MISSING + 1)); }
  done <<EOF
$(grep -oE '\(docs/[^)]+\.md\)' "$ROOT/goose-docs-map.md" | tr -d '()' | sort -u)
EOF
  [ "$TOTAL" -gt 0 ] && check "map lists $TOTAL entries" 1 || check "map lists entries" 0
  [ "$MISSING" = "0" ] && check "all map entries resolve" 1 || check "all map entries resolve ($MISSING missing)" 0
fi

if [ "$OFFLINE" = "1" ]; then
  echo "==> Offline skill check (GOOSE_DOCS_ROOT=$ROOT)"
  command -v "$GOOSE_BIN" >/dev/null 2>&1 || { echo "Error: goose binary not found: $GOOSE_BIN" >&2; exit 1; }

  PROMPT="Use the goose-doc-guide skill. Then answer two things in one short line: (1) the exact docs root path shown in the skill, (2) the exact heading line for the Offline / Air-gapped Docs page in the doc map. Use file tools only; do not use the network."

  OUT="$(GOOSE_DOCS_ROOT="$ROOT" "$GOOSE_BIN" run -t "$PROMPT" 2>&1 || true)"

  case "$OUT" in
    *"$ROOT"*) check "skill resolved docs root to the local bundle" 1 ;;
    *) check "skill resolved docs root to the local bundle" 0 ;;
  esac
  case "$OUT" in
    *"Offline / Air-gapped Docs"*) check "skill read the doc map offline" 1 ;;
    *) check "skill read the doc map offline" 0 ;;
  esac
fi

if [ "$FAIL" -gt 0 ]; then
  echo
  echo "==> FAILED ($FAIL checks)"
  exit 1
fi

echo
echo "==> PASSED"
