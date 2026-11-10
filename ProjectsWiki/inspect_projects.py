#!/usr/bin/env python3
"""
inspect_projects.py - Inspects local project directories to collect file trees,
extensions, sizes, and representative text snippets for indexing.
Writes output to projects_inventory.json (local/untracked).
"""
import json
import os
import sys

from wiki_config import load_wiki_config

WIKI_DIR = os.path.dirname(os.path.abspath(__file__))
OUT_PATH = os.path.join(WIKI_DIR, "projects_inventory.json")

TEXT_EXTS = {
    ".md", ".txt", ".py", ".rs", ".c", ".cpp", ".h", ".hpp", ".ino",
    ".sh", ".js", ".ts", ".html", ".css", ".json", ".yaml", ".yml",
    ".toml", ".ini", ".cfg", ".scad", ".gcode", ".csv",
}

SKIP_DIRS = {
    ".git", "node_modules", "__pycache__", ".venv", "venv", "build",
    ".pio", ".vscode", ".idea", "dist", "target", "site",
}


def scan_projects():
    cfg = load_wiki_config()
    projects_dir = cfg["projects_dir"]
    inventory = {}

    if not os.path.isdir(projects_dir):
        print(f":: [ProjectsWiki] Error: Projects directory not found: {projects_dir}")
        return

    print(f":: [ProjectsWiki] Scanning projects directory: {projects_dir}")
    for proj_name in sorted(os.listdir(projects_dir)):
        proj_path = os.path.join(projects_dir, proj_name)
        if not os.path.isdir(proj_path) or proj_name.startswith("."):
            continue

        files_list = []
        subdirs = set()
        ext_counts = {}
        snippets = []
        total_size = 0

        for root, dirs, files in os.walk(proj_path):
            dirs[:] = [d for d in sorted(dirs) if d not in SKIP_DIRS and not d.startswith(".")]
            rel_root = os.path.relpath(root, proj_path)
            if rel_root != ".":
                subdirs.add(rel_root)

            for fname in sorted(files):
                if fname.startswith("."):
                    continue
                fpath = os.path.join(root, fname)
                rel_path = fname if rel_root == "." else os.path.join(rel_root, fname)
                try:
                    sz = os.path.getsize(fpath)
                except OSError:
                    sz = 0
                total_size += sz
                ext = os.path.splitext(fname)[1].lower() or "(no ext)"
                ext_counts[ext] = ext_counts.get(ext, 0) + 1
                if len(files_list) < 120:
                    files_list.append({"path": rel_path, "size": sz, "ext": ext})

                # Capture real README or code snippets
                if ext in TEXT_EXTS and len(snippets) < 8 and sz > 0:
                    is_priority = any(
                        k in fname.lower()
                        for k in ("readme", "note", "info", "bom", "main", "config", "setup")
                    )
                    if is_priority or len(snippets) < 4:
                        try:
                            with open(fpath, "r", encoding="utf-8", errors="ignore") as f:
                                content = f.read(1500).strip()
                            if content:
                                snippets.append({"file": rel_path, "excerpt": content})
                        except Exception:
                            pass

        inventory[proj_name] = {
            "name": proj_name,
            "total_files": sum(ext_counts.values()),
            "total_size_bytes": total_size,
            "subdirs": sorted(subdirs)[:40],
            "ext_counts": ext_counts,
            "files": files_list,
            "snippets": snippets,
        }

    with open(OUT_PATH, "w", encoding="utf-8") as f:
        json.dump(inventory, f, indent=2)
    print(f":: [ProjectsWiki] Scanned {len(inventory)} projects into {OUT_PATH}")


if __name__ == "__main__":
    scan_projects()
