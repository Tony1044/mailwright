#!/bin/sh
# Build the Ubuntu/Debian package: packaging/linux/package.sh <path to release binary> <out dir>
set -eu
BIN=${1:-target/release/mailwright}
OUT=${2:-dist}
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
VERSION=$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)"/\1/')
ARCH=amd64
PKG=$(mktemp -d)
install -Dm755 "$BIN" "$PKG/usr/bin/mailwright"
install -Dm644 "$ROOT/packaging/linux/ai.storyteller.mailwright.desktop" "$PKG/usr/share/applications/ai.storyteller.mailwright.desktop"
install -Dm644 "$ROOT/packaging/linux/ai.storyteller.mailwright.metainfo.xml" "$PKG/usr/share/metainfo/ai.storyteller.mailwright.metainfo.xml"
for s in 16 24 32 48 64 128 256 512; do
  install -Dm644 "$ROOT/assets/app-icon/mailwright-$s.png" "$PKG/usr/share/icons/hicolor/${s}x${s}/apps/ai.storyteller.mailwright.png"
done
install -Dm644 "$ROOT/LICENSE-MIT" "$PKG/usr/share/doc/mailwright/LICENSE-MIT"
install -Dm644 "$ROOT/LICENSE-APACHE" "$PKG/usr/share/doc/mailwright/LICENSE-APACHE"
install -Dm644 "$ROOT/assets/fonts/OFL-Inter.txt" "$PKG/usr/share/doc/mailwright/fonts/OFL-Inter.txt"
install -Dm644 "$ROOT/assets/fonts/OFL-SourceSans3.txt" "$PKG/usr/share/doc/mailwright/fonts/OFL-SourceSans3.txt"
install -Dm644 "$ROOT/assets/fonts/OFL-SourceSerif4.txt" "$PKG/usr/share/doc/mailwright/fonts/OFL-SourceSerif4.txt"
install -Dm644 "$ROOT/assets/fonts/OFL-JetBrainsMono.txt" "$PKG/usr/share/doc/mailwright/fonts/OFL-JetBrainsMono.txt"
install -Dm644 "$ROOT/docs/SETUP.md" "$PKG/usr/share/doc/mailwright/SETUP.md"
SIZE=$(du -sk "$PKG/usr" | cut -f1)
mkdir -p "$PKG/DEBIAN"
cat > "$PKG/DEBIAN/control" <<CTRL
Package: mailwright
Version: $VERSION
Section: mail
Priority: optional
Architecture: $ARCH
Installed-Size: $SIZE
Depends: libc6 (>= 2.39), libgcc-s1, libwebkit2gtk-4.1-0, libgtk-3-0, libxkbcommon0, libwayland-client0, libx11-6, libxcursor1, libxi6, libxrandr2, libgl1 | libvulkan1, libdbus-1-3
Recommends: xdg-desktop-portal, gnome-keyring | kwalletmanager, sound-theme-freedesktop, pulseaudio-utils | pipewire-bin
Conflicts: mailcraft
Replaces: mailcraft
Maintainer: Mailwright <mailwright@storyteller.ai>
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
dpkg-deb --root-owner-group --build "$PKG" "$OUT/mailwright_${VERSION}-1_${ARCH}.deb"
rm -rf "$PKG"
echo "$OUT/mailwright_${VERSION}-1_${ARCH}.deb"
