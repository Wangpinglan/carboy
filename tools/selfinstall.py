"""把 Carboy 自己装进应用菜单。

没有这一步，GNOME Shell 找不到 app-id 对应的 .desktop，就会在 dock 里
凭空造一个伪应用 —— 名字直接显示 `io.github.carboy.MkDesktop`，图标是
一个通用齿轮。GTK 应用的窗口身份就是它的 application id，所以只需
一份同名 .desktop + 一个同名图标。
"""

from __future__ import annotations

import os
import shutil
from pathlib import Path

APP_ID = "io.github.carboy.MkDesktop"
APP_NAME = "Carboy"
APP_COMMENT = "为 Wine / Proton 应用生成可用的 .desktop"

ICON_SVG = """<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128">
  <defs>
    <!-- 底 -->
    <linearGradient id="bg" x1="0" y1="0" x2="0.6" y2="1">
      <stop offset="0" stop-color="#33445f"/>
      <stop offset="0.55" stop-color="#243449"/>
      <stop offset="1" stop-color="#18222f"/>
    </linearGradient>
    <!-- 玻璃主体 -->
    <linearGradient id="glass" x1="0.1" y1="0" x2="0.9" y2="1">
      <stop offset="0" stop-color="#8fd0f2"/>
      <stop offset="0.38" stop-color="#4c9fd6"/>
      <stop offset="0.72" stop-color="#2f74a8"/>
      <stop offset="1" stop-color="#1f4f78"/>
    </linearGradient>
    <!-- 酒液 -->
    <linearGradient id="wine" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#b4385c"/>
      <stop offset="0.5" stop-color="#8a2343"/>
      <stop offset="1" stop-color="#4e1226"/>
    </linearGradient>
    <linearGradient id="cork" x1="0" y1="0" x2="0.3" y2="1">
      <stop offset="0" stop-color="#e8b273"/>
      <stop offset="0.6" stop-color="#c98b4b"/>
      <stop offset="1" stop-color="#9a6229"/>
    </linearGradient>
    <linearGradient id="sheen" x1="0" y1="0" x2="1" y2="0.7">
      <stop offset="0" stop-color="#ffffff" stop-opacity="0.55"/>
      <stop offset="0.35" stop-color="#ffffff" stop-opacity="0.08"/>
      <stop offset="1" stop-color="#ffffff" stop-opacity="0"/>
    </linearGradient>
    <radialGradient id="glow" cx="0.35" cy="0.18" r="0.75">
      <stop offset="0" stop-color="#ffffff" stop-opacity="0.22"/>
      <stop offset="1" stop-color="#ffffff" stop-opacity="0"/>
    </radialGradient>

    <filter id="soft" x="-30%" y="-30%" width="160%" height="160%">
      <feGaussianBlur stdDeviation="3.2"/>
    </filter>
    <filter id="tiny" x="-20%" y="-20%" width="140%" height="140%">
      <feGaussianBlur stdDeviation="1.1"/>
    </filter>

    <!-- 瓶身轮廓，用于裁切酒液与玻璃 -->
    <path id="jug" d="M52 40
      C 43 52 32 68 32 83
      C 32 100 46 111 64 111
      C 82 111 96 100 96 83
      C 96 68 85 52 76 40 Z"/>
    <clipPath id="jugClip"><use href="#jug"/></clipPath>
  </defs>

  <!-- 底：圆角方块 + 顶部微光 -->
  <rect x="4" y="4" width="120" height="120" rx="27" fill="url(#bg)"/>
  <rect x="4" y="4" width="120" height="120" rx="27" fill="url(#glow)"/>

  <!-- 投影 -->
  <ellipse cx="64" cy="112" rx="35" ry="7" fill="#000000" opacity="0.35" filter="url(#soft)"/>

  <!-- 瓶颈：细长 + 上端玻璃唇 -->
  <path d="M56.5 27 h15 v15 h-15 z" fill="url(#glass)"/>
  <path d="M56.5 27 h6 v15 h-6 z" fill="#ffffff" opacity="0.14"/>
  <path d="M68 27 h3.5 v15 h-3.5 z" fill="#0d2b45" opacity="0.18"/>

  <!-- 玻璃瓶身 -->
  <use href="#jug" fill="url(#glass)"/>

  <!-- 酒液（裁在瓶身里） -->
  <g clip-path="url(#jugClip)">
    <rect x="30" y="72" width="68" height="42" fill="url(#wine)"/>
    <ellipse cx="64" cy="72.5" rx="34" ry="5.5" fill="#c14a6b" opacity="0.85"/>
    <ellipse cx="64" cy="72.5" rx="34" ry="5.5" fill="none" stroke="#e8789a" stroke-opacity="0.5" stroke-width="0.8"/>
    <!-- 玻璃反光压酒液下半部 -->
    <rect x="30" y="72" width="68" height="42" fill="#ffffff" opacity="0.05"/>
    <!-- 气泡 -->
    <circle cx="50" cy="90" r="2.1" fill="#ffffff" opacity="0.35"/>
    <circle cx="56" cy="97" r="1.3" fill="#ffffff" opacity="0.28"/>
  </g>

  <!-- 玻璃高光 -->
  <path d="M47 55 C 40 66 37 75 38 86" fill="none" stroke="#ffffff" stroke-opacity="0.55"
        stroke-width="5.2" stroke-linecap="round" filter="url(#tiny)"/>
  <path d="M54 48 C 50 54 47 59 45 65" fill="none" stroke="#ffffff" stroke-opacity="0.30"
        stroke-width="2.6" stroke-linecap="round"/>
  <path d="M86 62 C 90 70 92 78 92 85" fill="none" stroke="#ffffff" stroke-opacity="0.16"
        stroke-width="3.4" stroke-linecap="round"/>
  <use href="#jug" fill="url(#sheen)"/>
  <use href="#jug" fill="none" stroke="#dff2ff" stroke-opacity="0.28" stroke-width="1.1"/>

  <!-- 瓶口 + 软木塞 -->
  <rect x="52" y="23.5" width="24" height="7.5" rx="3.4" fill="#3b86b8"/>
  <rect x="52" y="23.5" width="24" height="3.4" rx="1.7" fill="#bfe6ff" opacity="0.55"/>
  <rect x="53" y="11" width="22" height="15" rx="4" fill="url(#cork)"/>
  <rect x="53" y="11" width="22" height="15" rx="4" fill="none" stroke="#8a5620" stroke-opacity="0.55" stroke-width="0.9"/>
  <ellipse cx="64" cy="12.6" rx="10.2" ry="3" fill="#f0c48c"/>
  <ellipse cx="60" cy="12.4" rx="4.6" ry="1.5" fill="#ffffff" opacity="0.35"/>
  <rect x="53" y="20" width="22" height="2.2" fill="#7d4c1b" opacity="0.35"/>
</svg>
"""


