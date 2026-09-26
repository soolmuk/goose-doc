#!/usr/bin/env python3
"""Build a browsable goose docs bundle from the documentation site build.

The published site bundle is ~344 MB, almost all of it blog imagery and demo
videos that the documentation never loads. This keeps the site itself — the HTML
pages, the CSS/JS/fonts, and the markdown the `goose-doc-guide` skill reads —
plus every asset those pages reference, and drops the rest. The result is small
enough to embed in a single executable while still looking like the published
site.

It operates on the output of `documentation/`'s `npm run build`, which is a
complete docs root: `goose-docs-map.md`, `docs/**` (both `.md` and the rendered
`index.html`), and `assets/**`.

Usage:
    tools/build-site-lite.py <site-bundle.tar.gz> --out <file.tar.gz>
    tools/build-site-lite.py <site-build-dir> --dir --out <file.tar.gz>
"""
import argparse
import os
import re
import shutil
import sys
import tarfile

# Directories that are part of the published site but that the documentation
# never reads: the blog, the versioned copies, and the demo apps.
SKIP_TOP = {
    "blog", "v1", "extensions", "community", "grants", "recipes",
    "deeplink-generator", "extension", "oauth",
}

# Everything a page needs to render, plus the markdown the skill reads.
ALWAYS_EXT = {
    ".html", ".css", ".js", ".mjs", ".woff", ".woff2", ".ico", ".svg",
    ".json", ".xml", ".txt", ".md", ".map",
}

# Assets a page actually points at, of any type. Two forms occur in the built
# site: absolute ("/img/x.png", how Docusaurus emits most links) and relative
# ("img/x.png", used by the home page's hero). Both must be captured, or the
# file is dropped from the bundle and 404s offline.
ABS_REF = re.compile(r'(?:src|href|srcset|content|data-src)="(/[^"]+?)"')
REL_REF = re.compile(
    r'(?:src|href|srcset|content|data-src)="(?!/|#|https?:|mailto:|data:)([^"]+?)"'
)

# Images are downscaled to this budget; the site's decoration does not need more.
MAX_IMAGE_BYTES = 150 * 1024
MAX_WIDTH = 1200

# The stylesheet loads the brand font from a CDN. Offline that request fails and
# the page falls back to a system font, so the fonts are fetched once and served
# from the bundle with the stylesheet pointing at them.
FONT_URL = re.compile(r'url\((https?://[^)]+?\.(?:woff2?|ttf|otf))\)')
FONT_DIR = "assets/fonts"


def shrink(path):
    """Downscale an oversized raster image in place, when Pillow is available."""
    ext = os.path.splitext(path)[1].lower()
    if ext not in {".png", ".jpg", ".jpeg", ".webp"}:
        return
    oversized = os.path.getsize(path) > MAX_IMAGE_BYTES
    try:
        from PIL import Image
    except ImportError:
        if oversized:
            print(f"    warning: Pillow missing, keeping {os.path.basename(path)} as is")
        return

    try:
        with Image.open(path) as image:
            image.load()
            ratio = MAX_WIDTH / image.width if image.width > MAX_WIDTH else 1.0
            if not oversized and ratio == 1.0:
                return
            if ratio < 1.0:
                image = image.resize(
                    (MAX_WIDTH, max(1, int(image.height * ratio))), Image.LANCZOS
                )
            if ext in {".jpg", ".jpeg"}:
                image.convert("RGB").save(path, quality=80, optimize=True)
            else:
                if image.mode not in ("RGB", "RGBA"):
                    image = image.convert("RGBA")
                image.save(path, optimize=True)
    except Exception as error:  # noqa: BLE001 - a bad image must not stop the build
        print(f"    warning: could not shrink {os.path.basename(path)}: {error}")


def kept(rel_dir):
    return rel_dir.split("/")[0] not in SKIP_TOP


def referenced_assets(root):
    """Every asset path a kept HTML page points at.

    Relative references are resolved against the page's own directory, which is
    how a browser resolves them, so the bundle keeps the file at the path the
    page will request.
    """
    refs = set()
    for base, dirs, files in os.walk(root):
        rel_dir = os.path.relpath(base, root).replace("\\", "/")
        if not kept(rel_dir):
            dirs[:] = []
            continue
        for name in files:
            if not name.endswith(".html"):
                continue
            with open(os.path.join(base, name), encoding="utf-8", errors="replace") as fh:
                text = fh.read()
            for ref in ABS_REF.findall(text):
                refs.add(ref.lstrip("/"))
            for ref in REL_REF.findall(text):
                joined = ref if rel_dir == "." else f"{rel_dir}/{ref}"
                refs.add(os.path.normpath(joined).replace("\\", "/"))
    return refs


