# Workflow Rule: Cross-Platform Build & GitHub Releases

Every release of Kodiak must be compiled, verified, packaged for both Linux and Windows, and published to GitHub with a clear, well-written release page (following the No Emojis Policy).

## Required Release Steps

1. **Compile and Test**:
   - Run `npm run build` (`tsc && vite build`).
   - Run `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`.
   - Run `cargo test --manifest-path src-tauri/Cargo.toml`.

2. **Bump Versions and Update Documentation**:
   - Synchronize `X.Y.Z` across `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json`, `src/main.ts`, `README.md`, and `CHANGELOG.md`.

3. **Build Local Linux Split Packages**:
   - Arch/CachyOS: `make pkg` (`kodiak` and `kodiak-projectswiki`).
   - Debian/Ubuntu: `./scripts/build_deb.sh` (`kodiak_*_amd64.deb` and `kodiak-projectswiki_*_all.deb`).
   - Record exact byte sizes and `sha256sum` digests for release notes.

4. **Push Commit and Annotated Tag (Triggers Linux + Windows CI)**:
   - Push `main` and annotated tag `vX.Y.Z` to `origin` (`git tag -a vX.Y.Z -m "Kodiak vX.Y.Z"`).
   - Never rewrite or force-push published tags; increment to the next patch version if CI/packaging fixes are needed.
   - Ensure `.github/workflows/release.yml` produces:
     - `ubuntu-22.04`: `Kodiak_<ver>_amd64.AppImage` and `Kodiak_<ver>_amd64.deb`.
     - `windows-latest`: both `Kodiak_<ver>_x64-without-wiki-setup.exe` (via `src-tauri/tauri.nowiki.conf.json`) and `Kodiak_<ver>_x64-with-wiki-setup.exe` (via `scripts/build_windows_wiki.py`).

5. **Publish Well-Written Release Page and Verify Assets**:
   - Update the GitHub Release (`gh release edit` / `gh release upload --clobber`) with the 4 local Arch/Debian packages and structured, zero-emoji release notes (overview, changes, test results, asset table with SHA-256 digests, and compare URL).
   - Wait for and verify both `CI` and `Release Build` GitHub Actions workflows complete with `success` and all Linux and Windows assets appear on the release page (`gh release view vX.Y.Z --json assets`).