def data_home() -> Path:
    return Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))


def gui_path() -> Path:
    return Path(__file__).resolve().parent.parent / "gui" / "mkdesktop-gui.py"


def desktop_text(exec_cmd: str, name: str = APP_NAME) -> str:
    return f"""[Desktop Entry]
Type=Application
Name={name}
GenericName=启动项生成器
GenericName[zh_CN]=启动项生成器
Comment={APP_COMMENT}
Comment[zh_CN]={APP_COMMENT}
Exec={exec_cmd}
Icon={APP_ID}
Terminal=false
Categories=Utility;
Keywords=wine;proton;desktop;umu;launcher;
StartupNotify=true
StartupWMClass={APP_ID}
"""


def find_python() -> str:
    """挑一个真能 import gi 的解释器。

    PATH 里的 python3 未必是系统那个 —— 沙箱 / pyenv / uv 都可能顶掉它，
    结果就是桌面项里写了个没有 PyGObject 的解释器，点下去直接
    ModuleNotFoundError，Shell 只能干转圈。
    """
    import subprocess
    import sys

    cands = [sys.executable, "/usr/bin/python3", shutil.which("python3") or "",
             shutil.which("python3.13") or "", shutil.which("python3.12") or ""]
    seen = set()
    for c in cands:
        if not c or c in seen:
            continue
        seen.add(c)
        if not Path(c).is_file():
            continue
        try:
            r = subprocess.run([c, "-c", "import gi"], capture_output=True, timeout=20)
        except (OSError, subprocess.SubprocessError):
            continue
        if r.returncode == 0:
            return c
    raise RuntimeError("找不到带 PyGObject 的 python3（需要 python3-gobject / python3-gi）")


