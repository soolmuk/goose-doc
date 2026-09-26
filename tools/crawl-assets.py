"""Crawl the running server and check that every referenced asset resolves.

Resolves relative refs against the page's own path, exactly as a browser does,
so a dropped hero image or a missing script shows up as a failure.
"""
import os
import re
import sys
import urllib.error
import urllib.request
from collections import deque

BASE = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:10820"
MAX_PAGES = int(sys.argv[2]) if len(sys.argv) > 2 else 300

ASSET = re.compile(
    r'(?:src|href|srcset|content|data-src)="([^"]+?\.(?:png|jpe?g|gif|webp|svg|ico|css|js|mjs|woff2?|mp4|webm|mov|json|xml|txt))"'
)
LINK = re.compile(r'href="(/[^"#]*)"')
SKIP = (".css", ".js", ".png", ".jpg", ".jpeg", ".svg", ".ico", ".xml",
        ".json", ".zip", ".txt", ".webp", ".gif", ".woff", ".woff2")

# Sections the trimmed bundle drops on purpose. Their 404s are expected, so they
# are reported but do not fail the run: without this the check could never pass
# on the bundle that actually ships.
IGNORED_PREFIXES = ("/blog", "/community", "/extensions", "/recipes",
                    "/deeplink-generator")


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "goose-doc-crawl"})
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            return response.status, response.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as error:
        return error.code, ""
    except Exception as error:  # noqa: BLE001
        return 0, str(error)


def resolve(ref, page_path):
    """Turn a reference into an absolute path the browser would request.

    Paths are joined POSIX-style: this runs on Windows too, where os.path.join
    would produce backslashes that are not valid in a URL.
    """
    ref = ref.split("#")[0].split("?")[0]
    if not ref:
        return None
    if ref.startswith("//"):
        return None
    if ref.startswith("http://") or ref.startswith("https://") or ref.startswith("data:"):
        return ("external", ref)
    if ref.startswith("/"):
        return ("local", ref)
    base_dir = page_path.rsplit("/", 1)[0] if "/" in page_path else ""
    parts = [p for p in (base_dir + "/" + ref).split("/") if p not in ("", ".")]
    resolved = []
    for part in parts:
        if part == "..":
            if resolved:
                resolved.pop()
        else:
            resolved.append(part)
    return ("local", "/" + "/".join(resolved))


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

    for ref in sorted(set(ASSET.findall(body))):
        resolved = resolve(ref, path)
        if not resolved:
            continue
        kind, target = resolved
        if kind == "external":
            continue
        if target in seen_assets:
            continue
        seen_assets.add(target)
        code, _ = fetch(BASE + target)
        if code != 200:
            bad_assets.append((code, ref, target, path))

    for link in sorted(set(LINK.findall(body))):
        if link in seen_pages or link.endswith(SKIP) or "/assets/" in link:
            continue
        queue.append(link)

expected_pages = [entry for entry in bad_pages
                   if entry[1].startswith(IGNORED_PREFIXES)]
expected_assets = [entry for entry in bad_assets
                   if entry[2].startswith(IGNORED_PREFIXES)]
unexpected_pages = [entry for entry in bad_pages
                    if not entry[1].startswith(IGNORED_PREFIXES)]
unexpected_assets = [entry for entry in bad_assets
                     if not entry[2].startswith(IGNORED_PREFIXES)]

print(f"pages checked:  {checked} (broken: {len(bad_pages)}, "
      f"{len(expected_pages)} trimmed on purpose)")
print(f"assets checked: {len(seen_assets)} (broken: {len(bad_assets)}, "
      f"{len(expected_assets)} trimmed on purpose)")
for status, path in expected_pages[:5]:
    print(f"  page {status} (trimmed)  {path}")
for status, ref, target, page in expected_assets[:5]:
    print(f"  asset {status} (trimmed)  {ref}")
for status, path in unexpected_pages[:15]:
    print(f"  PAGE  {status}  {path}")
for status, ref, target, page in unexpected_assets[:25]:
    print(f"  ASSET {status}  {ref}  -> {target}   (on {page})")

if unexpected_pages or unexpected_assets:
    print()
    print(f"FAILED: {len(unexpected_pages)} page(s) and "
          f"{len(unexpected_assets)} asset(s) that should have been served")
    sys.exit(1)

print()
print("OK: every page and asset that should be served resolves")
