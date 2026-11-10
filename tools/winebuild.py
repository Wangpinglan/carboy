#!/usr/bin/python3
"""从源码构建 wine（和 umu-launcher 的构建流程同构）。

只做三件事：查依赖 → 在源码树里 build/ 下 configure + make →（可选）make install。
不碰 Proton：那类仓库太大，且社区已有大量打好补丁的构建产物，直接拿来用即可。

用法:
  ./winebuild.py --check ~/项目/wine-11.21
  ./winebuild.py --build ~/项目/wine-11.21 --jobs 16 [--arch32] [--install]
"""

import glob
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")

REQUIRED_TOOLS = (
    ("gcc", "C 编译器"),
    ("g++", "C++ 编译器"),
    ("make", "构建入口"),
    ("flex", "词法分析生成"),
    ("bison", "语法分析生成"),
    ("pkg-config", "依赖探测"),
)

PE_NAME = "PE 交叉编译器"
STATIC_LIBC = "静态 libc"

# (pkg-config 模块, 说明)
REQUIRED_MODULES = (
    ("freetype2", "字体渲染"),
    ("fontconfig", "字体配置"),
    ("x11", "X11"),
    ("xext", "X11 扩展"),
    ("xrender", "X11 渲染"),
    ("xrandr", "显示模式"),
    ("xi", "输入设备"),
    ("xcursor", "光标"),
    ("xfixes", "X11 修复扩展"),
    ("xinerama", "多屏"),
    ("xcomposite", "合成"),
    ("gnutls", "TLS（联网组件）"),
    ("alsa", "声音"),
    ("libpulse", "PulseAudio"),
    ("vulkan", "Vulkan（DXVK/VKD3D 需要）"),
)

FEDORA_PKGS = {
    PE_NAME: "lld",
    STATIC_LIBC: "glibc-static",
    "freetype2": "freetype-devel", "fontconfig": "fontconfig-devel",
    "x11": "libX11-devel", "xext": "libXext-devel", "xrender": "libXrender-devel",
    "xrandr": "libXrandr-devel", "xi": "libXi-devel", "xcursor": "libXcursor-devel",
    "xfixes": "libXfixes-devel", "xinerama": "libXinerama-devel",
    "xcomposite": "libXcomposite-devel", "gnutls": "gnutls-devel",
    "alsa": "alsa-lib-devel", "libpulse": "pulseaudio-libs-devel",
    "vulkan": "vulkan-loader-devel",
}

DEBIAN_PKGS = {
    PE_NAME: "lld",
    STATIC_LIBC: "libc6-dev",
    "freetype2": "libfreetype-dev", "fontconfig": "libfontconfig-dev",
    "x11": "libx11-dev", "xext": "libxext-dev", "xrender": "libxrender-dev",
    "xrandr": "libxrandr-dev", "xi": "libxi-dev", "xcursor": "libxcursor-dev",
    "xfixes": "libxfixes-dev", "xinerama": "libxinerama-dev",
    "xcomposite": "libxcomposite-dev", "gnutls": "libgnutls28-dev",
    "alsa": "libasound2-dev", "libpulse": "libpulse-dev",
    "vulkan": "libvulkan-dev",
}


def has_module(mod: str) -> bool:
    if not shutil.which("pkg-config"):
        return False
    r = subprocess.run(["pkg-config", "--exists", mod], capture_output=True)
    return r.returncode == 0


def pe_compiler() -> tuple[bool, str]:
    """wine 9+ 需要能产出 Windows PE 的交叉编译器。"""
    for t in ("x86_64-w64-mingw32-gcc", "amd64-w64-mingw32-gcc"):
        if shutil.which(t):
            return True, f"使用 {t}"
    if (shutil.which("clang")
            and (shutil.which("lld") or shutil.which("ld.lld"))
            and shutil.which("llvm-dlltool")):
        return True, "使用 clang + lld + llvm-dlltool"
    if shutil.which("clang"):
        return False, "缺 lld：clang 有了但链接器不完整（sudo dnf install lld）"
    return False, "缺：装 lld（配合 clang）或 mingw64-gcc"


def has_static_libc() -> bool:
    """preloader 需要 -static-pie；没有静态 libc 时 wine 会降级成 -Ttext 链接，
    在 x86_64 上直接 relocation truncated to fit。"""
    for p in ("/usr/lib64/libc.a", "/usr/lib/libc.a",
              "/usr/lib/x86_64-linux-gnu/libc.a"):
        if Path(p).is_file():
            return True
    return False


def tool_status() -> list[dict]:
    pe_ok, pe_note = pe_compiler()
    return [
        {"name": n, "required": True, "found": bool(shutil.which(n)), "note": note}
        for n, note in REQUIRED_TOOLS
    ] + [
        {"name": PE_NAME, "required": True, "found": pe_ok, "note": pe_note},
        {"name": STATIC_LIBC, "required": True, "found": has_static_libc(),
         "note": "preloader 要 -static-pie；缺了会在链接阶段 relocation 溢出"},
    ] + [
        {"name": m, "required": True, "found": has_module(m), "note": note}
        for m, note in REQUIRED_MODULES
    ]


