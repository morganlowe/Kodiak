#!/usr/bin/env python3
"""
sync_api.py - Local AJAX Control API + Project File/Media Server.
Listens on 127.0.0.1:8765 to allow the browser UI to:
  1. Trigger make_wiki.py via POST /sync
  2. Stream real project files, PDFs, manuals, images, and videos via GET /files/<project>/<path>
  3. Inspect & tune/edit project descriptions via GET /project-meta and POST /tune
  4. Check & toggle systemd user service via GET /service-status and POST /service-toggle
"""
import html
import http.server
import json
import mimetypes
import os
import re
import socketserver
import subprocess
import sys
import threading
import urllib.error
import urllib.parse
import urllib.request
from typing import Dict, Any

from wiki_config import load_wiki_config, load_overrides, save_overrides

WIKI_DIR = os.path.dirname(os.path.abspath(__file__))
MAKE_WIKI_SCRIPT = os.path.join(WIKI_DIR, "make_wiki.py")
BUILD_WIKI_SCRIPT = os.path.join(WIKI_DIR, "build_wiki.py")
PROJECTS_JSON = os.path.join(WIKI_DIR, "docs", "projects.json")
INV_PATH = os.path.join(WIKI_DIR, "projects_inventory.json")
SERVICE_FILE = os.path.expanduser("~/.config/systemd/user/projects-wiki.service")

_sync_lock = threading.Lock()


def load_json_file(path, default):
    if os.path.exists(path):
        try:
            with open(path, "r", encoding="utf-8") as f:
                return json.load(f)
        except Exception:
            pass
    return default


def extract_doc_parts(doc_path: str, proj_name: str):
    """Splits a project .md file into (title, overview_body, assets_tail)."""
    if not os.path.exists(doc_path):
        return proj_name, "", ""
    with open(doc_path, "r", encoding="utf-8", errors="ignore") as f:
        text = f.read()

    assets_tail = ""
    main_part = text
    for marker in ("\n---\n\n## 📁 Directory Summary", "\n## 📁 Directory Summary", "\n## 📁 Directory Status"):
        if marker in text:
            idx = text.index(marker)
            main_part = text[:idx].rstrip()
            assets_tail = text[idx:]
            break

    lines = main_part.splitlines()
    title = proj_name
    body_start = 0
    for i, line in enumerate(lines):
        if line.strip().startswith("# "):
            title = line.strip()[2:].strip()
            body_start = i + 1
            break

    body_lines = lines[body_start:]
    while body_lines and not body_lines[0].strip():
        body_lines.pop(0)
    if body_lines and re.match(r"^##\s+Overview\s*$", body_lines[0].strip(), re.IGNORECASE):
        body_lines.pop(0)
        while body_lines and not body_lines[0].strip():
            body_lines.pop(0)

    overview_body = "\n".join(body_lines).strip()
    return title, overview_body, assets_tail


def ai_tune_project(proj_name: str, current_title: str, current_cat: str, current_overview: str, user_prompt: str, cfg: Dict[str, Any]) -> dict:
    """Uses local Ollama to tune title, category, and overview."""
    inv = load_json_file(INV_PATH, {}).get(proj_name, {})
    file_names = [f["path"] for f in inv.get("files", [])[:30]]
    categories = list(cfg["categories"].keys())

    system_msg = (
        "You are a concise technical archivist updating a hardware/software wiki page. "
        "Given the user's correction note and the folder's actual filenames, return ONLY a valid JSON object "
        'with keys: "title" (concise project title), "category" (one of: '
        + ", ".join(f'"{c}"' for c in categories)
        + '), and "overview" (1 to 3 factual paragraphs in Markdown accurately describing the project based on '
        "the user's note without inventing fake components)."
    )
    user_msg = (
        f"Project Folder: {proj_name}\n"
        f"Current Title: {current_title}\n"
        f"Current Category: {current_cat}\n"
        f"Files in folder ({inv.get('total_files', 0)} total): {', '.join(file_names) or 'none'}\n\n"
        f"User Correction / Tuning Note:\n{user_prompt}\n\n"
        "Respond with JSON only: {\"title\": \"...\", \"category\": \"...\", \"overview\": \"...\"}"
    )

    models_to_try = [cfg["ollama_model"], "qwen2.5-coder:7b", "llama3.2:3b"]
    api_endpoint = f"{cfg['ollama_url'].rstrip('/')}/api/chat"

    for model_name in models_to_try:
        try:
            req_body = json.dumps({
                "model": model_name,
                "messages": [
                    {"role": "system", "content": system_msg},
                    {"role": "user", "content": user_msg},
                ],
                "stream": False,
                "format": "json",
            }).encode("utf-8")
            req = urllib.request.Request(
                api_endpoint,
                data=req_body,
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=25) as resp:
                resp_data = json.loads(resp.read().decode("utf-8"))
                content = resp_data.get("message", {}).get("content", "")
                parsed = json.loads(content)
                if parsed.get("title") and parsed.get("overview"):
                    cat = parsed.get("category")
                    if cat not in categories:
                        cat = current_cat
                    return {
                        "title": parsed["title"].strip(),
                        "category": cat,
                        "overview": parsed["overview"].strip(),
                    }
        except Exception:
            continue

    # Fallback if Ollama is unreachable
    cleaned = user_prompt.strip()
    cleaned = re.sub(r"^(?:it'?s\s+not\s+[^,.;]+[,.;]\s*)?(?:it'?s\s+)?", "", cleaned, flags=re.IGNORECASE)
    if cleaned:
        cleaned = cleaned[0].upper() + cleaned[1:]
        if not cleaned.endswith("."):
            cleaned += "."
    return {
        "title": current_title if current_title != proj_name else f"{proj_name}",
        "category": current_cat,
        "overview": cleaned or current_overview,
    }


