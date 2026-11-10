#!/bin/bash
# 打 Debian / Ubuntu 的 deb。
# 本机是 Fedora，没有 dpkg-deb，所以直接按 deb 格式手工组装：
#   ar( debian-binary, control.tar.gz, data.tar.gz )
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
ARCH="amd64"
OUT="$ROOT/../out"
mkdir -p "$OUT"

BIN="target/release/carboy"
if [ ! -x "$BIN" ]; then
  echo "==> 编译 release"
  cargo build --release --offline
fi

STAGE="$(mktemp -d)"
PKG="$STAGE/pkg"
install -Dm755 "$BIN" "$PKG/usr/bin/carboy"
install -Dm644 data/io.github.carboy.MkDesktop.desktop \
  "$PKG/usr/share/applications/io.github.carboy.MkDesktop.desktop"
install -Dm644 data/io.github.carboy.MkDesktop.svg \
  "$PKG/usr/share/icons/hicolor/scalable/apps/io.github.carboy.MkDesktop.svg"
install -Dm644 data/io.github.carboy.MkDesktop.metainfo.xml \
  "$PKG/usr/share/metainfo/io.github.carboy.MkDesktop.metainfo.xml"
install -Dm644 LICENSE "$PKG/usr/share/doc/carboy/copyright"

mkdir -p "$PKG/DEBIAN"
SIZE="$(du -sk "$PKG/usr" | cut -f1)"
cat > "$PKG/DEBIAN/control" <<EOF
Package: carboy
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Installed-Size: $SIZE
Depends: libgtk-4-1 (>= 4.10), libadwaita-1-0 (>= 1.5), libglib2.0-0
Maintainer: Carboy contributors <carboy@example.invalid>
Homepage: https://github.com/carboy/carboy
Description: Wine / Proton 应用的 .desktop 生成器
 自动推导窗口标识（StartupWMClass），从 exe 的 PE 资源里抠图标，
 装进 hicolor 多档尺寸并刷新缓存。生成的桌面项能被 GNOME / KDE /
 XFCE 正确识别，不再出现齿轮占位图标或任务栏合并。
EOF

( cd "$PKG" && find usr -type f -exec md5sum {} + > DEBIAN/md5sums )

echo "==> 组装 deb"
( cd "$PKG" && tar --owner=0 --group=0 --numeric-owner -czf "$STAGE/data.tar.gz" usr )
( cd "$PKG/DEBIAN" && tar --owner=0 --group=0 --numeric-owner -czf "$STAGE/control.tar.gz" . )
printf '2.0\n' > "$STAGE/debian-binary"

DEB="$OUT/carboy_${VERSION}_${ARCH}.deb"
rm -f "$DEB"
( cd "$STAGE" && ar rc "$DEB" debian-binary control.tar.gz data.tar.gz )

echo "==> 内容："
ar t "$DEB" | sed 's/^/    /'
echo "==> 完成：$DEB（$(du -h "$DEB" | cut -f1)）"
rm -rf "$STAGE"
