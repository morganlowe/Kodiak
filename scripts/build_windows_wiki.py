#!/usr/bin/env python3
"""
build_windows_wiki.py - Compiles ProjectsWiki into a standalone Windows .exe using PyInstaller.

Bundles Python runtime, MkDocs Material, and all ProjectsWiki scripts so Windows users
can run the ProjectsWiki documentation hub with zero external dependencies.
"""
import sys
import os
import shutil
import subprocess

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PROJECTS_WIKI_DIR = os.path.join(ROOT_DIR, "ProjectsWiki")
WIKI_SERVER_SCRIPT = os.path.join(PROJECTS_WIKI_DIR, "wiki_server.py")
OUTPUT_RESOURCE_DIR = os.path.join(ROOT_DIR, "src-tauri", "resources", "ProjectsWiki")
DIST_DIR = os.path.join(ROOT_DIR, "dist", "windows-wiki")


def main():
    print(":: [Windows Build] Preparing ProjectsWiki standalone executable...")

    # Ensure output directories exist
    os.makedirs(OUTPUT_RESOURCE_DIR, exist_ok=True)
    os.makedirs(DIST_DIR, exist_ok=True)

    # PyInstaller command with full dependencies collected
    cmd = [
        sys.executable,
        "-m",
        "PyInstaller",
        "--noconfirm",
        "--clean",
        "--onefile",
        "--name",
        "kodiak-projectswiki",
        "--paths",
        PROJECTS_WIKI_DIR,
        "--collect-all",
        "mkdocs",
        "--collect-all",
        "material",
        "--hidden-import",
        "yaml",
        "--hidden-import",
        "jinja2",
        "--hidden-import",
        "markdown",
        "--hidden-import",
        "wiki_config",
        "--hidden-import",
        "sync_api",
        "--hidden-import",
        "make_wiki",
        "--hidden-import",
        "build_wiki",
        "--hidden-import",
        "rebuild_accurate_docs",
        WIKI_SERVER_SCRIPT,
    ]

    print(f":: [Windows Build] Executing PyInstaller: {' '.join(cmd)}")
    proc = subprocess.run(cmd, cwd=PROJECTS_WIKI_DIR)
    if proc.returncode != 0:
        print("[!] PyInstaller build failed!")
        return proc.returncode

    built_exe = os.path.join(PROJECTS_WIKI_DIR, "dist", "kodiak-projectswiki.exe" if os.name == "nt" else "kodiak-projectswiki")
    if not os.path.exists(built_exe):
        # On non-Windows testing, might be without .exe extension
        fallback = os.path.join(PROJECTS_WIKI_DIR, "dist", "kodiak-projectswiki")
        if os.path.exists(fallback):
            built_exe = fallback

    if os.path.exists(built_exe):
        dest_exe = os.path.join(OUTPUT_RESOURCE_DIR, "kodiak-projectswiki.exe")
        shutil.copy2(built_exe, dest_exe)
        shutil.copy2(built_exe, os.path.join(DIST_DIR, "kodiak-projectswiki.exe"))
        print(f":: [Windows Build] Successfully staged: {dest_exe}")

    # Copy template configs, docs, and assets
    for item in ["mkdocs.yml", "config.example.toml"]:
        src = os.path.join(PROJECTS_WIKI_DIR, item)
        if os.path.exists(src):
            shutil.copy2(src, os.path.join(OUTPUT_RESOURCE_DIR, item))

    docs_src = os.path.join(PROJECTS_WIKI_DIR, "docs")
    docs_dst = os.path.join(OUTPUT_RESOURCE_DIR, "docs")
    if os.path.exists(docs_src):
        if os.path.exists(docs_dst):
            shutil.rmtree(docs_dst)
        shutil.copytree(docs_src, docs_dst)

    print(":: [Windows Build] ProjectsWiki standalone bundle ready in src-tauri/resources/ProjectsWiki/.")
    return 0


if __name__ == "__main__":
    sys.exit(main() or 0)
