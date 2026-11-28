#!/usr/bin/env python3
"""
build_windows_wiki.py - Builds the BenchBear project wiki runner as a Windows .exe using PyInstaller.

Bundles Python runtime, MkDocs Material, and BenchBear wiki scripts so Windows users
can run the BenchBear project wiki with zero external dependencies.
"""
import sys
import os
import shutil
import subprocess

ROOT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BENCHBEAR_DIR = os.path.join(ROOT_DIR, "BenchBear")
WIKI_SERVER_SCRIPT = os.path.join(BENCHBEAR_DIR, "wiki_server.py")
OUTPUT_RESOURCE_DIR = os.path.join(ROOT_DIR, "src-tauri", "resources", "BenchBear")
DIST_DIR = os.path.join(ROOT_DIR, "dist", "windows-wiki")


def main():
    print(":: [Windows Build] Preparing BenchBear project wiki runner...")

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
        "kodiak-benchbear",
        "--paths",
        BENCHBEAR_DIR,
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
    proc = subprocess.run(cmd, cwd=BENCHBEAR_DIR)
    if proc.returncode != 0:
        print("[!] PyInstaller build failed!")
        return proc.returncode

    built_exe = os.path.join(BENCHBEAR_DIR, "dist", "kodiak-benchbear.exe" if os.name == "nt" else "kodiak-benchbear")
    if not os.path.exists(built_exe):
        # On non-Windows testing, might be without .exe extension
        fallback = os.path.join(BENCHBEAR_DIR, "dist", "kodiak-benchbear")
        if os.path.exists(fallback):
            built_exe = fallback

    if os.path.exists(built_exe):
        dest_exe = os.path.join(OUTPUT_RESOURCE_DIR, "kodiak-benchbear.exe")
        shutil.copy2(built_exe, dest_exe)
        shutil.copy2(built_exe, os.path.join(DIST_DIR, "kodiak-benchbear.exe"))
        print(f":: [Windows Build] Successfully staged: {dest_exe}")

    # Build BenchBear Rust Inventory & Barcode binary
    print(":: [Windows Build] Compiling BenchBear inventory & barcode binary...")
    rust_cmd = ["cargo", "build", "--release", "--manifest-path", os.path.join(BENCHBEAR_DIR, "Cargo.toml")]
    rust_proc = subprocess.run(rust_cmd, cwd=ROOT_DIR)
    if rust_proc.returncode != 0:
        print("[!] BenchBear Rust build failed!")
        return rust_proc.returncode

    benchbear_bin = os.path.join(
        BENCHBEAR_DIR,
        "target",
        "release",
        "benchbear.exe" if os.name == "nt" else "benchbear",
    )
    if os.path.exists(benchbear_bin):
        dest_benchbear = os.path.join(OUTPUT_RESOURCE_DIR, "benchbear.exe")
        shutil.copy2(benchbear_bin, dest_benchbear)
        shutil.copy2(benchbear_bin, os.path.join(DIST_DIR, "benchbear.exe"))
        print(f":: [Windows Build] Successfully staged: {dest_benchbear}")

    # Copy template configs, docs, MCP wrapper, and assets
    for item in ["mkdocs.yml", "config.example.toml", "LICENSE"]:
        src = os.path.join(BENCHBEAR_DIR, item)
        if os.path.exists(src):
            shutil.copy2(src, os.path.join(OUTPUT_RESOURCE_DIR, item))

    mcp_dst = os.path.join(OUTPUT_RESOURCE_DIR, "mcp")
    os.makedirs(mcp_dst, exist_ok=True)
    for item in ["server.py", "README.md"]:
        src = os.path.join(BENCHBEAR_DIR, "mcp", item)
        if os.path.exists(src):
            shutil.copy2(src, os.path.join(mcp_dst, item))

    docs_src = os.path.join(BENCHBEAR_DIR, "docs")
    docs_dst = os.path.join(OUTPUT_RESOURCE_DIR, "docs")
    if os.path.exists(docs_src):
        if os.path.exists(docs_dst):
            shutil.rmtree(docs_dst)
        os.makedirs(docs_dst, exist_ok=True)
        allowed_docs = {
            "index.md",
            "demo-project.md",
            "inventory-hub.md",
            "extra.css",
            "extra.js",
            "Bear-built-logo.png",
        }
        for fname in allowed_docs:
            src_f = os.path.join(docs_src, fname)
            if os.path.isfile(src_f):
                shutil.copy2(src_f, os.path.join(docs_dst, fname))

    print(":: [Windows Build] BenchBear companion bundle ready in src-tauri/resources/BenchBear/.")
    return 0


if __name__ == "__main__":
    sys.exit(main() or 0)
