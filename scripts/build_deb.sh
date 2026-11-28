#!/usr/bin/env bash
# ==============================================================================
# Debian/Ubuntu Packaging Script for Kodiak & kodiak-benchbear
# Builds clean, standard .deb packages using dpkg-deb
# ==============================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION=$(sed -n 's/.*"version":[[:space:]]*"\([^"]*\)".*/\1/p' "$ROOT_DIR/src-tauri/tauri.conf.json" | head -n1)
RELEASE="1"
DIST_DIR="$ROOT_DIR/dist/deb"
BUNDLE_DEB_DIR="$ROOT_DIR/src-tauri/target/release/bundle/deb"

echo ":: [Debian Build] Packaging Kodiak v${VERSION}-${RELEASE}..."

mkdir -p "$DIST_DIR" "$BUNDLE_DEB_DIR"
BUILD_TMP="/tmp/kodiak-deb-build"
rm -rf "$BUILD_TMP"
mkdir -p "$BUILD_TMP"

# ------------------------------------------------------------------------------
# 1. Package: kodiak (amd64)
# ------------------------------------------------------------------------------
KODIAK_STAGE="$BUILD_TMP/kodiak_${VERSION}-${RELEASE}_amd64"
mkdir -p "$KODIAK_STAGE/DEBIAN" \
         "$KODIAK_STAGE/usr/bin" \
         "$KODIAK_STAGE/usr/share/applications" \
         "$KODIAK_STAGE/usr/share/doc/kodiak" \
         "$KODIAK_STAGE/usr/share/icons/hicolor/scalable/apps" \
         "$KODIAK_STAGE/usr/share/icons/hicolor/32x32/apps" \
         "$KODIAK_STAGE/usr/share/icons/hicolor/128x128/apps" \
         "$KODIAK_STAGE/usr/share/icons/hicolor/256x256@2/apps" \
         "$KODIAK_STAGE/usr/share/icons/hicolor/512x512/apps"

# Binary
install -Dm755 "$ROOT_DIR/src-tauri/target/release/kodiak" "$KODIAK_STAGE/usr/bin/kodiak"

# Desktop launcher & icons
install -Dm644 "$ROOT_DIR/src/assets/kodiak-icon.svg" "$KODIAK_STAGE/usr/share/icons/hicolor/scalable/apps/kodiak.svg"
install -Dm644 "$ROOT_DIR/src-tauri/icons/32x32.png" "$KODIAK_STAGE/usr/share/icons/hicolor/32x32/apps/kodiak.png"
install -Dm644 "$ROOT_DIR/src-tauri/icons/128x128.png" "$KODIAK_STAGE/usr/share/icons/hicolor/128x128/apps/kodiak.png"
install -Dm644 "$ROOT_DIR/src-tauri/icons/128x128@2x.png" "$KODIAK_STAGE/usr/share/icons/hicolor/256x256@2/apps/kodiak.png"
install -Dm644 "$ROOT_DIR/src-tauri/icons/icon.png" "$KODIAK_STAGE/usr/share/icons/hicolor/512x512/apps/kodiak.png"

cat > "$KODIAK_STAGE/usr/share/applications/kodiak.desktop" << 'EOF'
[Desktop Entry]
Name=Kodiak
Comment=Local-First AI Task Delegation Workbench
Exec=/usr/bin/kodiak
Icon=kodiak
Terminal=false
Type=Application
Categories=Development;Utility;
StartupWMClass=kodiak
EOF

# Documentation
install -Dm644 "$ROOT_DIR/README.md" "$KODIAK_STAGE/usr/share/doc/kodiak/README.md"
install -Dm644 "$ROOT_DIR/CHANGELOG.md" "$KODIAK_STAGE/usr/share/doc/kodiak/CHANGELOG.md"
install -Dm644 "$ROOT_DIR/docs/DECISIONS.md" "$KODIAK_STAGE/usr/share/doc/kodiak/DECISIONS.md"
install -Dm644 "$ROOT_DIR/docs/THEME.md" "$KODIAK_STAGE/usr/share/doc/kodiak/THEME.md"

cat > "$KODIAK_STAGE/DEBIAN/control" << EOF
Package: kodiak
Version: ${VERSION}-${RELEASE}
Section: devel
Priority: optional
Architecture: amd64
Maintainer: Morgan Lowe <morgan@local>
Depends: libwebkit2gtk-4.1-0, libgtk-3-0, libayatana-appindicator3-1, librsvg2-common, openssl
Recommends: kodiak-benchbear, python3
Conflicts: foreman
Replaces: foreman
Description: Local-First AI Task Delegation Workbench
 Kodiak is a local-first AI task delegation workbench pairing cloud planning
 with local Ollama execution, strict filesystem boundaries, and human oversight.
EOF

# Fix permissions
find "$KODIAK_STAGE" -type d -exec chmod 755 {} +
chmod 755 "$KODIAK_STAGE/usr/bin/kodiak"
chmod 755 "$KODIAK_STAGE/DEBIAN"
dpkg-deb --build --root-owner-group "$KODIAK_STAGE" "$DIST_DIR/kodiak_${VERSION}-${RELEASE}_amd64.deb"
cp -f "$DIST_DIR/kodiak_${VERSION}-${RELEASE}_amd64.deb" "$BUNDLE_DEB_DIR/"
echo ":: [Debian Build] Built $DIST_DIR/kodiak_${VERSION}-${RELEASE}_amd64.deb"