def get_service_status() -> dict:
    enabled = False
    active = False
    try:
        res_en = subprocess.run(
            ["systemctl", "--user", "is-enabled", "projects-wiki.service"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        enabled = res_en.stdout.strip() == "enabled"
    except Exception:
        pass
    try:
        res_act = subprocess.run(
            ["systemctl", "--user", "is-active", "projects-wiki.service"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        active = res_act.stdout.strip() == "active"
    except Exception:
        pass
    return {"ok": True, "enabled": enabled, "active": active}


def set_service_boot(enable: bool) -> dict:
    os.makedirs(os.path.dirname(SERVICE_FILE), exist_ok=True)
    wiki_sh = os.path.join(WIKI_DIR, "wiki.sh")
    unit_content = f"""[Unit]
Description=ProjectsWiki Local Server
After=network.target

[Service]
Type=simple
WorkingDirectory={WIKI_DIR}
ExecStart={wiki_sh} daemon
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"""
    unit_changed = True
    if os.path.exists(SERVICE_FILE):
        try:
            with open(SERVICE_FILE, "r", encoding="utf-8") as f:
                if f.read() == unit_content:
                    unit_changed = False
        except Exception:
            pass

    if unit_changed:
        with open(SERVICE_FILE, "w", encoding="utf-8") as f:
            f.write(unit_content)
        subprocess.run(["systemctl", "--user", "daemon-reload"], capture_output=True, timeout=5)
    if enable:
        subprocess.run(["systemctl", "--user", "enable", "projects-wiki.service"], capture_output=True, timeout=5)
    else:
        subprocess.run(["systemctl", "--user", "disable", "projects-wiki.service"], capture_output=True, timeout=5)

    return get_service_status()


class SyncAPIHandler(http.server.BaseHTTPRequestHandler):
    def _send_cors_headers(self, status=200, content_type="application/json", extra_headers=None):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, HEAD, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type, Accept, Range")
        if extra_headers:
            for k, v in extra_headers.items():
                self.send_header(k, v)
        self.end_headers()

    def _read_json_body(self) -> dict:
        length = int(self.headers.get("Content-Length", "0"))
        if length <= 0:
            return {}
        raw = self.rfile.read(length).decode("utf-8", errors="ignore")
        return json.loads(raw)

    def do_OPTIONS(self):
        self._send_cors_headers(204)

    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        if parsed.path.startswith("/health"):
            self._send_cors_headers(200, extra_headers={"Cache-Control": "no-store"})
            self.wfile.write(json.dumps({"ok": True, "service": "projects-wiki-sync"}).encode("utf-8"))
            return
        if parsed.path.startswith("/service-status"):
            self._send_cors_headers(200, extra_headers={"Cache-Control": "no-store"})
            self.wfile.write(json.dumps(get_service_status()).encode("utf-8"))
            return
        if parsed.path.startswith("/project-meta"):
            qs = urllib.parse.parse_qs(parsed.query)
            name = (qs.get("name") or [""])[0]
            self._handle_project_meta(name)
            return
        if parsed.path.startswith("/sync"):
            self._handle_sync()
            return
        if parsed.path.startswith("/files/"):
            self._serve_project_file(parsed.path[len("/files/"):])
            return
        self._send_cors_headers(404)
        self.wfile.write(json.dumps({"ok": False, "error": "Not found"}).encode("utf-8"))

    def do_POST(self):
        parsed = urllib.parse.urlparse(self.path)
        if parsed.path.startswith("/sync"):
            self._handle_sync()
            return
        if parsed.path.startswith("/service-toggle"):
            body = self._read_json_body()
            current = get_service_status()
            target = body.get("enable", not current["enabled"])
            res = set_service_boot(bool(target))
            self._send_cors_headers(200, extra_headers={"Cache-Control": "no-store"})
            self.wfile.write(json.dumps(res).encode("utf-8"))
            return
        if parsed.path.startswith("/tune"):
            self._handle_tune()
            return
        if parsed.path.startswith("/build"):
            self._handle_build()
            return
        self._send_cors_headers(404)
        self.wfile.write(json.dumps({"ok": False, "error": "Not found"}).encode("utf-8"))

    def _handle_project_meta(self, proj_name: str):
        clean_name = proj_name.replace(" ", "_")
        doc_path = os.path.join(WIKI_DIR, "docs", f"{clean_name}.md")
        title, overview, _ = extract_doc_parts(doc_path, proj_name)
        catalog = load_json_file(PROJECTS_JSON, [])
        cfg = load_wiki_config()
        valid_cats = list(cfg["categories"].keys())
        cat = valid_cats[0] if valid_cats else "General & Prototypes"
        for item in catalog:
            if item.get("name") == proj_name or item.get("file") == f"{clean_name}.md":
                cat = item.get("category", cat)
                break
        overrides = load_overrides(cfg)
        ovr = overrides.get(proj_name, {})
        self._send_cors_headers(200, extra_headers={"Cache-Control": "no-store"})
        self.wfile.write(
            json.dumps({
                "ok": True,
                "name": proj_name,
                "title": ovr.get("title") or title,
                "category": ovr.get("category") or cat,
                "overview": ovr.get("overview") or overview,
                "categories": valid_cats,
            }).encode("utf-8")
        )

    def _handle_tune(self):
        try:
            body = self._read_json_body()
            proj_name = (body.get("name") or "").strip()
            if not proj_name:
                self._send_cors_headers(400)
                self.wfile.write(json.dumps({"ok": False, "error": "Missing project name"}).encode("utf-8"))
                return

            cfg = load_wiki_config()
            valid_cats = list(cfg["categories"].keys())
            default_cat = valid_cats[0] if valid_cats else "General & Prototypes"

            clean_name = proj_name.replace(" ", "_")
            doc_path = os.path.join(WIKI_DIR, "docs", f"{clean_name}.md")
            cur_title, cur_overview, assets_tail = extract_doc_parts(doc_path, proj_name)

            mode = body.get("mode", "manual")
            new_title = (body.get("title") or cur_title or proj_name).strip()
            new_cat = (body.get("category") or default_cat).strip()
            new_overview = (body.get("overview") or cur_overview).strip()
            user_prompt = (body.get("prompt") or "").strip()

            if mode == "ai" and user_prompt:
                ai_res = ai_tune_project(proj_name, new_title, new_cat, new_overview, user_prompt, cfg)
                new_title = ai_res["title"]
                new_cat = ai_res["category"]
                new_overview = ai_res["overview"]

            overrides = load_overrides(cfg)
            overrides[proj_name] = {
                "title": new_title,
                "category": new_cat,
                "overview": new_overview,
            }
            save_overrides(overrides, cfg)

            # Write updated markdown while preserving directory summary / assets tail
            new_md = f"# {new_title}\n\n## Overview\n\n{new_overview}\n"
            if assets_tail:
                new_md += "\n" + assets_tail.lstrip("\n")
            with open(doc_path, "w", encoding="utf-8") as f:
                f.write(new_md)

            # Rebuild catalog & mkdocs.yml
            subprocess.run([sys.executable, MAKE_WIKI_SCRIPT], cwd=WIKI_DIR, capture_output=True, timeout=60)

            self._send_cors_headers(200, extra_headers={"Cache-Control": "no-store"})
            self.wfile.write(
                json.dumps({
                    "ok": True,
                    "name": proj_name,
                    "title": new_title,
                    "category": new_cat,
                    "overview": new_overview,
                }).encode("utf-8")
            )
        except Exception as exc:
            self._send_cors_headers(500)
            self.wfile.write(json.dumps({"ok": False, "error": str(exc)}).encode("utf-8"))

    def _handle_build(self):
        try:
            body = self._read_json_body()
            proj_name = (body.get("name") or body.get("project") or "").strip()
            cfg = load_wiki_config()
            model = (body.get("model") or cfg["ollama_model"]).strip()
            overwrite = bool(body.get("overwrite", True))

            cmd = [sys.executable, BUILD_WIKI_SCRIPT, "--model", model, "--projects-dir", cfg["projects_dir"]]
            if proj_name:
                cmd.extend(["--project", proj_name])
            else:
                cmd.append("--all")
            if overwrite:
                cmd.append("--overwrite")

            proc = subprocess.run(cmd, cwd=WIKI_DIR, capture_output=True, text=True, timeout=300)
            if proc.returncode != 0:
                self._send_cors_headers(500)
                self.wfile.write(json.dumps({"ok": False, "error": proc.stderr or proc.stdout}).encode("utf-8"))
                return

            subprocess.run([sys.executable, MAKE_WIKI_SCRIPT], cwd=WIKI_DIR, capture_output=True, timeout=60)

            self._send_cors_headers(200, extra_headers={"Cache-Control": "no-store"})
            self.wfile.write(json.dumps({
                "ok": True,
                "project": proj_name,
                "output": proc.stdout,
            }).encode("utf-8"))
        except Exception as exc:
            self._send_cors_headers(500)
            self.wfile.write(json.dumps({"ok": False, "error": str(exc)}).encode("utf-8"))

    def _serve_project_file(self, encoded_rel_path: str):
        cfg = load_wiki_config()
        projects_dir = cfg["projects_dir"]
        rel_path = urllib.parse.unquote(encoded_rel_path).lstrip("/")
        base_real = os.path.realpath(projects_dir)
        target_real = os.path.realpath(os.path.join(base_real, rel_path))

        if not (target_real == base_real or target_real.startswith(base_real + os.sep)):
            self._send_cors_headers(403)
            self.wfile.write(json.dumps({"ok": False, "error": "Forbidden"}).encode("utf-8"))
            return

        if not os.path.isfile(target_real):
            self._send_cors_headers(404)
            self.wfile.write(json.dumps({"ok": False, "error": "File not found"}).encode("utf-8"))
            return

        ctype, _ = mimetypes.guess_type(target_real)
        if not ctype:
            ctype = "application/octet-stream"

        try:
            fsize = os.path.getsize(target_real)
            self._send_cors_headers(
                200,
                content_type=ctype,
                extra_headers={
                    "Content-Length": str(fsize),
                    "Cache-Control": "public, max-age=3600",
                },
            )
            with open(target_real, "rb") as f:
                while True:
                    chunk = f.read(65536)
                    if not chunk:
                        break
                    self.wfile.write(chunk)
        except (BrokenPipeError, ConnectionResetError):
            pass
        except Exception:
            pass

    def _handle_sync(self):
        if not _sync_lock.acquire(blocking=False):
            self._send_cors_headers(409, extra_headers={"Cache-Control": "no-store"})
            self.wfile.write(
                json.dumps({"ok": False, "error": "A sync is already in progress."}).encode("utf-8")
            )
            return

        try:
            before_count = len(load_json_file(PROJECTS_JSON, []))
            proc = subprocess.run(
                [sys.executable, MAKE_WIKI_SCRIPT],
                cwd=WIKI_DIR,
                capture_output=True,
                text=True,
                timeout=300,
            )
            after_count = len(load_json_file(PROJECTS_JSON, []))
            ok = proc.returncode == 0
            payload = {
                "ok": ok,
                "projects_count": after_count,
                "added_count": max(0, after_count - before_count),
                "output": (proc.stdout or "").strip(),
                "error": (proc.stderr or "").strip() if not ok else None,
            }
            self._send_cors_headers(200 if ok else 500, extra_headers={"Cache-Control": "no-store"})
            self.wfile.write(json.dumps(payload).encode("utf-8"))
        except Exception as exc:
            self._send_cors_headers(500, extra_headers={"Cache-Control": "no-store"})
            self.wfile.write(json.dumps({"ok": False, "error": str(exc)}).encode("utf-8"))
        finally:
            _sync_lock.release()

    def log_message(self, format, *args):
        pass


class Port8000RedirectHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        cfg = load_wiki_config()
        self.send_response(302)
        self.send_header("Location", f"http://{cfg['site_domain']}{self.path}")
        self.end_headers()

    def log_message(self, format, *args):
        pass


class ReusableTCPServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
    allow_reuse_address = True
    daemon_threads = True


def start_redirect_server():
    try:
        with ReusableTCPServer(("127.0.0.1", 8000), Port8000RedirectHandler) as httpd:
            httpd.serve_forever()
    except Exception:
        pass


def main():
    cfg = load_wiki_config()
    api_port = cfg["port_api"]
    enable_redirect = os.environ.get("WIKI_REDIRECT_8000", "0") == "1"

    if enable_redirect:
        t = threading.Thread(target=start_redirect_server, daemon=True)
        t.start()

    try:
        with ReusableTCPServer(("127.0.0.1", api_port), SyncAPIHandler) as httpd:
            httpd.serve_forever()
    except OSError:
        pass


if __name__ == "__main__":
    main()
