#!/usr/bin/env python3
"""
wiki_server.py - Cross-Platform Execution Engine for Kodiak ProjectsWiki.

Provides a unified runner for:
  - serve:   Launches MkDocs Material server & Sync API concurrently.
  - sync:    Regenerates projects.json and mkdocs.yml navigation.
  - build:   Synthesizes documentation for projects using local Ollama.
  - rebuild: Rebuilds accurate Markdown pages with media links.
  - stop:    Gracefully terminates running server processes (Windows/Linux).

Can be executed directly via Python or compiled into `kodiak-projectswiki.exe` via PyInstaller.
"""
import sys
import os
import argparse
import subprocess
import shutil
import threading
import time

# Ensure ProjectsWiki directory is on sys.path
WIKI_DIR = os.path.dirname(os.path.abspath(__file__))
if WIKI_DIR not in sys.path:
    sys.path.insert(0, WIKI_DIR)

from wiki_config import load_wiki_config


def find_mkdocs_cmd():
    """Locates the mkdocs executable or module runner across platforms."""
    # 1. System PATH
    cmd = shutil.which("mkdocs")
    if cmd:
        return [cmd]

    # 2. Python module invocation
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

    # 3. Known virtual environments
    candidates = [
        # Linux standard local venv
        os.path.expanduser("~/.local/share/kodiak/projectswiki-venv/bin/mkdocs"),
        # Windows standard local venv
        os.path.expandvars(r"%LOCALAPPDATA%\kodiak\projectswiki-venv\Scripts\mkdocs.exe"),
        # pyenv shims
        os.path.expanduser("~/.pyenv/shims/mkdocs"),
    ]
    for c in candidates:
        if os.path.isfile(c) and os.access(c, os.X_OK):
            return [c]

    return None


def run_sync_api(port_api: int):
    """Runs the Sync API HTTP server in a background thread."""
    try:
        from sync_api import ReusableTCPServer, SyncAPIHandler
        print(f":: [ProjectsWiki] Sync API starting on 127.0.0.1:{port_api}")
        with ReusableTCPServer(("127.0.0.1", port_api), SyncAPIHandler) as httpd:
            httpd.serve_forever()
    except OSError as e:
        print(f":: [ProjectsWiki] Sync API bind note: {e}")
    except Exception as e:
        print(f":: [ProjectsWiki] Sync API error: {e}")


def serve_command(args):
    """Starts the MkDocs Material documentation server and Sync API concurrently."""
    cfg = load_wiki_config()
    port_web = cfg.get("port_web", 8000)
    port_api = cfg.get("port_api", 8765)
    site_domain = cfg.get("site_domain", "wiki.local")

    # Start Sync API in daemon thread
    api_thread = threading.Thread(target=run_sync_api, args=(port_api,), daemon=True)
    api_thread.start()

    bind_addr = f"127.0.0.1:{port_web}"
    print(f":: [ProjectsWiki] Serving MkDocs on http://{bind_addr} (Domain: http://{site_domain})")

    # If bundled via PyInstaller, mkdocs may be importable directly
    if getattr(sys, "frozen", False):
        try:
            from mkdocs.commands import serve as mkdocs_serve
            from mkdocs.config import load_config
            cfg_file = os.path.join(WIKI_DIR, "mkdocs.yml")
            if os.path.exists(cfg_file):
                print(f":: [ProjectsWiki] Launching in-process MkDocs Material server...")
                # Serve blocking in main thread
                mkdocs_serve.serve(config_file=cfg_file, dev_addr=bind_addr)
                return 0
        except Exception as e:
            print(f":: [ProjectsWiki] In-process mkdocs note: {e}, falling back to CLI...")

    mkdocs_cmd = find_mkdocs_cmd()
    if not mkdocs_cmd:
        print(f"[!] MkDocs was not found on PATH or in standard venvs.")
        print(f"[!] Please install mkdocs-material: pip install mkdocs-material pyyaml")
        # Keep Sync API alive even if MkDocs CLI is missing
        while True:
            time.sleep(1)
        return 1

    cmd = list(mkdocs_cmd) + ["serve", "-a", bind_addr]
    env = os.environ.copy()
    env["NO_MKDOCS_2_WARNING"] = "1"

    proc = subprocess.Popen(cmd, cwd=WIKI_DIR, env=env)
    try:
        proc.wait()
    except KeyboardInterrupt:
        proc.terminate()
        proc.wait()
    return proc.returncode


def sync_command(args):
    """Regenerates projects.json and mkdocs.yml."""
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
        # All projects
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
        subprocess.run(["taskkill", "/F", "/IM", "kodiak-projectswiki.exe", "/T"], stderr=subprocess.DEVNULL)
    else:
        subprocess.run(["pkill", "-f", "mkdocs serve"], stderr=subprocess.DEVNULL)
        subprocess.run(["pkill", "-f", "sync_api.py"], stderr=subprocess.DEVNULL)
    print(":: [ProjectsWiki] Stopped server processes.")
    return 0


def main():
    parser = argparse.ArgumentParser(description="Kodiak ProjectsWiki Cross-Platform Runner")
    subparsers = parser.add_subparsers(dest="command", help="Command to execute")

    # serve
    sub_serve = subparsers.add_parser("serve", help="Run documentation server & Sync API")
    sub_serve.set_defaults(func=serve_command)

    # sync
    sub_sync = subparsers.add_parser("sync", help="Sync project inventory & mkdocs.yml")
    sub_sync.set_defaults(func=sync_command)

    # build
    sub_build = subparsers.add_parser("build", help="Synthesize documentation via Ollama")
    sub_build.add_argument("--project", "-p", help="Target project directory name")
    sub_build.add_argument("--model", "-m", help="Ollama model")
    sub_build.add_argument("--ollama-url", help="Ollama URL")
    sub_build.add_argument("--all", "-a", action="store_true", help="Build all projects")
    sub_build.add_argument("--no-overwrite", action="store_true", help="Do not overwrite existing docs")
    sub_build.add_argument("--dry-run", action="store_true", help="Dry run without writing")
    sub_build.set_defaults(func=build_command)

    # rebuild
    sub_rebuild = subparsers.add_parser("rebuild", help="Rebuild accurate markdown documentation")
    sub_rebuild.set_defaults(func=rebuild_command)

    # stop
    sub_stop = subparsers.add_parser("stop", help="Stop running server instances")
    sub_stop.set_defaults(func=stop_command)

    args = parser.parse_args()

    # Default to serve if no subcommand provided
    if not args.command:
        return serve_command(args)

    return args.func(args)


if __name__ == "__main__":
    sys.exit(main() or 0)
