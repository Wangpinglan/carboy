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
# brp-mangle-shebangs 默认把 #!/bin/sh 改写成 #!/usr/bin/sh，再用 touch 改回
# 时间戳。winetricks 是上游原样分发的，一旦被改写，它自报的 sha256 就和实际
# 文件对不上（`winetricks --version` 直接打这串哈希），所以给它加 --files 排除。
#
# rpmbuild 没有"追加一条宏文件"的开关（`--macros` 是整份替换），所以临时往
# ~/.config/rpm/macros 里追加一段宿主 __os_install_post 的副本（只多了那个
# --files），跑完还原成原样。宿主上 touch 又被禁（EPERM），不改写也顺带绕开了
# 那步失败。换机器打 rpm 时，这段副本要跟宿主 `rpm --eval '%{__os_install_post}'`
# 对一遍。
USER_MACROS="${XDG_CONFIG_HOME:-$HOME/.config}/rpm/macros"
mkdir -p "$(dirname "$USER_MACROS")"
HAD_MACROS=0
[ -f "$USER_MACROS" ] && { cp "$USER_MACROS" "$STAGE/macros.bak"; HAD_MACROS=1; }
cat >> "$USER_MACROS" <<'MACRO'

# --- carboy build-rpm.sh 临时追加，跑完删除 ---
%__os_install_post %{expand:
    /usr/lib/rpm/redhat/brp-ldconfig
    COMPRESS="gzip -9 -n" COMPRESS_EXT=".gz" /usr/lib/rpm/brp-compress
    /usr/lib/rpm/brp-strip /usr/bin/strip
    /usr/lib/rpm/brp-strip-comment-note /usr/bin/strip /usr/bin/objdump
    /usr/lib/rpm/redhat/brp-strip-lto /usr/bin/strip
    /usr/lib/rpm/check-rpaths
    /usr/lib/rpm/redhat/brp-mangle-shebangs --files carboy/winetricks
    /usr/lib/rpm/brp-remove-la-files
    /usr/lib/rpm/redhat/brp-python-rpm-in-distinfo
    env /usr/lib/rpm/redhat/brp-python-bytecompile "" "1" "0" "-j%{?_smp_build_ncpus}"
    /usr/lib/rpm/redhat/brp-python-hardlink
    /usr/bin/add-det --brp -j%{?_smp_build_ncpus} %{buildroot}
}
MACRO

restore_macros() {
  if [ "$HAD_MACROS" = 1 ]; then
    cp "$STAGE/macros.bak" "$USER_MACROS"
  else
    rm -f "$USER_MACROS"
  fi
}
trap restore_macros EXIT

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

restore_macros
trap - EXIT

find "$STAGE/rpm/RPMS" -name '*.rpm' -exec cp -v {} "$OUT/" \;
echo "==> 完成：$(ls "$OUT"/*.rpm 2>/dev/null | wc -l) 个 rpm"
rm -rf "$STAGE"
