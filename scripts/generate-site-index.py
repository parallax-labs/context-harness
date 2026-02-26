#!/usr/bin/env python3
"""
Generate site/static/site-index.json for the search widget.
Reads site/content/*.md, extracts title/description from front matter,
and builds a list of { title, url, description } for site navigation search.
Run from repo root. Requires site/config.toml (base_url) and site/content/.
"""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CONTENT = ROOT / "site" / "content"
CONFIG = ROOT / "site" / "config.toml"
OUT = ROOT / "site" / "static" / "site-index.json"


def get_base_url():
    with open(CONFIG) as f:
        for line in f:
            m = re.match(r'base_url\s*=\s*["\']([^"\']+)["\']', line)
            if m:
                return m.group(1).rstrip("/")
    return "https://parallax-labs.github.io/context-harness"


def parse_front_matter(path):
    text = path.read_text(encoding="utf-8")
    title = path.stem.replace("-", " ").replace("_", " ").title()
    description = ""
    if text.startswith("+++"):
        end = text.index("+++", 3)
        fm = text[3:end]
        for line in fm.split("\n"):
            if line.strip().startswith("title"):
                m = re.search(r'title\s*=\s*["\']([^"\']+)["\']', line)
                if m:
                    title = m.group(1)
            if line.strip().startswith("description"):
                m = re.search(r'description\s*=\s*["\']([^"\']+)["\']', line)
                if m:
                    description = m.group(1)
    return title, description


def path_to_url(rel_path, base_url):
    """Convert content-relative path to full URL (Zola-style)."""
    parts = rel_path.replace("\\", "/").split("/")
    # _index.md -> section index (e.g. docs/_index -> /docs/)
    if parts[-1] == "_index.md":
        return base_url + "/" + "/".join(parts[:-1]) + "/"
    # page.md -> /section/.../page/
    stem = parts[-1].replace(".md", "")
    return base_url + "/" + "/".join(parts[:-1] + [stem]) + "/"


def main():
    base_url = get_base_url()
    entries = []

    # Static entries for top-level pages (no content file or special)
    entries.append({
        "title": "Context Harness",
        "url": base_url + "/",
        "description": "Local-first context engine for AI tools",
    })
    entries.append({
        "title": "Demo",
        "url": base_url + "/demo/",
        "description": "Search a pre-built knowledge base in your browser",
    })

    for path in sorted(CONTENT.rglob("*.md")):
        rel = path.relative_to(CONTENT)
        if rel.parts[0].startswith("_"):
            continue
        title, description = parse_front_matter(path)
        url = path_to_url(str(rel), base_url)
        entries.append({"title": title, "url": url, "description": description or ""})

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump(entries, f, indent=2)
    print(f"Wrote {len(entries)} entries to {OUT}")


if __name__ == "__main__":
    main()