def copy_tree(root, dest, refs):
    count = 0
    total = 0
    for base, dirs, files in os.walk(root):
        rel_dir = os.path.relpath(base, root).replace("\\", "/")
        if not kept(rel_dir):
            dirs[:] = []
            continue
        for name in files:
            rel = name if rel_dir == "." else f"{rel_dir}/{name}"
            ext = os.path.splitext(name)[1].lower()
            if ext not in ALWAYS_EXT and rel not in refs:
                continue
            target = os.path.join(dest, rel.replace("/", os.sep))
            os.makedirs(os.path.dirname(target), exist_ok=True)
            shutil.copy2(os.path.join(base, name), target)
            shrink(target)
            count += 1
            total += os.path.getsize(target)
    return count, total


def localize_fonts(dest):
    """Download the stylesheet's remote fonts and point the CSS at them.

    Without this the brand font would only load online, and an offline reader
    would see the page in a fallback font. A failed download leaves the original
    URL in place, so the bundle still builds without network access.
    """
    css_root = os.path.join(dest, "assets", "css")
    if not os.path.isdir(css_root):
        return 0

    import urllib.request

    localized = 0
    for name in os.listdir(css_root):
        if not name.endswith(".css"):
            continue
        path = os.path.join(css_root, name)
        with open(path, encoding="utf-8", errors="replace") as fh:
            text = fh.read()

        def replace(match):
            nonlocal localized
            url = match.group(1)
            file_name = url.rsplit("/", 1)[-1]
            target = os.path.join(dest, FONT_DIR.replace("/", os.sep), file_name)
            if not os.path.isfile(target):
                os.makedirs(os.path.dirname(target), exist_ok=True)
                try:
                    with urllib.request.urlopen(url, timeout=30) as response:
                        data = response.read()
                except Exception as error:  # noqa: BLE001
                    print(f"    warning: could not fetch {file_name}: {error}")
                    return match.group(0)
                with open(target, "wb") as out:
                    out.write(data)
            localized += 1
            return f"url(/ {FONT_DIR}/{file_name})".replace("/ ", "/")

        rewritten = FONT_URL.sub(replace, text)
        if rewritten != text:
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(rewritten)

    if localized:
        print(f"    localized {localized} font references")
    return localized


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", help="site bundle .tar.gz, or a build directory with --dir")
    parser.add_argument("--dir", action="store_true", help="treat source as an extracted directory")
    parser.add_argument("--out", required=True, help="output .tar.gz")
    parser.add_argument("--work-dir", help="scratch directory (default: a temp dir)")
    args = parser.parse_args()

    work = args.work_dir or os.path.join(
        os.environ.get("TEMP", "/tmp"), "goose-site-lite"
    )
    src = os.path.join(work, "full")
    dest = os.path.join(work, "lite")

    for stale in (src, dest):
        if os.path.isdir(stale):
            shutil.rmtree(stale)
    os.makedirs(src, exist_ok=True)

    if args.dir:
        src = os.path.abspath(args.source)
        if not os.path.isdir(src):
            print(f"Error: not a directory: {src}", file=sys.stderr)
            return 1
    else:
        if not os.path.isfile(args.source):
            print(f"Error: not a file: {args.source}", file=sys.stderr)
            return 1
        print(f"==> Extracting {args.source}")
        with tarfile.open(args.source) as tar:
            # Python 3.12+ warns without an explicit filter; older versions do
            # not accept the argument at all.
            try:
                tar.extractall(src, filter="data")
            except TypeError:
                tar.extractall(src)

    if not os.path.isfile(os.path.join(src, "goose-docs-map.md")):
        print(f"Error: {src} has no goose-docs-map.md, so it is not a docs root",
              file=sys.stderr)
        return 1

    print("==> Selecting the pages and the assets they use")
    refs = referenced_assets(src)
    count, total = copy_tree(src, dest, refs)
    print(f"    {count} files, {total / 1048576:.1f} MB")

    print("==> Localizing the brand font")
    localize_fonts(dest)

    # The map must resolve completely, or the skill silently misses pages.
    with open(os.path.join(src, "goose-docs-map.md"), encoding="utf-8") as fh:
        entries = sorted({m.strip("()") for m in re.findall(r"\(docs/[^)]+\.md\)", fh.read())})
    missing = [
        entry for entry in entries
        if not os.path.isfile(os.path.join(dest, entry.replace("/", os.sep)))
    ]
    if missing:
        print(f"Error: {len(missing)} of {len(entries)} map entries missing, "
              f"e.g. {missing[0]}", file=sys.stderr)
        return 1
    print(f"    {len(entries)} map entries, all present")

    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    print(f"==> Packaging {args.out}")
    with tarfile.open(args.out, "w:gz", compresslevel=9) as tar:
        tar.add(dest, arcname=".")

    size = os.path.getsize(args.out)
    print(f"    {size / 1048576:.1f} MB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
