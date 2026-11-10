#!/usr/bin/python3
"""umu-launcher 的安装/构建助手（含 umu-run 定位）。

两条路：
  1) build_source(dir)   按官方 README：./configure.sh --user-install → make → make install
                         需要 make + cargo + rustc + scdoc
  2) install_prebuilt()  走 uv / pipx 装 PyPI 上的预编译包，不需要 Rust 工具链

用法:
  ./umubuild.py --check ~/项目/UMU/umu-launcher
  ./umubuild.py --build ~/项目/UMU/umu-launcher
  ./umubuild.py --uv
"""

import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

# make 是硬性的；cargo/scdoc 只影响 Rust 版 umu-shim 与 man page，
# zipapp 路线（默认)不需要它们 —— 所以只提示，不拦。
REQUIRED_DEPS = ("make",)
OPTIONAL_DEPS = ("cargo", "rustc", "scdoc")

ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")

FEDORA_PKGS = {"make": "make", "python3": "python3", "cargo": "cargo",
               "rustc": "rust", "scdoc": "scdoc"}
DEBIAN_PKGS = {"make": "build-essential", "python3": "python3", "cargo": "cargo",
               "rustc": "rustc", "scdoc": "scdoc"}

# (名字, 是否必须, 说明)
TOOLS = (
    ("make", True, "构建入口，跑 Makefile"),
    ("python3", True, "把 umu 打成 zipapp"),
    ("cargo", False, "编 Rust 版 umu-shim（zipapp 路线用不到）"),
    ("rustc", False, "同上"),
    ("scdoc", False, "生成 man page"),
)


def tool_status() -> list[dict]:
    return [
        {"name": n, "required": req, "found": bool(shutil.which(n)), "note": note}
        for n, req, note in TOOLS
    ]


def install_hint(names) -> str:
    names = list(names)
    fed = " ".join(FEDORA_PKGS.get(n, n) for n in names)
    deb = " ".join(DEBIAN_PKGS.get(n, n) for n in names)
    return f"Fedora:  sudo dnf install {fed}\nDebian:  sudo apt install {deb}"


class MissingDeps(Exception):
    def __init__(self, missing):
        self.missing = missing
        super().__init__("缺少构建依赖: " + ", ".join(missing))

    def hint(self) -> str:
        return install_hint(self.missing)


def is_source_dir(d: Path) -> bool:
    return (d / "configure.sh").is_file() and (d / "pyproject.toml").is_file()


def probe(d: Path) -> dict:
    return {
        "is_source": is_source_dir(d),
        "missing_required": [t for t in REQUIRED_DEPS if not shutil.which(t)],
        "missing_optional": [t for t in OPTIONAL_DEPS if not shutil.which(t)],
        "has_builddir": (d / "builddir").is_dir(),
        "umu_run": find_installed(),
    }


def find_installed():
    for p in (
        Path.home() / ".local/bin/umu-run",
        Path.home() / ".local/share/uv/tools/umu-launcher/bin/umu-run",
        Path.home() / ".local/share/pipx/venvs/umu-launcher/bin/umu-run",
        Path("/usr/local/bin/umu-run"),
        Path("/usr/bin/umu-run"),
    ):
        if p.is_file() and p.exists():
            return str(p)
    w = shutil.which("umu-run")
    return w


def _run(cmd, cwd, log):
    log(f"$ {' '.join(cmd)}")
    proc = subprocess.Popen(
        cmd, cwd=str(cwd), stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, errors="replace", bufsize=1,
    )
    assert proc.stdout
    for line in proc.stdout:
        log(ANSI.sub("", line.rstrip()))
    rc = proc.wait()
    if rc != 0:
        raise RuntimeError(f"命令失败（退出码 {rc}）: {' '.join(cmd)}")


