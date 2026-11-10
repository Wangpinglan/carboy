#!/usr/bin/env bash
# Carboy 参考源码一键克隆
# 用法: ./clone-refs.sh [p0|p1|p2|all]      默认 p0
#       MIRROR=https://gh-proxy.com/ ./clone-refs.sh p0     # 走镜像
#       DEST=/some/dir ./clone-refs.sh all
#
# 说明:
#   - 全部 --depth 1，不拉历史
#   - wine 不在列表里 —— 本地 Proton 仓库的 wine 子模块就是 Valve 的 fork，
#     比上游更贴近实际运行环境，直接读 ~/项目/LinkToProton/wine 即可
#   - 大仓库用 sparse-checkout，只取需要读的目录
#   - 幂等：已存在的跳过

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST="${DEST:-$HERE/repos}"
MIRROR="${MIRROR:-}"

log()  { printf '%s\n' "$*"; }
ok()   { printf '   ok    %s\n' "$*"; }
skip() { printf '   skip  %s\n' "$*"; }
fail() { printf '   FAIL  %s\n' "$*"; }

# 仅对 github.com 的地址套镜像前缀；gitlab 一般直连更快
mir() {
  case "$1" in
    https://github.com/*) printf '%s%s' "$MIRROR" "$1" ;;
    *) printf '%s' "$1" ;;
  esac
}

command -v git >/dev/null || { log "没有 git，先装 git"; exit 1; }
mkdir -p "$DEST"
[ -n "$MIRROR" ] && log "镜像: $MIRROR"
log "目标: $DEST"
log ""

clone_full() {
  local name=$1 url=$2
  if [ -d "$DEST/$name/.git" ]; then skip "$name"; return 0; fi
  log " > $name"
  if git clone --depth 1 --single-branch "$(mir "$url")" "$DEST/$name" >/dev/null 2>&1; then
    ok "$name"
  else
    fail "$name  <- $url"
    log "      （可试 MIRROR=https://gh-proxy.com/ 重跑）"
  fi
}

clone_sparse() {
  local name=$1 url=$2; shift 2
  if [ -d "$DEST/$name/.git" ]; then skip "$name"; return 0; fi
  log " > $name  (sparse: $*)"
  if git clone --depth 1 --single-branch --filter=blob:none --sparse "$(mir "$url")" "$DEST/$name" >/dev/null 2>&1; then
    if git -C "$DEST/$name" sparse-checkout set "$@" >/dev/null 2>&1; then
      ok "$name"; return 0
    fi
    fail "sparse-checkout $name"; return 0
  fi
  log "   ! blob filter 不可用，退回完整 shallow clone"
  git clone --depth 1 --single-branch "$(mir "$url")" "$DEST/$name" >/dev/null 2>&1 \
    && ok "$name" || fail "$name  <- $url"
}

p0() {
  log "== P0 决定架构，先读这些 =="
  clone_full   "umu-launcher" "https://github.com/Open-Wine-Components/umu-launcher"
  clone_full   "lutris"       "https://github.com/lutris/lutris"
  clone_full   "heroic"       "https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher"
  clone_full   "bottles"      "https://github.com/bottlesdevs/Bottles"
  clone_full   "winegui"      "https://gitlab.melroy.org/melroy/winegui"
  clone_sparse "gnome-shell"  "https://gitlab.gnome.org/GNOME/gnome-shell" src
  log "   note  wine 已在本地 Proton 子模块里，跳过"
}

p1() {
  log "== P1 写到对应模块时再翻 =="
  clone_sparse "glib"               "https://gitlab.gnome.org/GNOME/glib" gio
  clone_full   "dash-to-dock"       "https://github.com/micheleg/dash-to-dock"
  clone_full   "desktop-file-utils" "https://gitlab.freedesktop.org/xdg/desktop-file-utils"
  clone_full   "protonup-qt"        "https://github.com/DavidoTek/ProtonUp-Qt"
  clone_full   "proton-ge-custom"   "https://github.com/GloriousEggroll/proton-ge-custom"
  clone_sparse "mangohud"           "https://github.com/flightlessmango/MangoHud" src
  clone_sparse "gamescope"          "https://github.com/ValveSoftware/gamescope" src
  clone_full   "gtk4-rs"            "https://github.com/gtk-rs/gtk4-rs"
  clone_full   "loupe"              "https://gitlab.gnome.org/GNOME/loupe"
  clone_full   "relm4"              "https://github.com/Relm4/relm4"
}

p2() {
  log "== P2 需要时再说 =="
  clone_sparse "kwin"         "https://invent.kde.org/plasma/kwin" src
  clone_full   "pelite"       "https://github.com/CasualX/pelite"
  clone_full   "umu-database" "https://github.com/Open-Wine-Components/umu-database"
  clone_full   "winetricks"   "https://github.com/Winetricks/winetricks"
  clone_full   "fractal"      "https://gitlab.gnome.org/GNOME/fractal"
}

case "${1:-p0}" in
  p0)  p0 ;;
  p1)  p1 ;;
  p2)  p2 ;;
  all) p0; p1; p2 ;;
  *)   log "用法: $0 [p0|p1|p2|all]"; exit 1 ;;
esac

log ""
log "完成。位置: $DEST"
[ -d "$DEST" ] && du -sh "$DEST" 2>/dev/null | sed 's/^/总占用: /'
