# Project Rules

## 1. No Emojis Policy
- **STRICT RULE**: Do NOT use emojis anywhere in this project.
- Applies to all UI elements, HTML templates, button labels, toasts, badges, documentation (README, CHANGELOG, DECISIONS), commit messages, release descriptions, and tool outputs.
- Replace any emojis with plain text labels, standard typography, or CSS styling.

## 2. Build & Release Policy (Linux & Windows)
- **ALWAYS COMPILE & VERIFY**: Before every release commit and tag, run `npm run build`, `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`, and `cargo test --manifest-path src-tauri/Cargo.toml`, and confirm all checks pass cleanly.
- **ALWAYS SYNC VERSIONS & DOCS**: Bump semantic versions (`X.Y.Z`) consistently across `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`, About/UI version labels in `src/main.ts`, `README.md`, and `CHANGELOG.md`.
- **BUILD FOR LINUX & WINDOWS**:
  - **Linux Split Packages**: Build local Arch packages (`make pkg`) and Debian packages (`./scripts/build_deb.sh`), producing core `kodiak` and companion `kodiak-projectswiki` packages.
  - **GitHub Actions Cross-Platform Bundles (`.github/workflows/release.yml`)**: Triggered by pushing an annotated release tag (`vX.Y.Z`) to build and upload:
    - **Ubuntu Linux (`ubuntu-22.04`)**: `Kodiak_<ver>_amd64.AppImage` and `Kodiak_<ver>_amd64.deb`.
    - **Windows x64 (`windows-latest`)**: **BOTH** NSIS setup installers (staged outside Vite's `dist/` directory in `$env:RUNNER_TEMP\windows-releases`):
      - `Kodiak_<ver>_x64-without-wiki-setup.exe` (built with `--config src-tauri/tauri.nowiki.conf.json` so `bundle.resources` is empty).
      - `Kodiak_<ver>_x64-with-wiki-setup.exe` (built after freezing `kodiak-projectswiki.exe` via `scripts/build_windows_wiki.py`).
- **PUSH & PUBLISH WELL-WRITTEN RELEASE PAGE**:
  - Commit release changes on `main`, create an annotated tag (`git tag -a vX.Y.Z -m "Kodiak vX.Y.Z"`), and push both `main` and `vX.Y.Z` to `origin`. Never force-push or move an already published tag; advance to the next patch version for follow-up fixes.
  - Attach the verified local Arch/Debian packages and publish well-written, structured, zero-emoji release notes on the GitHub Release page (clear title, executive summary, categorized change list, verification summary, full asset and SHA-256 table covering Linux and Windows variants, and GitHub compare link).
  - Verify `CI` and `Release Build` runs finish with `success` and confirm all Linux and Windows release assets (including both Windows installers with and without `ProjectsWiki`) are uploaded to the release page.