def build_source(src: Path, log=lambda s: None) -> Path:
    if not is_source_dir(src):
        raise ValueError(f"{src} 不像 umu-launcher 源码目录（缺 configure.sh / pyproject.toml）")

    missing = [t for t in REQUIRED_DEPS if not shutil.which(t)]
    if missing:
        raise MissingDeps(missing)
    optional = [t for t in OPTIONAL_DEPS if not shutil.which(t)]
    if optional:
        log(f":: 没装 {', '.join(optional)} —— 只影响 Rust 版 umu-shim 和 man page，"
            f"默认的 zipapp 构建不需要，继续")

    _run(["./configure.sh", "--user-install"], src, log)
    _run(["make", f"-j{max(1, (shutil.os.cpu_count() or 2))}"], src, log)
    _run(["make", "install"], src, log)

    p = Path.home() / ".local/bin/umu-run"
    if not p.exists():
        raise RuntimeError("make install 完成，但没找到 ~/.local/bin/umu-run")
    return p


def install_prebuilt(log=lambda s: None) -> Path:
    if shutil.which("uv"):
        _run(["uv", "tool", "install", "--force", "umu-launcher"], Path.home(), log)
    elif shutil.which("pipx"):
        _run(["pipx", "install", "--force", "umu-launcher"], Path.home(), log)
    else:
        raise RuntimeError("既没有 uv 也没有 pipx。装一个：sudo dnf install uv 或 pipx")
    p = find_installed()
    if not p:
        raise RuntimeError("安装命令成功，但没找到 umu-run")
    return Path(p)


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 1
    log = lambda s: print(s, flush=True)  # noqa: E731
    if args[0] == "--check":
        d = Path(args[1]).expanduser() if len(args) > 1 else None
        if not d:
            print("已安装:", find_installed() or "无")
            return 0
        info = probe(d)
        print(f"源码目录  : {info['is_source']}")
        print(f"必须缺    : {', '.join(info['missing_required']) or '无'}")
        print(f"可选缺    : {', '.join(info['missing_optional']) or '无'}")
        print(f"已安装    : {info['umu_run'] or '无'}")
        return 0
    if args[0] == "--build":
        try:
            print("完成:", build_source(Path(args[1]).expanduser(), log))
        except MissingDeps as e:
            print(e, file=sys.stderr)
            print(e.hint(), file=sys.stderr)
            return 2
        return 0
    if args[0] == "--uv":
        print("完成:", install_prebuilt(log))
        return 0
    print(__doc__)
    return 1


if __name__ == "__main__":
    sys.exit(main())


# ---------- 定位 umu-run ----------
UMU_CANDIDATES = [
    "umu-run",              # 正式安装：<prefix>/bin/umu-run 或版本目录
    "bin/umu-run",
    "builddir/umu-run",     # 源码树 make 出来的 zipapp
    "venv/bin/umu-run",
    ".venv/bin/umu-run",
    "tools/umu-run-posix",  # 源码树里的调试脚本（仅兜底）
]


def find_umu() -> str | None:
    """在标准安装位置找 umu-run。不猜第三方启动器自带的副本。"""
    p = shutil.which("umu-run")
    if p:
        return p
    for p in (
        Path.home() / ".local/bin/umu-run",
        Path.home() / ".local/share/pipx/venvs/umu-launcher/bin/umu-run",
        Path.home() / ".local/share/umu-launcher/venv/bin/umu-run",
        Path("/usr/local/bin/umu-run"),
        Path("/usr/bin/umu-run"),
    ):
        if p.is_file() and os.access(p, os.X_OK):
            return str(p)
    return None


def resolve_umu(value: str | None) -> str | None:
    """把「目录 / 文件 / 命令名」统一解析成可执行的 umu-run 路径。

    支持直接指向 umu-launcher 的源码或安装目录：
        ~/项目/UMU/umu-launcher      -> tools/umu-run-posix
        ~/.local/share/umu           -> venv/bin/umu-run
    """
    if not value:
        return None
    p = Path(value).expanduser()
    if p.is_dir():
        for rel in UMU_CANDIDATES:
            cand = p / rel
            if cand.is_file():
                if not os.access(cand, os.X_OK):
                    try:
                        os.chmod(cand, 0o755)
                    except OSError:
                        pass
                return str(cand)
        return None
    if p.is_file():
        return str(p)
    return shutil.which(value)
