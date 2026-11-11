#!/usr/bin/env python3
"""
make_wiki.py - Dynamic Catalog & Navigation Generator for Kodiak ProjectsWiki.

Scans the configured projects directory, extracts metadata, builds projects.json,
and generates mkdocs.yml navigation without personal hardcoded paths or taxonomies.
"""
import datetime
import json
import os
import re
from typing import Dict, Any, List

from wiki_config import load_wiki_config, load_overrides

WIKI_DIR = os.path.dirname(os.path.abspath(__file__))
WIKI_DOCS_DIR = os.path.join(WIKI_DIR, "docs")
os.makedirs(WIKI_DOCS_DIR, exist_ok=True)

VALID_EXTENSIONS = (
    ".py", ".rs", ".c", ".cpp", ".h", ".sh", ".js", ".ts", ".ino", ".md", ".txt", ".json", ".yaml", ".yml", ".toml"
)

SKIP_DOCS = {
    "index.md",
}

TAG_KEYWORDS = [
    ("ESP32", r"\besp32\b|\besp-32\b"),
    ("RP2040 / Pico", r"\brp2040\b|\bpico\b"),
    ("Arduino", r"\barduino\b|\b\.ino\b"),
    ("Raspberry Pi", r"\braspberry pi\b|\brpi\b"),
    ("Python", r"\bpython\b|\bpyqt\b|\bmicropython\b"),
    ("Rust", r"\brust\b|\bcargo\b"),
    ("Audio / MIDI", r"\baudio\b|\bmidi\b|\bsynth\b|\bsampler\b|\bi2s\b|\blooper\b"),
    ("E-Ink / Display", r"\be-ink\b|\beink\b|\boled\b|\btft\b|\blcd\b|\bneopixel\b"),
    ("Wi-Fi / IoT", r"\bwifi\b|\bwi-fi\b|\bmqtt\b|\brepeater\b|\bbluetooth\b|\bble\b"),
    ("Linux", r"\blinux\b|\bbash\b|\bsystemd\b"),
    ("Mechanical / CAD", r"\bengine\b|\bmotor\b|\bcad\b|\b3d\b|\bstl\b|\bstepper\b"),
]


def _write_file_if_changed(filepath: str, content: str) -> bool:
    """Writes content to filepath only if the file does not exist or content differs.
    Preserves mtime and prevents spurious MkDocs live-reload cascades when content is unchanged."""
    if os.path.exists(filepath):
        try:
            with open(filepath, "r", encoding="utf-8") as f:
                if f.read() == content:
                    return False
        except Exception:
            pass
    with open(filepath, "w", encoding="utf-8") as f:
        f.write(content)
    return True


def classify_project(slug_lower: str, text_lower: str, categories: Dict[str, List[str]]) -> str:
    """Classifies a project into the configured taxonomy based on keywords."""
    for category, keywords in categories.items():
        for pat in keywords:
            if not pat:
                continue
            if pat in slug_lower or re.search(r"\b" + re.escape(pat) + r"\b", text_lower):
                return category
    
    # Return first category with empty match list or default fallback
    for category, keywords in categories.items():
        if not keywords:
            return category
    return list(categories.keys())[0] if categories else "General & Prototypes"


def get_project_timestamps(project_path: str, doc_path: str) -> tuple[int, int, str, str]:
    mtimes = []
    ctimes = []

    if os.path.isdir(project_path):
        for root, dirs, files in os.walk(project_path):
            dirs[:] = [
                d
                for d in dirs
                if not d.startswith(".")
                and d not in {"node_modules", "__pycache__", "venv", ".venv", "build", "dist", "target", ".git"}
            ]
            for fname in files:
                if fname.startswith("."):
                    continue
                fpath = os.path.join(root, fname)
                try:
                    st = os.stat(fpath)
                    mtimes.append(st.st_mtime)
                    btime = getattr(st, "st_birthtime", None)
                    ctimes.append(btime if btime else st.st_mtime)
                except OSError:
                    pass
        if not mtimes:
            try:
                st = os.stat(project_path)
                mtimes.append(st.st_mtime)
                btime = getattr(st, "st_birthtime", None)
                ctimes.append(btime if btime else st.st_mtime)
            except OSError:
                pass

    if not mtimes and os.path.exists(doc_path):
        try:
            st = os.stat(doc_path)
            mtimes.append(st.st_mtime)
            btime = getattr(st, "st_birthtime", None)
            ctimes.append(btime if btime else st.st_mtime)
        except OSError:
            pass

    now_ts = datetime.datetime.now().timestamp()
    created_ts = int(min(ctimes)) if ctimes else int(now_ts)
    modified_ts = int(max(mtimes)) if mtimes else int(now_ts)
    if created_ts > modified_ts:
        created_ts = modified_ts

    created_date = datetime.date.fromtimestamp(created_ts).isoformat()
    modified_date = datetime.date.fromtimestamp(modified_ts).isoformat()
    return created_ts, modified_ts, created_date, modified_date


