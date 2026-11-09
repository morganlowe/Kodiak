# Maintainer: Frank <frank@local>
# Version is read dynamically from src-tauri/tauri.conf.json so PKGBUILD never drifts from the app version.

pkgname=kodiak
pkgver=$(sed -n 's/.*"version":[[:space:]]*"\([^"]*\)".*/\1/p' "${startdir}/src-tauri/tauri.conf.json" | head -n1)
pkgrel=1
pkgdesc="Local-First AI Task Delegation Workbench (Cloud Interpret/Verify + Local Ollama Execute)"
arch=('x86_64')
url="https://wiki.local"
license=('Apache-2.0')
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
  RUSTFLAGS="--remap-path-prefix=${startdir}=/usr/src/${pkgname}" cargo build --release --manifest-path src-tauri/Cargo.toml
}

package() {
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
  install -Dm644 "README.md" "${pkgdir}/usr/share/doc/${pkgname}/README.md"
  install -Dm644 "docs/DECISIONS.md" "${pkgdir}/usr/share/doc/${pkgname}/DECISIONS.md"
  install -Dm644 "docs/THEME.md" "${pkgdir}/usr/share/doc/${pkgname}/THEME.md"
}
