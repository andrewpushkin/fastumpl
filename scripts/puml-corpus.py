#!/usr/bin/env python3
"""Builds a corpus of large PlantUML files from public GitHub repositories.

    scripts/puml-corpus.py search    # find files, add them to the manifest
    scripts/puml-corpus.py fetch     # download every file in the manifest

The manifest (tests/puml-corpus/manifest.tsv) is the database: one line per
file with its repository, the commit it was found at, its path and size. It
pins each file to a commit, so the same corpus can be downloaded again at any
time. The files themselves are not committed (they belong to their authors);
`fetch` puts them in target/puml-corpus/<owner>/<repo>/<path>, keeping each
repository's folders so that `!include` of a relative path resolves.

`fetch` also follows `!include`, `!include_once`, `!include_many` and
`!includesub` of relative paths: each included file is fetched from the same
commit and added to the manifest with role `include`. Standard library
(`<C4/...>`) and web includes are not fetched.

`search` uses GitHub code search through the `gh` command (logged in); it
waits out the search rate limit (10 requests a minute). `fetch` needs no
login. Only the Python standard library is used.

Then check the corpus:

    PUML_CORPUS=target/puml-corpus scripts/dev.sh \
        cargo test -p umpl-draw --test puml_corpus -- --ignored --nocapture
"""

import concurrent.futures
import json
import os
import posixpath
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MANIFEST = os.path.join(ROOT, "tests", "puml-corpus", "manifest.tsv")
FILES = os.path.join(ROOT, "target", "puml-corpus")
HEADER = "repo\tcommit\tpath\tbytes\trole"

# Words that pick out class diagrams, searched in several size bands so that
# each query's cap of 1000 results reaches further.
TERMS = ["class", "interface", "extends", "<|--", "package", "enum", "abstract", "implements", "*--", "!include"]
SIZES = ["5121..7000", "7001..10000", "10001..15000", "15001..30000", ">30000"]
EXTENSIONS = ["puml", "plantuml", "iuml", "pu", "wsd"]
INCLUDE = re.compile(r"^\s*!(include|include_once|include_many|includesub)\s+(.+?)\s*$", re.I)


def load():
    rows = {}
    if os.path.exists(MANIFEST):
        with open(MANIFEST, encoding="utf-8") as f:
            for line in f.read().splitlines()[1:]:
                repo, commit, path, size, role = line.split("\t")
                rows[(repo, path)] = [repo, commit, path, size, role]
    return rows


def save(rows):
    os.makedirs(os.path.dirname(MANIFEST), exist_ok=True)
    tmp = MANIFEST + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        f.write(HEADER + "\n")
        for key in sorted(rows):
            f.write("\t".join(rows[key]) + "\n")
    os.replace(tmp, MANIFEST)


def gh_search(query, page):
    """One page of code search results, waiting out the rate limit."""
    while True:
        r = subprocess.run(
            ["gh", "api", "-X", "GET", "search/code", "-f", f"q={query}",
             "-f", "per_page=100", "-f", f"page={page}"],
            capture_output=True, text=True)
        if r.returncode == 0:
            return json.loads(r.stdout)
        err = r.stderr + r.stdout
        if "rate limit" in err.lower() or "403" in err or "429" in err:
            print("  rate limited; waiting 30 s", flush=True)
            time.sleep(30)
            continue
        if "422" in err or "Cannot access beyond" in err:
            return {"items": []}
        print(f"  search failed: {err.strip()[:200]}", flush=True)
        return {"items": []}


def search(pages):
    rows = load()
    before = len(rows)
    queries = [f'extension:{ext} size:{size} "{term}"'
               for term in TERMS for size in SIZES for ext in EXTENSIONS[:1]]
    queries += [f"extension:{ext} size:>5120 class" for ext in EXTENSIONS[1:]]
    for q in queries:
        for page in range(1, pages + 1):
            d = gh_search(q, page)
            items = d.get("items", [])
            new = 0
            for it in items:
                repo = it["repository"]["full_name"]
                m = re.search(r"/blob/([0-9a-f]{40})/", it["html_url"])
                if not m:
                    continue
                key = (repo, it["path"])
                if key not in rows:
                    rows[key] = [repo, m.group(1), it["path"], "?", "main"]
                    new += 1
            print(f"{q} p{page}: {len(items)} results, {new} new, {len(rows)} total", flush=True)
            save(rows)
            if len(items) < 100:
                break
            time.sleep(6.5)
        time.sleep(6.5)
    print(f"manifest: {len(rows)} files ({len(rows) - before} new)")


def local(repo, path):
    return os.path.join(FILES, repo, path)


def download(repo, commit, path):
    dest = local(repo, path)
    if os.path.exists(dest):
        return dest, None
    url = "https://raw.githubusercontent.com/{}/{}/{}".format(
        repo, commit, urllib.parse.quote(path))
    req = urllib.request.Request(url, headers={"User-Agent": "puml-corpus-fetch"})
    for attempt in range(4):
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                data = r.read()
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            with open(dest, "wb") as f:
                f.write(data)
            return dest, None
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return None, "404"
            time.sleep(2 ** attempt)
        except Exception as e:  # noqa: BLE001 - network errors of any kind
            time.sleep(2 ** attempt)
            err = str(e)
    return None, "failed"


def includes(dest, repo_path):
    """Repository paths of the relative files `dest` includes."""
    out = []
    try:
        text = open(dest, encoding="utf-8", errors="replace").read()
    except OSError:
        return out
    base = posixpath.dirname(repo_path)
    for line in text.splitlines():
        m = INCLUDE.match(line)
        if not m:
            continue
        target = m.group(2).strip().strip('"').replace("\\", "/")
        target = target.split("!")[0].strip()  # file!SUBPART or file!2
        if not target or target.startswith("<") or "://" in target or target.startswith("%"):
            continue
        p = posixpath.normpath(posixpath.join(base, target))
        if not p.startswith(".."):
            out.append(p)
    return out


def fetch():
    rows = load()
    todo = list(rows.values())
    done = set()
    failed = 0
    with concurrent.futures.ThreadPoolExecutor(16) as pool:
        while todo:
            futures = {pool.submit(download, r[0], r[1], r[2]): r for r in todo
                       if (r[0], r[2]) not in done}
            todo = []
            for fut in concurrent.futures.as_completed(futures):
                row = futures[fut]
                done.add((row[0], row[2]))
                dest, err = fut.result()
                if dest is None:
                    failed += 1
                    row[3] = err
                    continue
                row[3] = str(os.path.getsize(dest))
                for inc in includes(dest, row[2]):
                    key = (row[0], inc)
                    if key not in rows:
                        rows[key] = [row[0], row[1], inc, "?", "include"]
                        todo.append(rows[key])
            save(rows)
            print(f"fetched {len(done)} files, {failed} failed", flush=True)
    save(rows)


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in ("search", "fetch"):
        print(__doc__)
        sys.exit(2)
    if sys.argv[1] == "search":
        search(int(sys.argv[2]) if len(sys.argv) > 2 else 3)
    else:
        fetch()


if __name__ == "__main__":
    main()
