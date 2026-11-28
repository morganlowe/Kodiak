#!/usr/bin/env python3
"""
mcp/server.py - stdio MCP Server wrapper for Projects & Parts Wiki.

Prefers the compiled Rust binary (`target/release/parts-wiki mcp`) so MCP tools
operate directly against the portable SQLite DB, locations, suppliers, costs,
BOMs, and barcode labels. Falls back to the legacy Markdown parser if the Rust
binary has not been built yet.
"""
import datetime
import json
import os
import pathlib
import re
import sys

WIKI_ROOT = pathlib.Path(__file__).resolve().parent.parent
RUST_BIN = WIKI_ROOT / "target" / "release" / "benchbear"
if not RUST_BIN.is_file():
    RUST_BIN = WIKI_ROOT / "target" / "release" / "parts-wiki"

if RUST_BIN.is_file() and os.access(RUST_BIN, os.X_OK):
    os.execv(str(RUST_BIN), [str(RUST_BIN), "--wiki-root", str(WIKI_ROOT), "mcp"])

# Legacy fallback if Rust binary is not compiled yet
INV_DIR = WIKI_ROOT / "inventory"
PROJ_DIR = WIKI_ROOT / "projects"
AUDIT_LOG = WIKI_ROOT / "data" / "audit.jsonl"


def audit(tool: str, detail: dict) -> None:
    AUDIT_LOG.parent.mkdir(parents=True, exist_ok=True)
    entry = {"ts": datetime.datetime.now(datetime.timezone.utc).isoformat(), "tool": tool, "detail": detail}
    with AUDIT_LOG.open("a", encoding="utf-8") as f:
        f.write(json.dumps(entry) + "\n")


def safe_resolve(rel_or_path: str) -> pathlib.Path:
    p = pathlib.Path(rel_or_path)
    target = (WIKI_ROOT / p).resolve() if not p.is_absolute() else p.resolve()
    if target != WIKI_ROOT and WIKI_ROOT not in target.parents:
        raise ValueError(f"Path outside wiki root: {target}")
    return target


def parse_table_file(path: pathlib.Path):
    lines = path.read_text(encoding="utf-8").splitlines()
    headers = []
    rows = []
    row_indices = []
    for idx, line in enumerate(lines):
        if line.startswith("|"):
            cells = [c.strip() for c in line.strip("|").split("|")]
            if not headers:
                headers = cells
            elif all(set(c) <= set("-: ") for c in cells):
                continue
            else:
                rows.append(cells)
                row_indices.append(idx)
    return lines, headers, rows, row_indices


def format_row(cells: list[str]) -> str:
    return "| " + " | ".join(cells) + " |"


def search_parts(query: str = "", low_stock_only: bool = False):
    q = query.strip().lower()
    matches = []
    for md in sorted(INV_DIR.glob("*.md")):
        if md.stem in {"locations", "suppliers", "valuation"}:
            continue
        _, headers, rows, _ = parse_table_file(md)
        for r in rows:
            rec = {headers[i].lower(): r[i] for i in range(min(len(headers), len(r)))}
            qty = int(re.sub(r"[^0-9-]", "", rec.get("qty", "0")) or 0)
            min_q = int(re.sub(r"[^0-9-]", "", rec.get("min", "0")) or 0)
            is_low = qty <= min_q
            if low_stock_only and not is_low:
                continue
            blob = " ".join(r).lower()
            if not q or q in blob:
                rec["_file"] = md.name
                rec["_low_stock"] = is_low
                matches.append(rec)
    return matches


def adjust_stock(sku: str, delta: int, note: str = ""):
    for md in sorted(INV_DIR.glob("*.md")):
        lines, _, rows, row_indices = parse_table_file(md)
        for r, line_idx in zip(rows, row_indices):
            if r and r[0].strip().lower() == sku.strip().lower():
                old_qty = int(re.sub(r"[^0-9-]", "", r[4]) or 0)
                new_qty = max(0, old_qty + int(delta))
                r[4] = str(new_qty)
                lines[line_idx] = format_row(r)
                md.write_text("\n".join(lines) + "\n", encoding="utf-8")
                audit("adjust_stock", {"sku": sku, "delta": delta, "old_qty": old_qty, "new_qty": new_qty, "note": note})
                return {"sku": sku, "file": md.name, "old_qty": old_qty, "new_qty": new_qty, "note": note}
    raise KeyError(f"SKU not found: {sku}")


def read_project(name: str):
    slug = name if name.endswith(".md") else f"{name}.md"
    p = safe_resolve(PROJ_DIR / slug)
    return {"project": slug, "content": p.read_text(encoding="utf-8")}


def check_bom(project_name: str):
    proj = read_project(project_name)["content"]
    needed = {}
    for m in re.finditer(r"`([A-Z0-9_-]+)`\s*x(\d+)", proj):
        sku = m.group(1)
        needed[sku] = needed.get(sku, 0) + int(m.group(2))
    stock_map = {item["sku"]: int(re.sub(r"[^0-9-]", "", item.get("qty", "0")) or 0) for item in search_parts("")}
    report = []
    for sku, req in needed.items():
        avail = stock_map.get(sku, 0)
        report.append({"sku": sku, "needed": req, "available": avail, "ok": avail >= req})
    return {"project": project_name, "items": report}


TOOLS = [
    {"name": "search_parts", "description": "Search parts across inventory tables", "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}, "low_stock_only": {"type": "boolean"}}}},
    {"name": "adjust_stock", "description": "Adjust quantity of a part SKU", "inputSchema": {"type": "object", "required": ["sku", "delta"], "properties": {"sku": {"type": "string"}, "delta": {"type": "integer"}, "note": {"type": "string"}}}},
    {"name": "read_project", "description": "Read a project Markdown page and BOM", "inputSchema": {"type": "object", "required": ["name"], "properties": {"name": {"type": "string"}}}},
    {"name": "check_bom", "description": "Verify available inventory stock against a project BOM", "inputSchema": {"type": "object", "required": ["project_name"], "properties": {"project_name": {"type": "string"}}}},
]


def handle_request(msg: dict):
    mid = msg.get("id")
    method = msg.get("method")
    params = msg.get("params") or {}
    if method == "initialize":
        return {"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}}, "serverInfo": {"name": "benchbear-mcp-legacy", "version": "1.0.0"}}}
    if method == "tools/list":
        return {"jsonrpc": "2.0", "id": mid, "result": {"tools": TOOLS}}
    if method == "tools/call":
        tname = params.get("name")
        args = params.get("arguments") or {}
        try:
            if tname == "search_parts":
                out = search_parts(**args)
            elif tname == "adjust_stock":
                out = adjust_stock(**args)
            elif tname == "read_project":
                out = read_project(**args)
            elif tname == "check_bom":
                out = check_bom(**args)
            else:
                raise ValueError(f"Unknown tool: {tname}")
            return {"jsonrpc": "2.0", "id": mid, "result": {"content": [{"type": "text", "text": json.dumps(out, indent=2)}]}}
        except Exception as exc:
            return {"jsonrpc": "2.0", "id": mid, "result": {"content": [{"type": "text", "text": f"Error: {exc}"}], "isError": True}}
    if mid is not None:
        return {"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": f"Method not found: {method}"}}
    return None


def main() -> None:
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        resp = handle_request(json.loads(line))
        if resp is not None:
            sys.stdout.write(json.dumps(resp) + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
