#!/usr/bin/env python3
"""carboy-desktop —— Wine/Proton 应用的 .desktop 生成器

只干一件事：把「名字 + 怎么启动 + 用什么图标 + 窗口 class」变成一份
合法、能被 GNOME/KDE/XFCE 正确识别的桌面项，并把图标装进去、刷新缓存。

用法示例：
  ./mkdesktop.py --name MihoyoLauncher \
      --exe "/home/wang/文档/MihoyoLauncher/miHoYo Launcher/launcher.exe" \
      --prefix /home/wang/.local/share/winegui/prefixes/DWProton \
      --umu-run /home/wang/.local/share/winegui/umu/umu-run \
      --proton /home/wang/.local/share/Steam/compatibilitytools.d/dwproton-11.0-16-x86_64 \
      --gameid umu-526997106 \
      --icon ./icon.png \
      --install

  # 只看会生成什么，不落盘
  ./mkdesktop.py ... --dry-run
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

# ---------- Exec 转义（Desktop Entry Spec §The Exec key） ----------
RESERVED = set(' \t\n"\'\\><~|&;$*?#()`')


def esc(arg: str) -> str:
    """转义单个 Exec 参数。先处理字面 %，再按规范转义引号内的特殊字符。"""
    if arg == "":
        return '""'
    body = []
    for ch in arg:
        if ch in '"`$\\':
            body.append("\\" + ch)
        elif ch == "%":
            body.append("%%")
        else:
            body.append(ch)
    s = "".join(body)
    if any(c in RESERVED for c in arg):
        return '"' + s + '"'
    return s


def slugify(name: str) -> str:
    s = re.sub(r"[^a-z0-9]+", "-", name.strip().lower()).strip("-")
    if s:
        return s
    import hashlib
    return "app-" + hashlib.sha1(name.strip().encode("utf-8")).hexdigest()[:8]


# umu 里写死的一条正则：只有形如 umu-xxx 的 GAMEID 才会被拆成 SteamAppId。
# 见 umu_run.py:279 `if match(r"^umu-[\d\w]+$", env["UMU_ID"])`，不匹配时
# STEAM_COMPAT_APP_ID 保持 "0"，于是 Proton 的 WM_CLASS 变成 steam_app_0。
UMU_ID_RE = re.compile(r"^umu-[\w]+$")


def is_umu_gameid(gameid: str | None) -> bool:
    return bool(gameid and UMU_ID_RE.match(gameid))


def auto_gameid(name: str) -> str:
    """按名称生成唯一 GAMEID。

    必须以 umu- 开头 —— 不是审美问题：umu 只认这个前缀，否则 SteamAppId
    永远是 0，所有游戏的 WM_CLASS 都会撞成 steam_app_0（任务栏合并）。
    """
    s = slugify(name)
    if s.startswith("app-"):     # 中文名走哈希，去掉多余的 app- 前缀，class 更干净
        s = s[4:]
    return "umu-" + s


def derive_wmclass(args) -> tuple[str, str]:
    """返回 (wm_class, 来源说明)。规则来自 wine/Proton 源码实测。

    注意：GAMEID 只有在走 Proton（umu）时才影响 class；wine 模式永远用 exe 名。
    """
    if args.wmclass:
        return args.wmclass, "手动指定 / 实测"
    proton_mode = bool(getattr(args, "umu_run", None) or getattr(args, "proton", None))
    if proton_mode:
        gid = args.gameid
        if not gid:
            return "steam_proton", "Proton 但无 GAMEID（所有游戏会撞车，建议补 --gameid）"
        if not is_umu_gameid(gid):
            return ("steam_app_0",
                    f"GAMEID={gid} 不以 umu- 开头，umu 会强制 SteamAppId=0，"
                    "所有游戏共用 steam_app_0")
        return f"steam_app_{gid.split('-', 1)[1]}", "由 GAMEID 派生（Proton 补丁）"
    if args.exe:
        return Path(args.exe).name.lower(), "wine：exe 文件名小写"
    return "", "无法推导"


def build_exec(args) -> str:
    if args.exec_raw:
        return args.exec_raw

    parts = []
    if args.umu_run or args.proton or args.gameid or args.prefix:
        pairs: dict[str, str] = {}
        if args.wineprefix_env and args.prefix:
            pairs["WINEPREFIX"] = args.prefix
        for kv in args.env or []:
            k, _, v = kv.partition("=")
            if k:
                pairs[k] = v
        if args.umu_run:
            if args.gameid:
                pairs["GAMEID"] = args.gameid
            if args.proton:
                pairs["PROTONPATH"] = args.proton
        if args.umu_use_steam:
            pairs["UMU_USE_STEAM"] = "1"
        parts.append("env")
        for k, v in pairs.items():
            parts.append(f"{k}={esc(v)}")

    if args.umu_run:
        launcher = resolve_umu(args.umu_run) or args.umu_run
    elif getattr(args, "wine_bin", None):
        launcher = resolve_wine(args.wine_bin) or args.wine_bin
    else:
        launcher = "wine"
    parts.append(esc(launcher))
    if args.exe:
        parts.append(esc(args.exe))
    for a in args.arg or []:
        parts.append(esc(a))
    return " ".join(parts)


def desktop_text(args, wm_class: str) -> str:
    slug = args.slug or slugify(args.name)
    icon = args.icon_name or slug
    workdir = args.workdir or (str(Path(args.exe).parent) if args.exe else "")

    lines = [
        "[Desktop Entry]",
        "Version=1.5",
        "Type=Application",
        f"Name={args.name}",
    ]
    if args.comment:
        lines.append(f"Comment={args.comment}")
    if args.name_zh:
        lines.append(f"Name[zh_CN]={args.name_zh}")
    lines.append(f"Exec={build_exec(args)}")
    if workdir:
        lines.append(f"Path={workdir}")
    lines.append(f"Icon={icon}")
    lines += [
        "Terminal=false",
        f"Categories={args.categories}",
        # Wine 不发 startup-complete 通知，设 true 会让 dock 光标一直转
        "StartupNotify=false",
    ]
    if wm_class:
        lines.append(f"StartupWMClass={wm_class}")
    if args.prefer_dgpu:
        lines.append("PrefersNonDefaultGPU=true")
    if args.gameid:
        lines.append(f"X-Carboy-GameId={args.gameid}")
    if args.prefix:
        lines.append(f"X-Carboy-Prefix={args.prefix}")
    lines.append(f"X-Carboy-Generated={datetime.now(timezone.utc).astimezone().isoformat(timespec='seconds')}")
    return "\n".join(lines) + "\n"


# ---------- 图标安装 ----------
SIZES = [16, 24, 32, 48, 64, 128, 256]


# ---------- 偏好设置 ----------
def config_dir() -> Path:
    return Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")) / "carboy"


def load_settings() -> dict:
    try:
        return json.loads((config_dir() / "settings.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def save_settings(data: dict) -> None:
    d = config_dir()
    d.mkdir(parents=True, exist_ok=True)
    p = d / "settings.json"
    tmp = p.with_suffix(".tmp")
    tmp.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
    os.replace(tmp, p)


from umubuild import find_umu, resolve_umu  # noqa: E402
from winebuild import find_wine, is_wine_source, resolve_wine  # noqa: E402


def install_icon(src: Path, slug: str, icons_root: Path) -> list[str]:
    """把 PNG/SVG 装进 hicolor 多尺寸目录，返回已安装路径。"""
    done = []
    if src.suffix.lower() == ".svg":
        dst_dir = icons_root / "scalable" / "apps"
        dst_dir.mkdir(parents=True, exist_ok=True)
        dst = dst_dir / f"{slug}.svg"
        shutil.copy2(src, dst)
        return [str(dst)]

    magick = shutil.which("magick") or shutil.which("convert")
    for n in SIZES:
        dst_dir = icons_root / f"{n}x{n}" / "apps"
        dst_dir.mkdir(parents=True, exist_ok=True)
        dst = dst_dir / f"{slug}.png"
        if magick:
            r = subprocess.run(
                [magick, str(src), "-resize", f"{n}x{n}", "-background", "none",
                 "-gravity", "center", "-extent", f"{n}x{n}", str(dst)],
                capture_output=True,
            )
            if r.returncode != 0:
                shutil.copy2(src, dst)
        else:
            shutil.copy2(src, dst)
        done.append(str(dst))
    return done


def extract_icon(exe: str):
    """从 exe 的 PE 资源里抠图标，返回 (png bytes, (w,h), 档位列表)。"""
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import peicon  # 同目录

    return peicon.extract(Path(exe))


def ensure_index_theme(icons_root: Path) -> None:
    """用户目录下的 hicolor 默认没有 index.theme，缺了它 gtk-update-icon-cache 会拒绝工作。"""
    idx = icons_root / "index.theme"
    if idx.exists():
        return
    src = Path("/usr/share/icons/hicolor/index.theme")
    if src.is_file():
        try:
            idx.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, idx)
        except OSError:
            pass


def refresh(icons_root: Path, apps_dir: Path) -> list[str]:
    """刷新图标缓存与桌面项索引。返回实际执行过的命令，便于 UI 展示。"""
    done = []
    if shutil.which("update-desktop-database"):
        subprocess.run(["update-desktop-database", str(apps_dir)], capture_output=True)
        done.append("update-desktop-database")

    ensure_index_theme(icons_root)
    if shutil.which("gtk-update-icon-cache") and (icons_root / "index.theme").exists():
        subprocess.run(["gtk-update-icon-cache", "-f", "-t", str(icons_root)],
                       capture_output=True)
        done.append("gtk-update-icon-cache")
    return done


# ---------- 管理已生成的桌面项 ----------
def parse_desktop_file(p: Path) -> dict:
    out: dict[str, str] = {"path": str(p)}
    for raw in p.read_text(encoding="utf-8", errors="replace").splitlines():
        s = raw.strip()
        if not s or s.startswith("#") or s.startswith("["):
            continue
        k, _, v = s.partition("=")
        out[k.strip()] = v.strip()
    return out


def list_x11_windows() -> list[dict]:
    """列出当前 X11/XWayland 窗口及其 WM_CLASS，用于反查窗口标识。"""
    if not shutil.which("xprop"):
        return []

    def run(*args):
        return subprocess.run(["xprop", *args], capture_output=True, text=True).stdout

    out = run("-root", "_NET_CLIENT_LIST")
    res = []
    for wid in re.findall(r"0x[0-9a-f]+", out):
        cls = run("-id", wid, "WM_CLASS")
        m = re.search(r'= "(.*?)", "(.*?)"', cls)
        nm = run("-id", wid, "_NET_WM_NAME")
        t = re.search(r'= "(.*)"', nm)
        res.append({
            "id": wid,
            "title": t.group(1) if t else "",
            "wm_class": m.group(2) if m else "",
        })
    return res


def parse_exec(s: str) -> dict | None:
    """把生成的 Exec 反解成 env / launcher / exe / args（用于「编辑已生成项」）。"""
    import shlex

    try:
        parts = shlex.split(s)
    except ValueError:
        return None
    envs: dict[str, str] = {}
    i = 0
    if parts and parts[0] == "env":
        i = 1
        while i < len(parts) and "=" in parts[i]:
            k, _, v = parts[i].partition("=")
            envs[k] = v
            i += 1
    rest = parts[i:]
    if not rest:
        return None
    return {
        "env": envs,
        "launcher": rest[0],
        "exe": rest[1] if len(rest) > 1 else None,
        "args": rest[2:] if len(rest) > 2 else [],
    }


def list_generated(apps_dir: Path) -> list[dict]:
    """只列我们自己生成的（带 X-Carboy-Generated 标记）。"""
    out = []
    if not apps_dir.is_dir():
        return out
    for p in sorted(apps_dir.glob("*.desktop")):
        try:
            info = parse_desktop_file(p)
        except OSError:
            continue
        if "X-Carboy-Generated" in info:
            info["slug"] = p.stem
            out.append(info)
    return out


def remove_generated(p: Path, icons_root: Path, apps_dir: Path) -> list[str]:
    """删桌面项 + 它带来的图标，然后刷新缓存。"""
    removed = []
    slug = p.stem
    for n in SIZES:
        f = icons_root / f"{n}x{n}" / "apps" / f"{slug}.png"
        if f.exists():
            f.unlink()
            removed.append(str(f))
    svg = icons_root / "scalable" / "apps" / f"{slug}.svg"
    if svg.exists():
        svg.unlink()
        removed.append(str(svg))
    if p.exists():
        p.unlink()
        removed.append(str(p))
    refresh(icons_root, apps_dir)
    return removed


def main() -> int:
    p = argparse.ArgumentParser(
        prog="mkdesktop", description="生成 Wine/Proton 应用的 .desktop（含图标安装与缓存刷新）")
    p.add_argument("--name", required=True, help="显示名")
    p.add_argument("--name-zh", help="中文名（写 Name[zh_CN]）")
    p.add_argument("--comment", default="", help="描述")
    p.add_argument("--exe", help="要启动的 exe 绝对路径")
    p.add_argument("--arg", action="append", help="追加参数，可重复")
    p.add_argument("--prefix", help="WINEPREFIX")
    p.add_argument("--umu-run", help="umu-launcher 目录 或 umu-run 文件（目录会自动在里面找 umu-run）")
    p.add_argument("--wine-bin", help="wine 可执行文件（不给则用 PATH 里的 wine）")
    p.add_argument("--proton", help="PROTONPATH（Proton 目录）")
    p.add_argument("--gameid", help="umu GAMEID，决定 WM_CLASS，必须唯一")
    p.add_argument("--env", action="append", help="额外环境变量 K=V，可重复")
    p.add_argument("--wineprefix-env", action="store_true", help="把 --prefix 作为 WINEPREFIX 写入 Exec")
    p.add_argument("--umu-use-steam", action="store_true", help="加 UMU_USE_STEAM=1")
    p.add_argument("--exec-raw", help="完全自定义 Exec，不走上面的拼接")
    p.add_argument("--wmclass", help="手动指定 StartupWMClass（默认自动推导）")
    p.add_argument("--icon", help="图标源文件（png/svg）。不给则只写 Icon=<slug>")
    p.add_argument("--icon-from-exe", action="store_true",
                   help="从 --exe 的 PE 资源里抠图标（不需要外部工具）")
    p.add_argument("--icon-name", help="Icon= 字段的值（默认用 slug）")
    p.add_argument("--slug", help="文件名与图标名（默认由 --name 生成）")
    p.add_argument("--workdir", help="Path= 工作目录（默认 exe 所在目录）")
    p.add_argument("--categories", default="Game;", help="Categories（默认 Game;）")
    p.add_argument("--prefer-dgpu", action="store_true")
    p.add_argument("--out", help="输出目录（默认 $XDG_DATA_HOME/applications）")
    p.add_argument("--install", action="store_true", help="同时安装图标并刷新缓存")
    p.add_argument("--dry-run", action="store_true", help="只打印，不写文件")
    args = p.parse_args()

    slug = args.slug or slugify(args.name)

    if not args.gameid and (args.umu_run or args.proton):
        args.gameid = auto_gameid(args.name)
        print(f"# GAMEID 未指定，按名称生成: {args.gameid}", file=sys.stderr)

    if args.icon_from_exe and not args.icon and args.exe:
        try:
            png, size, metas = extract_icon(args.exe)
            cache = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "carboy" / "icons"
            cache.mkdir(parents=True, exist_ok=True)
            f = cache / f"{slug}.png"
            f.write_bytes(png)
            args.icon = str(f)
            print(f"# 从 exe 抠出图标 {size[0]}x{size[1]}（共 {len(metas)} 档）→ {f}", file=sys.stderr)
        except Exception as e:
            print(f"# 抠图标失败: {e}", file=sys.stderr)

    data_home = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))
    apps_dir = Path(args.out) if args.out else data_home / "applications"
    icons_root = data_home / "icons" / "hicolor"

    wm_class, why = derive_wmclass(args)
    text = desktop_text(args, wm_class)

    print(text, end="")
    print(f"# WM_CLASS 来源: {why}", file=sys.stderr)
    if wm_class == "steam_proton":
        print("# 警告: steam_proton 是所有 Proton 直跑游戏共用的 class，会导致任务栏合并",
              file=sys.stderr)

    if args.dry_run:
        return 0

    apps_dir.mkdir(parents=True, exist_ok=True)
    target = apps_dir / f"{slug}.desktop"
    target.write_text(text, encoding="utf-8")
    os.chmod(target, 0o644)
    print(f"# 写入 {target}", file=sys.stderr)

    if args.install and args.icon:
        src = Path(args.icon).expanduser()
        if not src.exists():
            print(f"# 错误: 图标不存在 {src}", file=sys.stderr)
            return 1
        installed = install_icon(src, args.icon_name or slug, icons_root)
        print(f"# 图标装到 {len(installed)} 个尺寸目录", file=sys.stderr)
        refresh(icons_root, apps_dir)
        print("# 已刷新图标缓存与桌面项索引", file=sys.stderr)
    elif args.install:
        refresh(icons_root, apps_dir)

    if shutil.which("desktop-file-validate"):
        r = subprocess.run(["desktop-file-validate", str(target)],
                           capture_output=True, text=True)
        if r.returncode == 0 and not r.stderr.strip():
            print("# 校验: 通过", file=sys.stderr)
        else:
            print(f"# 校验输出:\n{r.stderr}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
