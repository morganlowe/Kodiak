#!/usr/bin/env python3
"""
rebuild_accurate_docs.py - Rebuilds and enriches project documentation pages
using real assets, media, PDFs, and code files indexed in projects_inventory.json.
Respects external overrides in ~/.config/kodiak/project_overrides.json.
"""
import json
import os
import re
import sys
import urllib.parse
from typing import Dict, Any

from wiki_config import load_wiki_config, load_overrides

WIKI_DIR = os.path.dirname(os.path.abspath(__file__))
DOCS_DIR = os.path.join(WIKI_DIR, "docs")
INV_PATH = os.path.join(WIKI_DIR, "projects_inventory.json")

# Document and media categories
DOC_EXTS = {
    ".pdf", ".txt", ".rtf", ".odt", ".doc", ".docx", ".epub",
    ".step", ".stp", ".dxf", ".dwg", ".kicad_sch", ".kicad_pcb", ".sch", ".brd", ".svg",
}
IMAGE_EXTS = {".jpg", ".jpeg", ".png", ".webp", ".gif", ".svg"}
VIDEO_EXTS = {".mp4", ".mov", ".webm"}
CODE_EXTS = {".py", ".c", ".cpp", ".h", ".hpp", ".ino", ".rs", ".js", ".jsx", ".ts", ".sh", ".kt"}


def fmt_size(num_bytes: int) -> str:
    if num_bytes <= 0:
        return "0 B"
    if num_bytes < 1024:
        return f"{num_bytes} B"
    if num_bytes < 1024 * 1024:
        return f"{num_bytes / 1024:.1f} KB"
    return f"{num_bytes / (1024 * 1024):.2f} MB"


def file_url(proj_name: str, rel_path: str, site_domain: str = "wiki.local", port_api: int = 8765) -> str:
    encoded_proj = urllib.parse.quote(proj_name)
    encoded_rel = "/".join(urllib.parse.quote(part) for part in rel_path.split("/"))
    return f"http://{site_domain}:{port_api}/files/{encoded_proj}/{encoded_rel}"


def build_assets_markdown(proj_name: str, inv_entry: dict, projects_dir: str, site_domain: str, port_api: int) -> str:
    files = inv_entry.get("files", [])
    total_files = inv_entry.get("total_files", 0)
    total_size = inv_entry.get("total_size_bytes", 0)
    ext_counts = inv_entry.get("ext_counts", {})
    snippets = inv_entry.get("snippets", [])
    project_full_path = os.path.join(projects_dir, proj_name)

    if total_files == 0:
        return f"\n## 📁 Directory Status\n\n*This project folder (`{project_full_path}`) is currently empty (0 files).*\n"

    md = []
    ext_summary = ", ".join(f"`{ext}` ({cnt})" for ext, cnt in sorted(ext_counts.items(), key=lambda x: -x[1])[:10])
    md.append(f"\n## 📁 Directory Summary\n")
    md.append(f"- **Location:** `{project_full_path}`")
    md.append(f"- **Total Files:** {total_files} ({fmt_size(total_size)})")
    if ext_summary:
        md.append(f"- **File Types:** {ext_summary}\n")

    # 1. Manuals, PDFs, CAD & Documents
    docs = [f for f in files if f["ext"] in DOC_EXTS]
    if docs:
        md.append("\n## 📄 Manuals, Documents & CAD Files\n")
        md.append("| File | Type | Size | Action |")
        md.append("| :--- | :--- | :--- | :--- |")
        for d in docs[:40]:
            url = file_url(proj_name, d["path"], site_domain, port_api)
            ext_label = d["ext"].lstrip(".").upper()
            md.append(
                f"| `{d['path']}` | **{ext_label}** | {fmt_size(d['size'])} | "
                f"[Open / Download ↗]({url}){{ target=\"_blank\" rel=\"noopener\" class=\"project-file-link\" }} |"
            )
        md.append("")

    # 2. Text / Note Snippets
    if snippets:
        md.append("\n## 📝 Notes & File Excerpts\n")
        for s in snippets[:5]:
            md.append(f"### `{s['file']}`\n")
            md.append("```text\n" + s["excerpt"].strip() + "\n```\n")

    # 3. Videos
    videos = [f for f in files if f["ext"] in VIDEO_EXTS and f["size"] > 0]
    if videos:
        md.append("\n## 🎬 Video Clips\n")
        for v in videos[:6]:
            url = file_url(proj_name, v["path"], site_domain, port_api)
            md.append(f"- **[{v['path']}]({url})** ({fmt_size(v['size'])})")
        md.append("")

    # 4. Photo Gallery
    images = [f for f in files if f["ext"] in IMAGE_EXTS]
    if images:
        non_empty_images = [img for img in images if img["size"] > 0]
        md.append(f"\n## 🖼️ Photo & Media Gallery ({len(images)} images)\n")
        if non_empty_images:
            md.append('<div class="project-media-grid">')
            for img in non_empty_images[:24]:
                url = file_url(proj_name, img["path"], site_domain, port_api)
                label = os.path.basename(img["path"])
                md.append(
                    f'  <a href="{url}" target="_blank" rel="noopener" class="media-thumb-card" title="{label}">'
                    f'    <img src="{url}" alt="{label}" loading="lazy" />'
                    f'    <span class="media-thumb-caption">{label}</span>'
                    f"  </a>"
                )
            md.append("</div>\n")
            if len(non_empty_images) > 24:
                md.append(f"*Showing first 24 of {len(non_empty_images)} media files in `{project_full_path}`.*\n")

    # 5. Other Project Files
    other_files = [
        f for f in files
        if f["ext"] not in DOC_EXTS
        and f["ext"] not in IMAGE_EXTS
        and f["ext"] not in VIDEO_EXTS
        and f["ext"] not in CODE_EXTS
    ]
    if other_files:
        md.append(f"\n## 📦 Additional Assets ({len(other_files)} files)\n")
        md.append("| File | Size |")
        md.append("| :--- | :--- |")
        for o in other_files[:35]:
            md.append(f"| `{o['path']}` | {fmt_size(o['size'])} |")
        md.append("")

    return "\n".join(md)