def extract_metadata(doc_path: str, project_name: str, doc_filename: str, projects_dir: str, categories: Dict[str, List[str]], overrides: dict = None) -> dict:
    try:
        with open(doc_path, "r", encoding="utf-8", errors="ignore") as f:
            text = f.read()
    except Exception:
        text = ""

    slug_base = doc_filename[:-3]
    ovr = {}
    if overrides:
        ovr = overrides.get(project_name) or overrides.get(slug_base) or {}

    lines = [line.strip() for line in text.splitlines() if line.strip()]
    title = ovr.get("title") or project_name
    if not ovr.get("title"):
        for line in lines:
            if line.startswith("# ") and len(line) > 2:
                raw_title = line[2:].strip()
                raw_title = re.sub(r"\s*Project Wiki\s*$", "", raw_title, flags=re.IGNORECASE)
                raw_title = re.sub(r"^Project(?: Wiki| Repository)?:\s*", "", raw_title, flags=re.IGNORECASE)
                if raw_title:
                    title = raw_title
                break

    summary = ovr.get("overview") or ""
    if not summary:
        for line in lines:
            if (
                line.startswith("#")
                or line.startswith("```")
                or line.startswith("-")
                or line.startswith("*")
                or line.startswith(">")
                or re.match(r"^\d+\.", line)
                or "table of contents" in line.lower()
            ):
                continue
            cleaned = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", line)
            cleaned = re.sub(r"[`*_]", "", cleaned).strip()
            if len(cleaned) > 35:
                summary = cleaned
                break

    if not summary:
        summary = f"Documentation, hardware notes, and architecture reference for {project_name}."
    if len(summary) > 180:
        summary = summary[:177].rsplit(" ", 1)[0] + "..."

    text_lower = (project_name + " " + title + " " + summary + " " + text[:4000]).lower()
    category = ovr.get("category") or classify_project(slug_base.lower(), text_lower, categories)

    tags = list(ovr.get("tags") or [])
    if not tags:
        for label, pattern in TAG_KEYWORDS:
            if re.search(pattern, text_lower):
                tags.append(label)
            if len(tags) >= 4:
                break
    if not tags:
        tags.append(category.split(" ")[0])

    project_full_path = os.path.join(projects_dir, project_name)
    created_ts, modified_ts, created_date, modified_date = get_project_timestamps(
        project_full_path, doc_path
    )

    return {
        "name": project_name,
        "title": title,
        "file": doc_filename,
        "url": f"{slug_base}/",
        "category": category,
        "summary": summary,
        "tags": tags,
        "created_ts": created_ts,
        "modified_ts": modified_ts,
        "created_date": created_date,
        "modified_date": modified_date,
    }


