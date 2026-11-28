#!/usr/bin/env python3
"""
wiki_server.py - Cross-Platform Execution Engine for BenchBear.

Provides a unified runner for:
  - serve:   Launches MkDocs Material, Sync API, and the Rust Inventory/Barcode Hub concurrently.
  - sync:    Regenerates projects.json, inventory Markdown exports, and mkdocs.yml navigation.
  - build:   Synthesizes documentation for projects using local Ollama.
  - rebuild: Rebuilds accurate Markdown pages with media links.
  - stop:    Gracefully terminates running server processes (Windows/Linux).
"""
import argparse
import os
import shutil
import subprocess
import sys
import threading
import time
from typing import Optional

WIKI_DIR = os.path.dirname(os.path.abspath(__file__))
if WIKI_DIR not in sys.path:
    sys.path.insert(0, WIKI_DIR)

from wiki_config import load_wiki_config, ensure_workspace_ready


def find_mkdocs_cmd():
    """Locates the mkdocs executable or module runner across platforms."""
    cmd = shutil.which("mkdocs")
    if cmd:
        return [cmd]

    try:
        proc = subprocess.run(
            [sys.executable, "-m", "mkdocs", "--version"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        if proc.returncode == 0:
            return [sys.executable, "-m", "mkdocs"]
    except Exception:
        pass

    candidates = [
        os.path.join(WIKI_DIR, ".venv", "bin", "mkdocs"),
        os.path.join(WIKI_DIR, ".venv", "Scripts", "mkdocs.exe"),
        os.path.expanduser("~/.local/share/benchbear/venv/bin/mkdocs"),
        os.path.expanduser("~/.local/share/kodiak/benchbear-venv/bin/mkdocs"),
        os.path.expandvars(r"%LOCALAPPDATA%\benchbear\venv\Scripts\mkdocs.exe"),
        os.path.expandvars(r"%LOCALAPPDATA%\kodiak\benchbear-venv\Scripts\mkdocs.exe"),
        os.path.expanduser("~/.pyenv/shims/mkdocs"),
    ]
    for c in candidates:
        if os.path.isfile(c) and os.access(c, os.X_OK):
            return [c]

    return None


def find_or_build_rust_bin() -> Optional[str]:
    exe_name = "benchbear.exe" if os.name == "nt" else "benchbear"
    for cand in (
        os.path.join(WIKI_DIR, "target", "release", exe_name),
        os.path.join(WIKI_DIR, exe_name),
        os.path.join(WIKI_DIR, "bin", exe_name),
    ):
        if os.path.isfile(cand) and os.access(cand, os.X_OK):
            return cand
    which_bin = shutil.which("benchbear")
    if which_bin:
        return which_bin

    cargo = shutil.which("cargo")
    cargo_toml = os.path.join(WIKI_DIR, "Cargo.toml")
    target_name = "benchbear.exe" if os.name == "nt" else "benchbear"
    target_bin = os.path.join(WIKI_DIR, "target", "release", target_name)
    if cargo and os.path.isfile(cargo_toml):
        print(":: [BenchBear] Building Rust Inventory & Barcode binary (release)...")
        res = subprocess.run([cargo, "build", "--release"], cwd=WIKI_DIR)
        if res.returncode == 0 and os.path.isfile(target_bin):
            return target_bin
    return None


def run_sync_api(port_api: int):
    """Runs the Sync API HTTP server in a background thread."""
    try:
        from sync_api import ReusableTCPServer, SyncAPIHandler
        print(f":: [BenchBear] Sync API starting on 127.0.0.1:{port_api}")
        with ReusableTCPServer(("127.0.0.1", port_api), SyncAPIHandler) as httpd:
            httpd.serve_forever()
    except OSError as e:
        print(f":: [BenchBear] Sync API bind note: {e}")
    except Exception as e:
        print(f":: [BenchBear] Sync API error: {e}")


def serve_command(args):
    """Starts MkDocs Material, Sync API, and the Rust Inventory/Barcode Hub concurrently."""
    cfg = load_wiki_config()
    workspace_dir = str(ensure_workspace_ready())
    port_web = cfg.get("port_web", 8000)
    port_api = cfg.get("port_api", 8765)
    port_inv = cfg.get("port_inventory", 8090)
    data_dir = cfg.get("data_dir", os.path.join(workspace_dir, "data"))
    site_domain = cfg.get("site_domain", "benchbear.localhost")

    inv_proc = None
    rust_bin = find_or_build_rust_bin()
    if rust_bin:
        print(f":: [BenchBear] Starting Inventory & Barcode Hub on http://127.0.0.1:{port_inv}")
        inv_proc = subprocess.Popen(
            [
                rust_bin,
                "--wiki-root", workspace_dir,
                "--data-dir", data_dir,
                "serve",
                "--host", "127.0.0.1",
                "--port", str(port_inv),
            ],
            cwd=workspace_dir,
        )
    else:
        print(":: [BenchBear] Note: Rust binary unavailable; inventory hub not started.")

    api_thread = threading.Thread(target=run_sync_api, args=(port_api,), daemon=True)
    api_thread.start()

    bind_addr = f"127.0.0.1:{port_web}"
    print(f":: [BenchBear] Serving MkDocs on http://{bind_addr} (Domain: http://{site_domain})")

    mkdocs_cmd = find_mkdocs_cmd()
    if not mkdocs_cmd:
        print("[!] MkDocs was not found on PATH or in standard venvs.")
        print("[!] Please install mkdocs-material: pip install mkdocs-material pyyaml")
        try:
            while True:
                time.sleep(1)
        except KeyboardInterrupt:
            if inv_proc:
                inv_proc.terminate()
            return 1

    cmd = list(mkdocs_cmd) + ["serve", "-a", bind_addr]
    env = os.environ.copy()
    env["NO_MKDOCS_2_WARNING"] = "1"

    proc = subprocess.Popen(cmd, cwd=workspace_dir, env=env)
    try:
        proc.wait()
    except KeyboardInterrupt:
        proc.terminate()
        proc.wait()
    finally:
        if inv_proc and inv_proc.poll() is None:
            inv_proc.terminate()
            inv_proc.wait()
    return proc.returncode


def sync_command(args):
    """Exports inventory Markdown/snapshot and regenerates projects.json and mkdocs.yml."""
    cfg = load_wiki_config()
    workspace_dir = str(ensure_workspace_ready())
    rust_bin = find_or_build_rust_bin()
    if rust_bin:
        subprocess.run(
            [rust_bin, "--wiki-root", workspace_dir, "--data-dir", cfg["data_dir"], "export"],
            cwd=workspace_dir,
            check=False,
        )
    from make_wiki import main as make_main
    make_main()
    return 0


def build_command(args):
    """Synthesizes project documentation using local Ollama models."""
    from build_wiki import build_doc_for_project, sync_catalog
    cfg = load_wiki_config()
    projects_dir = cfg.get("projects_dir") or os.environ.get("PROJECTS_DIR") or os.path.expanduser("~/Projects")
    model = args.model or cfg.get("ollama_model", "qwen2.5-coder:7b")
    ollama_url = args.ollama_url or cfg.get("ollama_url", "http://127.0.0.1:11434")

    if not args.project and not args.all:
        print("[!] Please specify --project <name> or --all.")
        return 1

    if args.project:
        ok = build_doc_for_project(
            args.project,
            projects_dir,
            model=model,
            ollama_url=ollama_url,
            overwrite=not args.no_overwrite,
            dry_run=args.dry_run,
        )
        if ok and not args.dry_run:
            sync_catalog()
        return 0 if ok else 1
    else:
        from build_wiki import main as build_main
        build_main()
        return 0


def rebuild_command(args):
    """Rebuilds accurate Markdown documentation pages."""
    from rebuild_accurate_docs import main as rebuild_main
    rebuild_main()
    return 0


def stop_command(args):
    """Terminates running server instances across Windows and Linux."""
    if os.name == "nt":
        subprocess.run(["taskkill", "/F", "/IM", "benchbear.exe", "/T"], stderr=subprocess.DEVNULL)
        subprocess.run(["taskkill", "/F", "/IM", "kodiak-benchbear.exe", "/T"], stderr=subprocess.DEVNULL)
    else:
        subprocess.run(["pkill", "-f", "mkdocs serve"], stderr=subprocess.DEVNULL)
        subprocess.run(["pkill", "-f", "sync_api.py"], stderr=subprocess.DEVNULL)
        subprocess.run(["pkill", "-f", "benchbear.*serve"], stderr=subprocess.DEVNULL)
    print(":: [BenchBear] Stopped server processes.")
    return 0


def main():
    parser = argparse.ArgumentParser(description="BenchBear Cross-Platform Runner")
    subparsers = parser.add_subparsers(dest="command", help="Command to execute")

    sub_serve = subparsers.add_parser("serve", help="Run MkDocs, Sync API, and Rust Inventory Hub")
    sub_serve.set_defaults(func=serve_command)

    sub_sync = subparsers.add_parser("sync", help="Export inventory & sync project catalog + mkdocs.yml")
    sub_sync.set_defaults(func=sync_command)

    sub_build = subparsers.add_parser("build", help="Synthesize documentation via Ollama")
    sub_build.add_argument("--project", "-p", help="Target project directory name")
    sub_build.add_argument("--model", "-m", help="Ollama model")
    sub_build.add_argument("--ollama-url", help="Ollama URL")
    sub_build.add_argument("--all", "-a", action="store_true", help="Build all projects")
    sub_build.add_argument("--no-overwrite", action="store_true", help="Do not overwrite existing docs")
    sub_build.add_argument("--dry-run", action="store_true", help="Dry run without writing")
    sub_build.set_defaults(func=build_command)

    sub_rebuild = subparsers.add_parser("rebuild", help="Rebuild accurate markdown documentation")
    sub_rebuild.set_defaults(func=rebuild_command)

    sub_stop = subparsers.add_parser("stop", help="Stop running server instances")
    sub_stop.set_defaults(func=stop_command)

    args = parser.parse_args()
    if not args.command:
        return serve_command(args)

    return args.func(args)


if __name__ == "__main__":
    sys.exit(main() or 0)
