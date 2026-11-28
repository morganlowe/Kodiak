# Maintainer: Morgan Lowe
# Version is read dynamically from src-tauri/tauri.conf.json so PKGBUILD never drifts from the app version.

pkgname=('kodiak' 'kodiak-projectswiki')
pkgver=$(sed -n 's/.*"version":[[:space:]]*"\([^"]*\)".*/\1/p' "${startdir}/src-tauri/tauri.conf.json" | head -n1)
pkgrel=1
pkgdesc="Local-First AI Task Delegation Workbench"
arch=('x86_64')
url="https://github.com/morganlowe/Kodiak"
license=('Apache-2.0')
makedepends=(
  'rust'
  'nodejs'
  'npm'
  'pkgconf'
  'patchelf'
)
options=('!lto' '!debug')

pkgver() {
  sed -n 's/.*"version":[[:space:]]*"\([^"]*\)".*/\1/p' "${startdir}/src-tauri/tauri.conf.json" | head -n1
}

build() {
  cd "${startdir}"
  if [[ ! -d node_modules ]]; then
    npm install --no-audit --no-fund
  fi
  npm run build
  RUSTFLAGS="--remap-path-prefix=${startdir}=/usr/src/kodiak" cargo build --release --manifest-path src-tauri/Cargo.toml
  RUSTFLAGS="--remap-path-prefix=${startdir}=/usr/src/kodiak" cargo build --release --manifest-path ProjectsWiki/Cargo.toml
}

package_kodiak() {
  pkgdesc="Local-First AI Task Delegation Workbench (Cloud Interpret/Verify + Local Ollama Execute)"
  conflicts=('foreman')
  replaces=('foreman')
  depends=(
    'webkit2gtk-4.1'
    'gtk3'
    'libayatana-appindicator'
    'librsvg'
    'openssl'
    'gcc-libs'
    'glibc'
    'hicolor-icon-theme'
  )
  optdepends=(
    'kodiak-projectswiki: BenchBear project wiki and hardware inventory companion'
    'python-mkdocs-material: Material theme rendering for BenchBear'
  )

  cd "${startdir}"

  # Binary
  install -Dm755 "src-tauri/target/release/kodiak" "${pkgdir}/usr/bin/kodiak"

  # Icons (SVG + PNG sizes)
  install -Dm644 "src/assets/kodiak-icon.svg" "${pkgdir}/usr/share/icons/hicolor/scalable/apps/kodiak.svg"
  install -Dm644 "src-tauri/icons/32x32.png" "${pkgdir}/usr/share/icons/hicolor/32x32/apps/kodiak.png"
  install -Dm644 "src-tauri/icons/128x128.png" "${pkgdir}/usr/share/icons/hicolor/128x128/apps/kodiak.png"
  install -Dm644 "src-tauri/icons/128x128@2x.png" "${pkgdir}/usr/share/icons/hicolor/256x256@2/apps/kodiak.png"
  install -Dm644 "src-tauri/icons/icon.png" "${pkgdir}/usr/share/icons/hicolor/512x512/apps/kodiak.png"

  # Desktop entry
  install -dm755 "${pkgdir}/usr/share/applications"
  cat > "${pkgdir}/usr/share/applications/kodiak.desktop" << 'EOF'
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
  install -Dm644 "README.md" "${pkgdir}/usr/share/doc/kodiak/README.md"
  install -Dm644 "CHANGELOG.md" "${pkgdir}/usr/share/doc/kodiak/CHANGELOG.md"
  install -Dm644 "docs/DECISIONS.md" "${pkgdir}/usr/share/doc/kodiak/DECISIONS.md"
  install -Dm644 "docs/THEME.md" "${pkgdir}/usr/share/doc/kodiak/THEME.md"
}

package_kodiak-projectswiki() {
  license=('Apache-2.0' 'GPL-3.0-only')
  pkgdesc="BenchBear project wiki and hardware inventory companion for Kodiak"
  arch=('x86_64')
  depends=('python' 'gcc-libs' 'glibc')
  optdepends=(
    'python-mkdocs-material: Fast local rendering with Material for MkDocs'
    'zbar: Optional 1D barcode image decoding'
  )

  cd "${startdir}"

  local dest="${pkgdir}/usr/share/kodiak/projectswiki"
  install -dm755 "${dest}"
  tar -C "ProjectsWiki" \
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
    -cf - . | tar -C "${dest}" --no-same-owner -xf -
  find "${dest}/docs" -type f -name "*.md" ! -name "index.md" ! -name "demo-project.md" ! -name "inventory-hub.md" -delete

  install -Dm755 "ProjectsWiki/target/release/benchbear" "${dest}/benchbear"

  chmod +x "${dest}/wiki.sh"
  chmod +x "${dest}"/*.py "${dest}/app/wiki_app.py" "${dest}/mcp/server.py" 2>/dev/null || true

  # CLI wrapper in /usr/bin/wiki
  install -dm755 "${pkgdir}/usr/bin"
  ln -sf "/usr/share/kodiak/projectswiki/wiki.sh" "${pkgdir}/usr/bin/wiki"

  # Systemd user service unit (shipped, not auto-enabled)
  install -dm755 "${pkgdir}/usr/lib/systemd/user"
  cat > "${pkgdir}/usr/lib/systemd/user/projects-wiki.service" << 'EOF'
[Unit]
Description=BenchBear Local Server (http://benchbear.localhost)
After=network.target

[Service]
Type=simple
WorkingDirectory=/usr/share/kodiak/projectswiki
ExecStart=/usr/share/kodiak/projectswiki/wiki.sh daemon
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
EOF

  # Example configuration in /usr/share/doc/kodiak-projectswiki/
  install -Dm644 "ProjectsWiki/config.example.toml" "${pkgdir}/usr/share/doc/kodiak-projectswiki/config.example.toml"
  install -Dm644 "ProjectsWiki/project_overrides.example.json" "${pkgdir}/usr/share/doc/kodiak-projectswiki/project_overrides.example.json"
}
