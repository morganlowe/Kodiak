#!/usr/bin/env python3
"""
build_wiki.py - AI Documentation Generator for Kodiak ProjectsWiki.

Synthesizes rich technical documentation pages for local project repositories
using local Ollama models with zero cloud data transmission.
"""
import argparse
import json
import os
import subprocess
import sys
import urllib.request
from typing import List, Optional

from wiki_config import load_wiki_config

WIKI_DIR = os.path.dirname(os.path.abspath(__file__))
WIKI_DOCS_DIR = os.path.join(WIKI_DIR, "docs")

VALID_EXTENSIONS = (
    ".py", ".rs", ".c", ".cpp", ".h", ".hpp", ".sh", ".js", ".ts",
    ".ino", ".md", ".txt", ".json", ".toml", ".yaml", ".yml",
)


def call_ollama_chat(prompt: str, requested_model: str = "qwen2.5-coder:7b", ollama_url: str = "http://127.0.0.1:11434") -> str:
    """Zero-dependency HTTP client to local Ollama with model fallback."""
    candidate_models = [requested_model]
    for alt in ("qwen2.5-coder:7b", "qwen2.5-coder", "llama3.2:3b"):
        if alt not in candidate_models:
            candidate_models.append(alt)

    api_endpoint = f"{ollama_url.rstrip('/')}/api/chat"
    last_error = None
    for model_name in candidate_models:
        try:
            req_data = json.dumps({
                "model": model_name,
                "messages": [
                    {
                        "role": "system",
                        "content": (
                            "You are a principal embedded systems, hardware, and software engineer. "
                            "Write comprehensive, factual technical documentation in Markdown. "
                            "Include a clear summary, architecture overview, hardware/microcontroller specs, "
                            "software dependencies, and key workflows. Do not invent components not present in code."
                        ),
                    },
                    {"role": "user", "content": prompt},
                ],
                "stream": False,
            }).encode("utf-8")

            req = urllib.request.Request(
                api_endpoint,
                data=req_data,
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=120) as resp:
                data = json.loads(resp.read().decode("utf-8"))
                content = data.get("message", {}).get("content", "")
                if content.strip():
                    return content.strip()
        except Exception as e:
            last_error = e
            continue

    # Try python ollama library if installed as secondary fallback
    try:
        import ollama
        resp = ollama.chat(model=requested_model, messages=[{"role": "user", "content": prompt}])
        return resp["message"]["content"]
    except Exception:
        pass

    raise RuntimeError(f"Failed to query Ollama at {api_endpoint} with models {candidate_models}: {last_error}")


def extract_project_code_context(project_path: str, project_name: str, max_chars: int = 8000) -> str:
    """Scans and extracts representative code snippets from a project repository."""
    code_snippets = []
    total_chars = 0

    for root, dirs, files in os.walk(project_path):
        dirs[:] = [d for d in dirs if d not in {".git", "node_modules", "target", "venv", "__pycache__", "dist", "build", "site"}]
        for file in sorted(files):
            if file.lower().endswith(VALID_EXTENSIONS):
                file_path = os.path.join(root, file)
                rel_path = os.path.relpath(file_path, project_path)
                try:
                    with open(file_path, "r", encoding="utf-8", errors="ignore") as f:
                        content = f.read(1500)
                        if content.strip():
                            snippet = f"\n--- File: {rel_path} ---\n{content}\n"
                            code_snippets.append(snippet)
                            total_chars += len(snippet)
                            if total_chars >= max_chars:
                                break
                except Exception:
                    pass
        if total_chars >= max_chars:
            break

    if not code_snippets:
        return f"Project name: {project_name}\nRepository contains custom hardware, assets, or non-standard file formats."

    combined = "".join(code_snippets)
    return combined[:max_chars]