def install(gui: Path | None = None, python: str | None = None) -> list[Path]:
    """写图标 + 桌面项并刷新缓存，返回生成的文件路径。"""
    from mkdesktop import ensure_index_theme, refresh

    apps = data_home() / "applications"
    icons = data_home() / "icons" / "hicolor"
    apps.mkdir(parents=True, exist_ok=True)

    svg = icons / "scalable" / "apps" / f"{APP_ID}.svg"
    svg.parent.mkdir(parents=True, exist_ok=True)
    svg.write_text(ICON_SVG, encoding="utf-8")

    gui = gui or gui_path()
    python = python or find_python()
    exec_cmd = f'"{python}" "{gui}"'

    entry = apps / f"{APP_ID}.desktop"
    entry.write_text(desktop_text(exec_cmd), encoding="utf-8")
    entry.chmod(0o644)

    # 位图兜底：个别主题/缩放因子不吃 scalable
    for size in (32, 64, 128, 256):
        png = icons / f"{size}x{size}" / "apps" / f"{APP_ID}.png"
        png.parent.mkdir(parents=True, exist_ok=True)
        if render_png(svg, png, size):
            pass

    ensure_index_theme(icons)
    refresh(icons, apps)
    return [svg, entry]


def render_png(svg: Path, out: Path, size: int) -> bool:
    """用 rsvg-convert / ImageMagick 生成 PNG；都没有就跳过。"""
    for cmd in (["rsvg-convert", "-w", str(size), "-h", str(size), "-o", str(out), str(svg)],
                ["magick", "-background", "none", "-density", "384", str(svg),
                 "-resize", f"{size}x{size}", str(out)],
                ["convert", "-background", "none", "-density", "384", str(svg),
                 "-resize", f"{size}x{size}", str(out)]):
        if shutil.which(cmd[0]):
            import subprocess

            if subprocess.run(cmd, capture_output=True).returncode == 0 and out.exists():
                return True
    return False


def remove() -> list[Path]:
    from mkdesktop import refresh

    apps = data_home() / "applications"
    icons = data_home() / "icons" / "hicolor"
    gone = []
    for p in [apps / f"{APP_ID}.desktop", icons / "scalable" / "apps" / f"{APP_ID}.svg"]:
        if p.exists():
            p.unlink()
            gone.append(p)
    for size in (32, 64, 128, 256):
        p = icons / f"{size}x{size}" / "apps" / f"{APP_ID}.png"
        if p.exists():
            p.unlink()
            gone.append(p)
    refresh(icons, apps)
    return gone


if __name__ == "__main__":
    import argparse

    ap = argparse.ArgumentParser(description="把 Carboy 装进应用菜单")
    ap.add_argument("--install", action="store_true", help="安装桌面项与图标")
    ap.add_argument("--remove", action="store_true", help="卸载")
    a = ap.parse_args()
    if a.remove:
        for p in remove():
            print(f"已删除 {p}")
    else:
        for p in install():
            print(f"已写入 {p}")
        entry = data_home() / "applications" / f"{APP_ID}.desktop"
        for line in entry.read_text(encoding="utf-8").splitlines():
            if line.startswith("Exec="):
                print(f"启动命令 {line}")
        print(f"\n现在应用菜单里会出现「{APP_NAME}」，dock 也会认这个窗口了。")
