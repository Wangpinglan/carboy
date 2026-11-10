#!/usr/bin/env bash
# 窗口身份探针 —— 验证 WM_CLASS / _NET_WM_ICON / _NET_WM_PID
#
# 用法:
#   ./probe-window.sh                      只列当前所有窗口（游戏已经在跑时用）
#   ./probe-window.sh -- <命令...>          启动命令并监视 60 秒
#   ./probe-window.sh -t 120 -- umu-run /path/game.exe
#
# 依赖: xprop (x11-utils)

set -uo pipefail

TIMEOUT=60
while getopts "t:h" opt; do
  case $opt in
    t) TIMEOUT=$OPTARG ;;
    h) sed -n '2,10p' "$0"; exit 0 ;;
    *) exit 1 ;;
  esac
done
shift $((OPTIND-1))

command -v xprop >/dev/null || { echo "缺少 xprop：sudo dnf install x11-utils  或  sudo apt install x11-utils"; exit 1; }
xprop -root -len 0 >/dev/null 2>&1 || { echo "连不上 X (DISPLAY=${DISPLAY:-未设置})。在 X11 或 XWayland 会话里运行。"; exit 1; }

list_windows() {
  xprop -root _NET_CLIENT_LIST 2>/dev/null \
    | sed 's/.*#//' | tr ',' '\n' | tr -d ' ' | grep -E '^0x[0-9a-f]+$'
}

ppid_of() { sed 's/.*) //' "/proc/$1/stat" 2>/dev/null | awk '{print $2}'; }

in_tree() { # $1=pid $2=root
  local cur=$1 i=0
  while [ $i -lt 32 ]; do
    [ "$cur" = "$2" ] && return 0
    [ -z "$cur" ] || [ "$cur" -le 1 ] 2>/dev/null && return 1
    cur=$(ppid_of "$cur"); i=$((i+1))
  done
  return 1
}

icon_sizes() { # 只统计档位，不落地像素
  local out
  out=$(xprop -id "$1" _NET_WM_ICON 2>/dev/null)
  case "$out" in
    *"not found"*|"") echo "无"; return ;;
  esac
  printf '%s' "$out" | sed 's/^[^=]*=//' | tr ',' '\n' | awk '
    $1 ~ /^[0-9]+$/ {
      n = $1 + 0
      if (st == 0) { w = n; st = 1; next }
      if (st == 1) { h = n; printf "%dx%d ", w, h; need = w * h; got = 0; st = 2; cnt++; next }
      if (st == 2) { got++; if (got >= need) st = 0 }
    }
    END { if (cnt == 0) printf "无" }'
}

report() { # $1=id $2=rootpid(可为空)
  local id=$1 root=${2:-} line inst class pid title icon mark
  line=$(xprop -id "$id" WM_CLASS 2>/dev/null)
  inst=$(printf '%s' "$line" | sed -n 's/.*= "\([^"]*\)", *"\([^"]*\)".*/\1/p')
  class=$(printf '%s' "$line" | sed -n 's/.*= "\([^"]*\)", *"\([^"]*\)".*/\2/p')
  [ -z "$class" ] && { inst="-"; class="(无 WM_CLASS)"; }

  pid=$(xprop -id "$id" _NET_WM_PID 2>/dev/null | sed -n 's/.*= *\([0-9]*\).*/\1/p')
  title=$(xprop -id "$id" _NET_WM_NAME 2>/dev/null | sed -n 's/.*= "\(.*\)"/\1/p')
  icon=$(icon_sizes "$id")

  if [ -n "$root" ] && [ -n "$pid" ]; then
    if in_tree "$pid" "$root"; then mark="是"; else mark="否（进程树已断）"; fi
  else
    mark="-"
  fi

  printf '  窗口 %s  %s\n' "$id" "${title:0:48}"
  printf '    WM_CLASS  res_name=%s   res_class=%s\n' "$inst" "$class"
  printf '    _NET_WM_PID  %s   (在启动进程树内: %s)\n' "${pid:--}" "$mark"
  printf '    _NET_WM_ICON 档位  %s\n\n' "$icon"
  printf '%s\n' "$class" >> "$TMPCLASSES"
}

TMPCLASSES=$(mktemp); trap 'rm -f "$TMPCLASSES"' EXIT

echo "=== 基线快照 $(date +%H:%M:%S) ==="
if [ $# -eq 0 ]; then
  ids=$(list_windows)
  if [ -z "$ids" ]; then echo "当前没有窗口"; exit 0; fi
  n=0
  for id in $ids; do n=$((n+1)); report "$id"; done
  echo "共 $n 个窗口"
else
  base=$(list_windows)
  echo "启动: $*"
  "$@" & ROOT=$!
  echo "子进程 PID=$ROOT，监视 ${TIMEOUT}s …"
  deadline=$(( $(date +%s) + TIMEOUT ))
  seen="$base"
  found=0
  while [ "$(date +%s)" -lt "$deadline" ]; do
    sleep 0.3
    for id in $(list_windows); do
      case " $seen " in *" $id "*) continue ;; esac
      seen="$seen $id"
      found=$((found+1))
      echo ""
      echo "--- 新窗口 #$found ---"
      report "$id" "$ROOT"
    done
    kill -0 "$ROOT" 2>/dev/null || { [ "$found" -gt 0 ] && break; }
  done
  [ "$found" -eq 0 ] && echo "没有捕获到新窗口（游戏可能用了已存在的窗口，或已退出）"
fi

echo "=== res_class 汇总 ==="
if [ -s "$TMPCLASSES" ]; then
  sort "$TMPCLASSES" | uniq -c | sort -rn | awk '{printf "  %-30s %s 个窗口\n", $2, $1}'
  dup=$(sort "$TMPCLASSES" | uniq -d | head -1)
  [ -n "$dup" ] && echo "" && echo "  !! \"$dup\" 出现在多个窗口上 —— 这些窗口在任务栏会合并成一个图标"
else
  echo "  无数据"
fi
