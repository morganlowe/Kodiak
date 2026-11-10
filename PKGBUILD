# Maintainer: Frank <frank@local>
# Version is read dynamically from src-tauri/tauri.conf.json so PKGBUILD never drifts from the app version.

pkgname=('kodiak' 'kodiak-projectswiki')
pkgver=$(sed -n 's/.*"version":[[:space:]]*"\([^"]*\)".*/\1/p' "${startdir}/src-tauri/tauri.conf.json" | head -n1)
pkgrel=1
pkgdesc="Local-First AI Task Delegation Workbench"
arch=('x86_64')
url="https://wiki.local"
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
    'kodiak-projectswiki: Local Projects Wiki and AI documentation hub component'
    'python-mkdocs-material: Material theme rendering for Projects Wiki'
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
  pkgdesc="Local Projects Wiki and AI documentation hub component for Kodiak"
  arch=('any')
  depends=('python')
  optdepends=(
    'python-mkdocs-material: Fast local rendering with Material for MkDocs'
  )

  cd "${startdir}"

  local dest="${pkgdir}/usr/share/kodiak/projectswiki"
  install -dm755 "${dest}"
  cp -r --no-preserve=ownership "ProjectsWiki/." "${dest}/"
  rm -rf "${dest}/venv" \
         "${dest}/__pycache__" \
         "${dest}/.archive_duplicates" \
         "${dest}/site" \
         "${dest}/projects_inventory.json" \
         "${dest}/docs/projects.json" \
         "${dest}/project_overrides.json"

  chmod +x "${dest}/wiki.sh"
  chmod +x "${dest}"/*.py 2>/dev/null || true

  # CLI wrapper in /usr/bin/wiki
  install -dm755 "${pkgdir}/usr/bin"
  ln -sf "/usr/share/kodiak/projectswiki/wiki.sh" "${pkgdir}/usr/bin/wiki"

  # Systemd user service unit (shipped, not auto-enabled)
  install -dm755 "${pkgdir}/usr/lib/systemd/user"
  cat > "${pkgdir}/usr/lib/systemd/user/projects-wiki.service" << 'EOF'
[Unit]
Description=ProjectsWiki Local Server (http://wiki.local)
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
