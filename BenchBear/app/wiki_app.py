#!/usr/bin/env python3
"""
app/wiki_app.py - Entrypoint for the standalone BenchBear Inventory & Barcode Hub.
Delegates to the compiled Rust binary (`target/release/benchbear serve`) when available.
"""
import os
import pathlib
import subprocess
import sys

WIKI_ROOT = pathlib.Path(__file__).resolve().parent.parent
RUST_BIN = WIKI_ROOT / "target" / "release" / "benchbear"

if __name__ == "__main__":
    if RUST_BIN.is_file() and os.access(RUST_BIN, os.X_OK):
        os.execv(
            str(RUST_BIN),
            [str(RUST_BIN), "--wiki-root", str(WIKI_ROOT), "serve", *sys.argv[1:]],
        )
    print("[!] Rust binary not built yet. Building via `cargo build --release`...")
    subprocess.run(["cargo", "build", "--release"], cwd=str(WIKI_ROOT), check=True)
    os.execv(
        str(RUST_BIN),
        [str(RUST_BIN), "--wiki-root", str(WIKI_ROOT), "serve", *sys.argv[1:]],
    )