# ------------------------------------------------------------------------------
# 2. Package: kodiak-benchbear (amd64)
# ------------------------------------------------------------------------------
echo ":: [Debian Build] Building BenchBear inventory engine..."
cargo build --release --manifest-path "$ROOT_DIR/BenchBear/Cargo.toml"

WIKI_STAGE="$BUILD_TMP/kodiak-benchbear_${VERSION}-${RELEASE}_amd64"
mkdir -p "$WIKI_STAGE/DEBIAN" \
         "$WIKI_STAGE/usr/bin" \
         "$WIKI_STAGE/usr/lib/systemd/user" \
         "$WIKI_STAGE/usr/share/doc/kodiak-benchbear" \
         "$WIKI_STAGE/usr/share/kodiak/benchbear"

tar -C "$ROOT_DIR/BenchBear" \
    --exclude=target \
    --exclude=venv \
    --exclude=__pycache__ \
    --exclude=.archive_duplicates \
    --exclude=site \
    --exclude=data \
    --exclude=inventory \
    --exclude=projects \
    --exclude=docs/inventory \
    --exclude=docs/projects \
    --exclude=projects_inventory.json \
    --exclude=docs/projects.json \
    --exclude=project_overrides.json \
    -cf - . | tar -C "$WIKI_STAGE/usr/share/kodiak/benchbear" --no-same-owner -xf -
find "$WIKI_STAGE/usr/share/kodiak/benchbear/docs" -type f -name "*.md" ! -name "index.md" ! -name "demo-project.md" ! -name "inventory-hub.md" -delete

install -Dm755 "$ROOT_DIR/BenchBear/target/release/benchbear" "$WIKI_STAGE/usr/share/kodiak/benchbear/benchbear"

chmod +x "$WIKI_STAGE/usr/share/kodiak/benchbear/wiki.sh"
chmod +x "$WIKI_STAGE/usr/share/kodiak/benchbear"/*.py "$WIKI_STAGE/usr/share/kodiak/benchbear/app/wiki_app.py" "$WIKI_STAGE/usr/share/kodiak/benchbear/mcp/server.py" 2>/dev/null || true

# Symlink CLI wrappers
ln -sf "/usr/share/kodiak/benchbear/wiki.sh" "$WIKI_STAGE/usr/bin/benchbear-wiki"
ln -sf "/usr/share/kodiak/benchbear/wiki.sh" "$WIKI_STAGE/usr/bin/wiki"

# Systemd user unit (shipped, not enabled by maintainer scripts)
cat > "$WIKI_STAGE/usr/lib/systemd/user/benchbear.service" << 'EOF'
[Unit]
Description=BenchBear Local Server (http://benchbear.localhost)
After=network.target

[Service]
Type=simple
WorkingDirectory=/usr/share/kodiak/benchbear
ExecStart=/usr/share/kodiak/benchbear/wiki.sh daemon
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
EOF

# Example configurations
install -Dm644 "$ROOT_DIR/BenchBear/config.example.toml" "$WIKI_STAGE/usr/share/doc/kodiak-benchbear/config.example.toml"
install -Dm644 "$ROOT_DIR/BenchBear/project_overrides.example.json" "$WIKI_STAGE/usr/share/doc/kodiak-benchbear/project_overrides.example.json"

cat > "$WIKI_STAGE/DEBIAN/control" << EOF
Package: kodiak-benchbear
Version: ${VERSION}-${RELEASE}
Section: doc
Priority: optional
Architecture: amd64
Maintainer: Morgan Lowe <morgan@local>
Depends: python3, libc6, libgcc-s1
Recommends: python3-yaml
Suggests: kodiak, zbar-tools
Conflicts: kodiak-projectswiki
Replaces: kodiak-projectswiki
Description: Local BenchBear project wiki and hardware inventory companion for Kodiak
 Interactive MkDocs Material documentation hub, project explorer, BenchBear
 inventory/barcode engine, and local Ollama-powered documentation builder.
EOF

# Fix permissions
find "$WIKI_STAGE" -type d -exec chmod 755 {} +
chmod 755 "$WIKI_STAGE/usr/share/kodiak/benchbear/wiki.sh"
chmod 755 "$WIKI_STAGE/usr/share/kodiak/benchbear/benchbear"
chmod 755 "$WIKI_STAGE/usr/share/kodiak/benchbear"/*.py "$WIKI_STAGE/usr/share/kodiak/benchbear/app/wiki_app.py" "$WIKI_STAGE/usr/share/kodiak/benchbear/mcp/server.py" 2>/dev/null || true
chmod 755 "$WIKI_STAGE/DEBIAN"
dpkg-deb --build --root-owner-group "$WIKI_STAGE" "$DIST_DIR/kodiak-benchbear_${VERSION}-${RELEASE}_amd64.deb"
cp -f "$DIST_DIR/kodiak-benchbear_${VERSION}-${RELEASE}_amd64.deb" "$BUNDLE_DEB_DIR/"
echo ":: [Debian Build] Built $DIST_DIR/kodiak-benchbear_${VERSION}-${RELEASE}_amd64.deb"

# Clean build temporary folder
rm -rf "$BUILD_TMP"

echo ":: [Debian Build] Complete! Packages available in $DIST_DIR:"
ls -lh "$DIST_DIR"/*.deb