def build_doc_for_project(
    project_name: str,
    projects_dir: str,
    model: str = "qwen2.5-coder:7b",
    ollama_url: str = "http://127.0.0.1:11434",
    overwrite: bool = False,
    dry_run: bool = False,
) -> bool:
    """Generates markdown documentation for a given project."""
    project_path = os.path.join(projects_dir, project_name)
    if not os.path.isdir(project_path):
        print(f"[!] Project directory not found: {project_path}")
        return False

    clean_name = project_name.replace(" ", "_")
    doc_filename = f"{clean_name}.md"
    doc_path = os.path.join(WIKI_DOCS_DIR, doc_filename)

    if os.path.exists(doc_path) and not overwrite:
        print(f"[*] Skipping {project_name} (doc already exists at {doc_path}). Use --overwrite to replace.")
        return True

    print(f"[+] Analyzing project repository: {project_name} ({project_path})...")
    context = extract_project_code_context(project_path, project_name)

    prompt = (
        f"Analyze this project repository named '{project_name}' and write a comprehensive, "
        f"production-grade technical wiki page in Markdown. Detail its purpose, architecture, "
        f"hardware/software components, key dependencies, and usage.\n\n"
        f"Source Context:\n{context}"
    )

    if dry_run:
        print(f"[DRY-RUN] Target Doc: {doc_path}")
        print(f"[DRY-RUN] Prompt snippet:\n{prompt[:400]}...")
        return True

    print(f"[+] Generating documentation via local Ollama ({model})...")
    try:
        doc_content = call_ollama_chat(prompt, requested_model=model, ollama_url=ollama_url)
    except Exception as e:
        print(f"[!] Error generating wiki page for {project_name}: {e}")
        return False

    os.makedirs(WIKI_DOCS_DIR, exist_ok=True)
    with open(doc_path, "w", encoding="utf-8") as f:
        f.write(doc_content + "\n")

    print(f"[OK] Successfully wrote documentation to {doc_path}")
    return True


def sync_catalog():
    """Runs make_wiki.py to refresh the MkDocs navigation and catalog."""
    make_wiki_py = os.path.join(WIKI_DIR, "make_wiki.py")
    if os.path.exists(make_wiki_py):
        try:
            subprocess.run([sys.executable, make_wiki_py], cwd=WIKI_DIR, check=True)
            print("[OK] Projects Wiki catalog and mkdocs.yml updated.")
        except Exception as e:
            print(f"[!] Warning: failed to refresh catalog via make_wiki.py: {e}")


def main():
    cfg = load_wiki_config()
    default_projects_dir = cfg["projects_dir"]
    default_model = cfg["ollama_model"]
    default_ollama_url = cfg["ollama_url"]

    parser = argparse.ArgumentParser(description="Projects Wiki AI Documentation Generator")
    parser.add_argument("--project", "-p", help="Target a specific project folder by name")
    parser.add_argument("--projects-dir", "-d", default=default_projects_dir, help="Path to Projects directory")
    parser.add_argument("--model", "-m", default=default_model, help=f"Ollama model (default: {default_model})")
    parser.add_argument("--ollama-url", default=default_ollama_url, help=f"Ollama URL (default: {default_ollama_url})")
    parser.add_argument("--all", "-a", action="store_true", help="Generate docs for ALL projects in the directory")
    parser.add_argument("--overwrite", action="store_true", help="Overwrite existing project markdown files")
    parser.add_argument("--dry-run", action="store_true", help="Display prompt and context without calling Ollama")
    parser.add_argument("--no-sync", action="store_true", help="Do not trigger make_wiki.py catalog sync upon completion")

    args = parser.parse_args()

    projects_dir = os.path.realpath(os.path.expanduser(args.projects_dir))
    if not os.path.isdir(projects_dir):
        print(f"[!] Projects directory does not exist: {projects_dir}")
        sys.exit(1)

    print(f":: [ProjectsWiki] Active projects root: {projects_dir}")
    print(f":: [ProjectsWiki] Target model: {args.model}")

    success_count = 0
    failure_count = 0

    if args.project:
        ok = build_doc_for_project(
            args.project,
            projects_dir,
            model=args.model,
            ollama_url=args.ollama_url,
            overwrite=args.overwrite,
            dry_run=args.dry_run,
        )
        if ok:
            success_count += 1
        else:
            failure_count += 1
    elif args.all:
        candidates = sorted(os.listdir(projects_dir))
        for proj in candidates:
            p_path = os.path.join(projects_dir, proj)
            if not os.path.isdir(p_path) or proj.startswith("."):
                continue
            ok = build_doc_for_project(
                proj,
                projects_dir,
                model=args.model,
                ollama_url=args.ollama_url,
                overwrite=args.overwrite,
                dry_run=args.dry_run,
            )
            if ok:
                success_count += 1
            else:
                failure_count += 1
    else:
        print("[!] Please specify --project <name> or --all to build documentation.")
        sys.exit(1)

    print(f"\n[Summary] {success_count} project docs processed successfully, {failure_count} errors.")

    if not args.no_sync and not args.dry_run and success_count > 0:
        sync_catalog()


if __name__ == "__main__":
    main()
