#!/usr/bin/env python3
"""Crawl a served goose docs site and report broken pages and assets.

Used to confirm that the docs site a real browser sees is intact when served by
goose-doc, rather than only that the map's page paths return 200.
"""
import html
import re
import sys
import urllib.error
import urllib.request
from collections import deque

BASE = sys.argv[1] if len(sys.argv) > 1 else "http://192.168.45.42:10650"
MAX_PAGES = int(sys.argv[2]) if len(sys.argv) > 2 else 400

ASSET_RE = re.compile(
    r'(?:href|src)="(/[^"]*\.(?:css|js|mjs|png|jpe?g|svg|webp|gif|ico|woff2?|ttf|mp4|webm|json|xml))"'
)
LINK_RE = re.compile(r'href="(/[^"#]*)"')
SKIP_SUFFIX = (".css", ".js", ".png", ".jpg", ".svg", ".ico", ".xml", ".json", ".zip")


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "goose-doc-crawl"})
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            return response.status, response.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as error:
        return error.code, ""
    except Exception as error:  # noqa: BLE001 - report anything as a failure
        return 0, str(error)


def main():
    seen_pages = set()
    seen_assets = set()
    queue = deque(["/"])
    bad_pages = []
    bad_assets = []
    checked = 0

    while queue and checked < MAX_PAGES:
        path = queue.popleft()
        if path in seen_pages:
            continue
        seen_pages.add(path)

        status, body = fetch(BASE + path)
        checked += 1
        if status != 200:
            bad_pages.append((status, path))
            continue

        # Decode HTML entities first: a literal "&" in a URL becomes "&amp;"
        # in the markup, and requesting the undecoded form gives a false 404.
        decoded = html.unescape(body)

        for asset in sorted(set(ASSET_RE.findall(decoded))):
            if asset in seen_assets:
                continue
            seen_assets.add(asset)
            asset_status, _ = fetch(BASE + asset)
            if asset_status != 200:
                bad_assets.append((asset_status, asset))

        for link in sorted(set(LINK_RE.findall(decoded))):
            if link in seen_pages or link.endswith(SKIP_SUFFIX) or "/assets/" in link:
                continue
            queue.append(link)

    print(f"pages checked:  {checked} (broken: {len(bad_pages)})")
    print(f"assets checked: {len(seen_assets)} (broken: {len(bad_assets)})")
    for status, path in bad_pages[:20]:
        print(f"  PAGE  {status}  {path}")
    for status, asset in bad_assets[:20]:
        print(f"  ASSET {status}  {asset}")

    return 1 if (bad_pages or bad_assets) else 0


if __name__ == "__main__":
    sys.exit(main())
