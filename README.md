# goose-doc

Host goose documentation from your own machine or server, so `goose-doc-guide`
reads it without internet access.

goose ships a built-in skill, `goose-doc-guide`, that answers goose-specific
questions from the official documentation. It reads the docs from
`GOOSE_DOCS_ROOT`, which may be a local path or an HTTP(S) URL, and defaults to
`https://goose-docs.ai`. This project builds the docs into a self-contained
folder and serves it, so you can point goose at it:

```
GOOSE_DOCS_ROOT=http://docs.internal:10650
```

## Status

| Phase | Scope | State |
|---|---|---|
| P0 | Decisions (panel, port, bundle, distribution, schedule) | done |
| **P1** | Docs bundle pipeline (`tools/`, `docs-bundle.yml`) | **done** |
| **P2** | **Headless server, docs resolution, tests** | **done** |
| **P3** | **egui panel, settings persistence** | **done** |
| **P4** | **Cross-OS build and smoke tests** | **done** |
| P5 | Release assets, service install | planned |

See [PLAN.md](PLAN.md) for the full plan.

## Docs root contract

Whatever `GOOSE_DOCS_ROOT` points at must look like this:

```
<docs-root>/
├── goose-docs-map.md     # index the skill reads first
└── docs/
    ├── getting-started/*.md
    └── guides/**/*.md
```

Every path listed in `goose-docs-map.md` must exist, relative to the root. The
skill reads only the paths named there, so a mismatch produces silent 404s.

## Building a bundle

The bundle is built from a goose release tag, using the same version as your
goose binary so the docs match the runtime.

```bash
./tools/build-docs-bundle.sh 1.52.0 --out-dir ./out
```

Output:

```
out/goose-docs-1.52.0.tar.gz
out/goose-docs-1.52.0.tar.gz.manifest.json
```

Requirements: `git`, `node` (20+), `npm`, `jq`, `tar`.

The script checks out the goose tag, builds `documentation/` with
`npm run build`, asserts the docs root contract, and writes a manifest with the
checksums.

> The ACP reference page (`docs/gdk/acp/reference.md`) is generated, not
> committed, and the site build fails without it. The script generates it from
> the checked-out tag, pinning the version rather than resolving the latest
> release, so the page always matches the bundle.

## Verifying a bundle

```bash
./tools/verify-docs-root.sh --bundle ./out/goose-docs-1.52.0.tar.gz --offline
```

Structural checks always run; `--offline` additionally runs `goose run` with
`GOOSE_DOCS_ROOT` pointed at the extracted bundle, proving the skill reads it
from disk with no network access.

## Automation

`.github/workflows/docs-bundle.yml` runs 6 times a day, builds a bundle from
the latest goose release, and publishes it as a release asset on this
repository under the tag `docs-v<version>`. Nothing is written to the goose
repository; it is only read.

## Using the bundle

Extract it anywhere and point goose at it:

```bash
tar xzf goose-docs-1.52.0.tar.gz -C /opt/goose-docs
export GOOSE_DOCS_ROOT=/opt/goose-docs
```

## Serving it

`goose-doc` serves a docs root over HTTP so other machines can use it. It
listens on every interface by default, which is what a server deployment wants.

```bash
# Serve a bundle version from the cache (see `goose-doc fetch` below)
./goose-doc --port 10650

# Or serve a docs root directly
./goose-doc --docs-dir /opt/goose-docs --port 10650
```

The server prints the URL to use; on the goose side of another machine:

```bash
export GOOSE_DOCS_ROOT=http://<server-ip>:10650
```

| Flag | Default | Purpose |
|---|---|---|
| `--bind` | `0.0.0.0` | Listen address. The default serves every interface |
| `--port` | `10650` | Listen port. `0` picks a free port |
| `--docs-dir` | | Serve this docs root instead of a cached bundle |
| `--docs-version` | | Serve a specific cached version |
| `--headless` | off | No panel (servers, containers) |
| `--open` | off | Open a browser once listening |
| `--local-only` | off | Bind `127.0.0.1` instead, so only this machine can connect |

Other commands:

```bash
# Download a bundle into the cache (verifies sha256 before extracting)
./goose-doc fetch 1.52.0

# Show what would be served and whether the docs root is valid
./goose-doc doctor --docs-dir /opt/goose-docs

# List the addresses this machine can serve on
./goose-doc addresses
```

The server deliberately does **not** render pages or fall back to an HTML
shell: a missing path returns 404, so a broken link surfaces instead of being
masked. Markdown is served as `text/plain`, which is what the skill reads.

### Behaviour worth knowing

- Paths that escape the docs root (`..`, absolute paths) are refused with 403.
- The default bind is every interface, so a server is reachable from other
  machines. The advertised URL substitutes the machine's LAN address for the
  wildcard, so the printed `GOOSE_DOCS_ROOT` is usable as-is.
- `--port 0` binds a free port and prints the real port.
- Stop with SIGTERM/SIGINT; the port is released.
- Running without `--headless` opens a panel window. If a window cannot be
  opened (a server with no display, for example) it reports why and serves
  headless instead of failing.
- Closing the panel window also stops the server, so no socket is left behind.

### Panel

Running `goose-doc` with no `--headless` opens a small window:

- **Bind address** dropdown, seeded with `0.0.0.0` and the machine's addresses
- **Port** field
- **Start** / **Stop** buttons
- Status while running: URL, listening address, reach, docs version and page
  count, uploads, uptime, request count
- A **Copy** button for the `GOOSE_DOCS_ROOT=...` line to paste on the client
- **Open** to view the docs in a browser

Settings are saved to the goose-doc config directory after a successful start,
so the panel reopens with what you last used. Only bind address, port, docs
root, and the browser preference are stored; no secrets.

The panel text is English: egui's default font has no CJK glyphs, so other
scripts would render as blank boxes.

## License

Documentation content is from [aaif-goose/goose](https://github.com/aaif-goose/goose)
and is licensed Apache-2.0. This tooling is Apache-2.0 as well.
