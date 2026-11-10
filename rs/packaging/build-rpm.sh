#!/bin/bash
# 打 Fedora / RHEL 的 rpm：源码打包 → rpmbuild → 产物落到 out/
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# 本机 /tmp 是 10MB 的 tmpfs，rpmbuild 会把整个 target 编进去，必须挪走
export TMPDIR="${TMPDIR:-/var/tmp}"
cd "$ROOT"

VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
NAME="carboy"
STAGE="$(mktemp -d)"
OUT="$ROOT/../out"
mkdir -p "$OUT"

echo "==> 准备源码树 carboy-$VERSION"
mkdir -p "$STAGE/$NAME-$VERSION"
cp -r Cargo.toml Cargo.lock src data LICENSE packaging "$STAGE/$NAME-$VERSION/"
# 不带 target / .cargo，避免把几 GB 编进源码包
tar -C "$STAGE" -czf "$STAGE/$NAME-$VERSION.tar.gz" "$NAME-$VERSION"
echo "    源码包 $(du -h "$STAGE/$NAME-$VERSION.tar.gz" | cut -f1)"

echo "==> rpmbuild"
rpmbuild -bb packaging/carboy.spec \
  --define "_sourcedir $STAGE" \
  --define "_topdir $STAGE/rpm" \
  --define "_tmppath /var/tmp" \
  --define "_build_id_links none" \
  --define "debug_package %{nil}" \
  --define "carboy_offline 1" \
  > "$STAGE/rpmbuild.log" 2>&1 || {
    echo "rpmbuild 失败，最后 30 行："
    tail -30 "$STAGE/rpmbuild.log"
    exit 1
  }

find "$STAGE/rpm/RPMS" -name '*.rpm' -exec cp -v {} "$OUT/" \;
echo "==> 完成：$(ls "$OUT"/*.rpm 2>/dev/null | wc -l) 个 rpm"
rm -rf "$STAGE"
