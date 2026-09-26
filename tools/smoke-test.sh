#!/usr/bin/env bash
#
# Smoke test for a built goose-doc binary: start it headless on the fixture docs
# root and check the HTTP contract the goose-doc-guide skill depends on.
#
# Portable across the three supported platforms, including Git Bash on Windows,
# where process control and signals differ from Unix.
#
# Usage: smoke-test.sh [path-to-binary]
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${1:-$ROOT/target/release/goose-doc}"
FIXTURE="$ROOT/fixtures/docs-root"

if [ -x "$BIN" ]; then
  :
elif [ -x "$BIN.exe" ]; then
  BIN="$BIN.exe"
else
  echo "Error: binary not found or not executable: $BIN" >&2
  exit 1
fi

LOG="$(mktemp)"
SERVER_PID=""

cleanup() {
  if [ -n "$SERVER_PID" ]; then
    stop_server
  fi
  rm -f "$LOG"
}
trap cleanup EXIT

stop_server() {
  [ -n "$SERVER_PID" ] || return 0
  if kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    # Give the process a moment to release the port before escalating.
    j=0
    while [ "$j" -lt 20 ]; do
      j=$((j + 1))
      kill -0 "$SERVER_PID" 2>/dev/null || break
      sleep 0.1
    done
    if kill -0 "$SERVER_PID" 2>/dev/null; then
      kill -9 "$SERVER_PID" 2>/dev/null || true
    fi
  fi
  { wait "$SERVER_PID"; } 2>/dev/null || true
  SERVER_PID=""
}

fail() {
  echo "  FAIL $1"
  [ -n "${2:-}" ] && echo "       $2"
  exit 1
}

pass() {
  echo "  ok   $1"
}

# Git Bash has no curl; use the Windows binary when present.
curl_available() {
  command -v curl >/dev/null 2>&1 && return 0
  [ -n "${SYSTEMROOT:-}" ] && [ -x "$SYSTEMROOT/System32/curl.exe" ] && return 0
  return 1
}

if ! curl_available; then
  echo "Error: curl is required" >&2
  exit 1
fi

code() {
  curl -s -o /dev/null -w '%{http_code}' "$BASE$1"
}

echo "==> Starting $BIN"
# The default bind is every interface, which is what a server deployment uses,
# so the smoke test exercises that path rather than forcing loopback.
"$BIN" --headless --docs-dir "$FIXTURE" --port 0 > "$LOG" 2>&1 &
SERVER_PID=$!
# Own the child explicitly: without this the shell prints a "Terminated" job
# notice on shutdown, which looks like a failure in CI logs.
disown "$SERVER_PID" 2>/dev/null || true

ADDR=""
i=0
while [ "$i" -lt 75 ]; do
  i=$((i + 1))
  ADDR="$(sed -n 's|.*Serving goose docs at http://\([^ ]*\).*|\1|p' "$LOG" | head -1)"
  [ -n "$ADDR" ] && break
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    echo "Error: server exited during startup" >&2
    cat "$LOG" >&2
    exit 1
  fi
  sleep 0.2
done

if [ -z "$ADDR" ]; then
  echo "Error: server did not report an address" >&2
  cat "$LOG" >&2
  exit 1
fi

# A wildcard bind is not dialable, so the advertised address must be concrete.
case "$ADDR" in
  0.0.0.0*|*"0.0.0.0"*) fail "advertised address is a wildcard" "$ADDR" ;;
  *) pass "advertised a dialable address ($ADDR)" ;;
esac

BASE="http://$ADDR"
echo "==> Listening on $BASE"

echo "==> HTTP contract"
[ "$(code /healthz)" = "200" ] && pass "/healthz" || fail "/healthz"

[ "$(code /goose-docs-map.md)" = "200" ] || fail "/goose-docs-map.md"
pass "/goose-docs-map.md"

CT="$(curl -s -o /dev/null -w '%{content_type}' "$BASE/goose-docs-map.md")"
case "$CT" in
  text/plain*) pass "map served as plain text" ;;
  *) fail "map content type" "got: $CT" ;;
esac

[ "$(code /docs/guides/offline-docs.md)" = "200" ] || fail "/docs/guides/offline-docs.md"
pass "/docs/guides/offline-docs.md"

BODY="$(curl -s "$BASE/docs/guides/offline-docs.md")"
case "$BODY" in
  *"docs root"*) pass "page body served" ;;
  *) fail "page body" ;;
esac

[ "$(code /index.html)" = "200" ] || fail "/index.html"
pass "/index.html"

# A missing page must stay a 404. A rendered HTML shell here would mean an SPA
# fallback is in place, which hides broken paths from the skill.
CODE="$(code /docs/guides/does-not-exist.md)"
[ "$CODE" = "404" ] && pass "missing page is 404" || fail "missing page is 404" "got: $CODE"

BODY404="$(curl -s "$BASE/docs/guides/does-not-exist.md")"
case "$BODY404" in
  *"<html"*) fail "404 has no HTML fallback" ;;
  *) pass "404 has no HTML fallback" ;;
esac

echo "==> Every map entry resolves"
MISSING=0
COUNT=0
while read -r rel; do
  [ -n "$rel" ] || continue
  COUNT=$((COUNT + 1))
  [ "$(code "/$rel")" = "200" ] || { echo "       missing: $rel"; MISSING=$((MISSING + 1)); }
done <<EOF
$(grep -oE '\(docs/[^)]+\.md\)' "$FIXTURE/goose-docs-map.md" | tr -d '()' | sort -u)
EOF
[ "$MISSING" = "0" ] && pass "$COUNT map entries served" || fail "map entries" "$MISSING missing"

echo "==> Traversal is refused"
for attack in "/../Cargo.toml" "/docs/../../Cargo.toml"; do
  CODE="$(curl -s -o /dev/null -w '%{http_code}' --path-as-is "$BASE$attack")"
  case "$CODE" in
    403|404) pass "refused $attack" ;;
    *) fail "refused $attack" "got: $CODE" ;;
  esac
done

echo "==> Shutdown"
stop_server
if curl -s -o /dev/null --max-time 3 "$BASE/healthz" 2>/dev/null; then
  fail "port still serving after stop"
fi
pass "stopped and released the port"

echo
echo "==> PASSED"