def main():
    cfg = load_wiki_config()
    projects_dir = cfg["projects_dir"]
    site_domain = cfg["site_domain"]
    port_api = cfg["port_api"]

    if not os.path.exists(INV_PATH):
        print(f":: [ProjectsWiki] No {INV_PATH} found. Run inspect_projects.py first if needed.")
        return

    with open(INV_PATH, "r", encoding="utf-8") as f:
        inventory = json.load(f)

    overrides = load_overrides(cfg)
    rebuilt_count = 0
    enriched_count = 0

    for proj_name, inv_entry in sorted(inventory.items()):
        clean_name = proj_name.replace(" ", "_")
        doc_path = os.path.join(DOCS_DIR, f"{clean_name}.md")
        exts = set(inv_entry.get("ext_counts", {}).keys())
        has_code = bool(exts & CODE_EXTS)
        ovr = overrides.get(proj_name, {})

        if ovr:
            title = ovr.get("title") or proj_name
            overview = ovr.get("overview") or f"Project files stored in `{os.path.join(projects_dir, proj_name)}`."
            content = f"# {title}\n\n## Overview\n\n{overview}\n"
            content += build_assets_markdown(proj_name, inv_entry, projects_dir, site_domain, port_api)
            with open(doc_path, "w", encoding="utf-8") as out:
                out.write(content)
            rebuilt_count += 1
            continue

        if not has_code:
            title = proj_name
            overview = f"Reference files, media, and documentation stored in `{os.path.join(projects_dir, proj_name)}`."
            content = f"# {title}\n\n## Overview\n\n{overview}\n"
            content += build_assets_markdown(proj_name, inv_entry, projects_dir, site_domain, port_api)
            with open(doc_path, "w", encoding="utf-8") as out:
                out.write(content)
            rebuilt_count += 1
        else:
            if os.path.exists(doc_path):
                with open(doc_path, "r", encoding="utf-8", errors="ignore") as f:
                    existing = f.read()
                if "## 📁 Directory Summary" in existing:
                    existing = existing.split("## 📁 Directory Summary")[0].rstrip()
                # Strip broken relative links to LICENSE or CONTRIBUTING.md
                existing = re.sub(r"\[([^\]]+)\]\((?:LICENSE|CONTRIBUTING\.md)\)", r"`\1`", existing)
                content = existing.rstrip() + "\n\n---\n" + build_assets_markdown(proj_name, inv_entry, projects_dir, site_domain, port_api)
                with open(doc_path, "w", encoding="utf-8") as out:
                    out.write(content)
                enriched_count += 1

    print(f":: [ProjectsWiki] Rebuilt {rebuilt_count} pages and enriched {enriched_count} code pages.")


if __name__ == "__main__":
    main()
