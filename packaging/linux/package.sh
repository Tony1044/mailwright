#!/bin/sh
# Build the Ubuntu/Debian package: packaging/linux/package.sh <path to release binary> <out dir>
set -eu
BIN=${1:-target/release/mailcraft}
OUT=${2:-dist}
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
VERSION=$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)"/\1/')
ARCH=amd64
PKG=$(mktemp -d)
install -Dm755 "$BIN" "$PKG/usr/bin/mailcraft"
install -Dm644 "$ROOT/packaging/linux/ai.storyteller.mailcraft.desktop" "$PKG/usr/share/applications/ai.storyteller.mailcraft.desktop"
install -Dm644 "$ROOT/packaging/linux/ai.storyteller.mailcraft.metainfo.xml" "$PKG/usr/share/metainfo/ai.storyteller.mailcraft.metainfo.xml"
for s in 16 24 32 48 64 128 256 512; do
  install -Dm644 "$ROOT/assets/app-icon/mailcraft-$s.png" "$PKG/usr/share/icons/hicolor/${s}x${s}/apps/ai.storyteller.mailcraft.png"
done
install -Dm644 "$ROOT/LICENSE-MIT" "$PKG/usr/share/doc/mailcraft/LICENSE-MIT"
install -Dm644 "$ROOT/LICENSE-APACHE" "$PKG/usr/share/doc/mailcraft/LICENSE-APACHE"
install -Dm644 "$ROOT/assets/fonts/OFL-Inter.txt" "$PKG/usr/share/doc/mailcraft/fonts/OFL-Inter.txt"
install -Dm644 "$ROOT/assets/fonts/OFL-SourceSans3.txt" "$PKG/usr/share/doc/mailcraft/fonts/OFL-SourceSans3.txt"
install -Dm644 "$ROOT/assets/fonts/OFL-SourceSerif4.txt" "$PKG/usr/share/doc/mailcraft/fonts/OFL-SourceSerif4.txt"
install -Dm644 "$ROOT/assets/fonts/OFL-JetBrainsMono.txt" "$PKG/usr/share/doc/mailcraft/fonts/OFL-JetBrainsMono.txt"
install -Dm644 "$ROOT/docs/SETUP.md" "$PKG/usr/share/doc/mailcraft/SETUP.md"
SIZE=$(du -sk "$PKG/usr" | cut -f1)
mkdir -p "$PKG/DEBIAN"
cat > "$PKG/DEBIAN/control" <<CTRL
Package: mailcraft
Version: $VERSION
Section: mail
Priority: optional
Architecture: $ARCH
Installed-Size: $SIZE
Depends: libc6 (>= 2.39), libgcc-s1, libwebkit2gtk-4.1-0, libgtk-3-0, libxkbcommon0, libwayland-client0, libx11-6, libxcursor1, libxi6, libxrandr2, libgl1 | libvulkan1, libdbus-1-3
Recommends: xdg-desktop-portal, gnome-keyring | kwalletmanager
Maintainer: MailCraft <mailcraft@storyteller.ai>
Homepage: https://github.com/storytold
Description: Mail and calendar for Microsoft 365 (Exchange Online)
 A classic desktop mail and calendar client that works with Exchange Online
 through Microsoft Graph. Recent mail is kept on this computer for reading
 and searching offline; changes made offline are sent when back online.
CTRL
cat > "$PKG/DEBIAN/postinst" <<'POST'
#!/bin/sh
set -e
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q /usr/share/applications || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t /usr/share/icons/hicolor || true
POST
chmod 755 "$PKG/DEBIAN/postinst"
mkdir -p "$OUT"
dpkg-deb --root-owner-group --build "$PKG" "$OUT/mailcraft_${VERSION}-1_${ARCH}.deb"
rm -rf "$PKG"
echo "$OUT/mailcraft_${VERSION}-1_${ARCH}.deb"