def missing() -> list[str]:
    return [t["name"] for t in tool_status() if not t["found"]]


def install_hint(names=None) -> str:
    names = names if names is not None else missing()
    fed = " ".join(FEDORA_PKGS.get(n, n) for n in names)
    deb = " ".join(DEBIAN_PKGS.get(n, n) for n in names)
    return f"Fedora:  sudo dnf install {fed}\nDebian:  sudo apt install {deb}"


def is_source_dir(d: Path) -> bool:
    return (d / "configure").is_file() and (d / "dlls").is_dir()


def _run(cmd, cwd, log):
    log(f"$ {' '.join(str(c) for c in cmd)}")
    proc = subprocess.Popen(
        [str(c) for c in cmd], cwd=str(cwd), stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, text=True, errors="replace", bufsize=1,
    )
    assert proc.stdout
    for line in proc.stdout:
        log(ANSI.sub("", line.rstrip()))
    rc = proc.wait()
    if rc != 0:
        raise RuntimeError(f"命令失败（退出码 {rc}）: {' '.join(str(c) for c in cmd)}")


def build(src: Path, log=lambda s: None, jobs: int = 0,
          arch32: bool = False, prefix: str | None = None,
          do_install: bool = False) -> Path:
    if not is_source_dir(src):
        raise ValueError(f"{src} 不像 wine 源码目录（缺 configure / dlls）")

    miss = missing()
    if miss:
        raise RuntimeError(
            "缺依赖：" + ", ".join(miss) + "\n\n" + install_hint(miss))

    jobs = jobs or (os.cpu_count() or 4)
    builddir = src / "build"
    builddir.mkdir(exist_ok=True)

    cfg = ["../configure"]
    if prefix:
        cfg.append(f"--prefix={prefix}")
    cfg.append("--enable-archs=" + ("i386,x86_64" if arch32 else "x86_64"))
    _run(cfg, builddir, log)
    _run(["make", f"-j{jobs}"], builddir, log)

    wine = builddir / "wine"
    if not wine.exists():
        raise RuntimeError("make 完成但没找到 build/wine")

    if do_install:
        _run(["make", "install"], builddir, log)
        return Path(prefix or "/usr/local") / "bin" / "wine"
    return wine


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 1
    log = lambda s: print(s, flush=True)  # noqa: E731

    if args[0] == "--check":
        st = tool_status()
        bad = [t["name"] for t in st if not t["found"]]
        print(f"源码目录: {is_source_dir(Path(args[1]).expanduser()) if len(args) > 1 else 'n/a'}")
        print(f"缺失    : {', '.join(bad) or '无'}")
        if bad:
            print(install_hint(bad))
        return 0

    if args[0] == "--build":
        src = Path(args[1]).expanduser()
        jobs = 0
        if "--jobs" in args:
            jobs = int(args[args.index("--jobs") + 1])
        try:
            out = build(src, log, jobs=jobs,
                        arch32="--arch32" in args,
                        prefix=(os.path.expanduser("~/.local") if "--install" in args else None),
                        do_install="--install" in args)
        except Exception as e:  # noqa: BLE001
            print(f"失败: {e}", file=sys.stderr)
            return 2
        print("完成:", out)
        return 0

    print(__doc__)
    return 1


if __name__ == "__main__":
    sys.exit(main())


# ---------- 定位 wine ----------
def find_wine() -> str | None:
    """找可用的 wine 可执行文件（系统版 / Lutris 里的 Wine-GE 等）。"""
    for name in ("wine", "wine64"):
        p = shutil.which(name)
        if p:
            return p
    for p in (
        Path.home() / ".local/bin/wine",
        Path("/usr/local/bin/wine"),
        Path("/usr/bin/wine"),
    ):
        if p.is_file() and os.access(p, os.X_OK):
            return str(p)
    for pattern in (
        "~/.local/share/lutris/runners/wine/*/bin/wine",
        "~/.local/share/wine-ge/*/bin/wine",
    ):
        hits = sorted(glob.glob(os.path.expanduser(pattern)), reverse=True)
        if hits:
            return hits[0]
    return None


WINE_CANDIDATES = ["wine", "bin/wine", "build/wine", "wine64", "bin/wine64"]


def is_wine_source(d: Path) -> bool:
    """大致判断是不是 wine 的源码树（有 configure + dlls + loader）。"""
    return (d / "configure").is_file() and (d / "dlls").is_dir() and (d / "loader").is_dir()


def resolve_wine(value: str | None) -> str | None:
    """把「wine 可执行文件 / 安装目录 / 已编译的源码树」统一解析成可执行路径。"""
    if not value:
        return None
    p = Path(value).expanduser()
    if p.is_dir():
        for rel in WINE_CANDIDATES:
            c = p / rel
            if c.is_file() and os.access(c, os.X_OK):
                return str(c)
        return None
    if p.is_file():
        return str(p)
    return shutil.which(value)