def main():
    cfg = load_wiki_config()
    projects_dir = cfg["projects_dir"]
    categories = cfg["categories"]
    overrides = load_overrides(cfg)

    project_files = []
    seen_docs = set()

    # 1. Collect all existing documentation pages in docs/
    for fname in sorted(os.listdir(WIKI_DOCS_DIR)):
        if fname.endswith(".md") and fname not in SKIP_DOCS:
            proj_name = fname[:-3].replace("_", " ")
            project_files.append((proj_name, fname))
            seen_docs.add(fname)

    # 2. Check projects_dir for any new projects that don't have docs yet
    if os.path.isdir(projects_dir):
        for project_name in sorted(os.listdir(projects_dir)):
            project_path = os.path.join(projects_dir, project_name)
            if not os.path.isdir(project_path) or project_name.startswith("."):
                continue

            clean_name = project_name.replace(" ", "_")
            doc_filename = f"{clean_name}.md"
            doc_path = os.path.join(WIKI_DOCS_DIR, doc_filename)

            if doc_filename not in seen_docs:
                project_files.append((project_name, doc_filename))
                seen_docs.add(doc_filename)

            if os.path.exists(doc_path):
                continue

            print(f":: [ProjectsWiki] Analyzing new project: {project_name}...")
            code_snippet = ""
            all_files = []
            for root, dirs, files in os.walk(project_path):
                dirs[:] = [d for d in sorted(dirs) if not d.startswith(".") and d not in {"node_modules", "__pycache__", "venv", ".venv", "build", "dist", "target", ".git"}]
                rel_root = os.path.relpath(root, project_path)
                for file in sorted(files):
                    if file.startswith("."):
                        continue
                    rel_path = file if rel_root == "." else os.path.join(rel_root, file)
                    all_files.append(rel_path)
                    if file.lower().endswith(VALID_EXTENSIONS) and len(code_snippet) < 6000:
                        file_path = os.path.join(root, file)
                        try:
                            with open(file_path, "r", encoding="utf-8", errors="ignore") as f:
                                code_snippet += f"\n--- File: {rel_path} ---\n" + f.read()[:1200]
                        except Exception:
                            pass

            if not code_snippet.strip():
                # Non-code directory (manuals, photos, CAD, PDFs)
                file_bullets = "\n".join(f"- `{p}`" for p in all_files[:60]) or "- *(Empty folder)*"
                doc_content = (
                    f"# {project_name}\n\n"
                    f"## Overview\n\n"
                    f"Reference documents, manuals, media, and project files stored in `{project_path}`.\n\n"
                    f"## Project Files ({len(all_files)})\n\n"
                    f"{file_bullets}\n"
                )
            else:
                file_list_str = "\n".join(all_files[:60])
                prompt = (
                    f"Summarize the project folder '{project_name}' in Markdown based STRICTLY on the actual files "
                    f"and excerpts below. Do NOT invent hardware modules, CPUs, sensors, or software that are not "
                    f"explicitly present in these files.\n\n"
                    f"Files in directory:\n{file_list_str}\n\n"
                    f"File excerpts:\n{code_snippet[:6000]}"
                )
                try:
                    import urllib.request
                    req = urllib.request.Request(
                        f"{cfg['ollama_url'].rstrip('/')}/api/chat",
                        data=json.dumps({
                            "model": cfg["ollama_model"],
                            "messages": [{"role": "user", "content": prompt}],
                            "stream": False
                        }).encode("utf-8"),
                        headers={"Content-Type": "application/json"}
                    )
                    with urllib.request.urlopen(req, timeout=30) as resp:
                        res_data = json.loads(resp.read().decode("utf-8"))
                        doc_content = res_data.get("message", {}).get("content", f"# {project_name}\n\nProject documentation and workspace notes.")
                except Exception:
                    doc_content = f"# {project_name}\n\nProject documentation and workspace notes."

            _write_file_if_changed(doc_path, doc_content)

    # Build catalog metadata
    catalog = []
    by_category = {}
    for proj_name, doc_filename in project_files:
        doc_path = os.path.join(WIKI_DOCS_DIR, doc_filename)
        if not os.path.exists(doc_path):
            continue
        meta = extract_metadata(doc_path, proj_name, doc_filename, projects_dir, categories, overrides)
        catalog.append(meta)
        by_category.setdefault(meta["category"], []).append(meta)

    catalog_json = json.dumps(catalog, indent=2, ensure_ascii=False) + "\n"
    _write_file_if_changed(os.path.join(WIKI_DOCS_DIR, "projects.json"), catalog_json)

    category_order = list(categories.keys())
    for cat in by_category.keys():
        if cat not in category_order:
            category_order.append(cat)

    nav_yaml = """site_name: Projects Wiki
site_description: Interactive hardware, electronics, embedded systems, and software documentation hub
theme:
  name: material
  logo: Bear-built-logo.png
  favicon: Bear-built-logo.png
  palette:
    scheme: slate
    primary: deep orange
    accent: deep orange
  features:
    - navigation.instant
    - navigation.instant.prefetch
    - navigation.instant.progress
    - navigation.tracking
    - navigation.tabs
    - navigation.sections
    - navigation.top
    - search.suggest
    - search.highlight
    - content.code.copy

extra_css:
  - extra.css

extra_javascript:
  - extra.js

plugins:
  - search

markdown_extensions:
  - admonition
  - attr_list
  - md_in_html
  - tables
  - toc:
      permalink: true
  - pymdownx.details
  - pymdownx.superfences
  - pymdownx.highlight:
      anchor_linenums: true

nav:
  - Explorer: index.md
"""
    for cat in category_order:
        items = by_category.get(cat, [])
        if not items:
            continue
        nav_yaml += f"  - '{cat}':\n"
        for item in sorted(items, key=lambda x: x["name"].lower()):
            safe_name = item["name"].replace("'", "''")
            nav_yaml += f"      - '{safe_name}': {item['file']}\n"

    _write_file_if_changed(os.path.join(WIKI_DIR, "mkdocs.yml"), nav_yaml)

    print(f":: [ProjectsWiki] Generated projects.json ({len(catalog)} projects) and mkdocs.yml successfully.")


if __name__ == "__main__":
    main()
