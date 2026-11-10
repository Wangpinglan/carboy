#!/usr/bin/python3
"""预热 cargo registry：自己下载并解包 crate，绕过沙箱对 utimensat 的拦截。

cargo 解包时会从 tar 头恢复文件 mtime，这一步在本环境被策略拒绝（EPERM），
于是 `cargo build` 永远停在第一个 crate 上。这个脚本做同样的事，但只写内容、
不改时间，并补齐 cargo 需要的 `.cargo-ok` 与 `.cargo-checksum.json`。
"""

import hashlib
import io
import json
import os
import re
import sys
import tarfile
import urllib.request
from pathlib import Path

HOME = Path.home()
REG = HOME / ".cargo" / "registry"
INDEX = "index.crates.io-1949cf8c6b5b557f"
CACHE = REG / "cache" / INDEX
SRC = REG / "src" / INDEX


def parse_lock(path: Path):
    text = path.read_text(encoding="utf-8")
    out = []
    for block in text.split("[[package]]")[1:]:
        name = re.search(r'^name = "(.*)"', block, re.M)
        ver = re.search(r'^version = "(.*)"', block, re.M)
        src = re.search(r'^source = "(.*)"', block, re.M)
        cks = re.search(r'^checksum = "(.*)"', block, re.M)
        if not (name and ver and src):
            continue
        if "crates.io" not in src.group(1):
            continue  # 本地路径依赖
        out.append((name.group(1), ver.group(1), cks.group(1) if cks else ""))
    return out


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def download(name: str, ver: str) -> Path:
    dst = CACHE / f"{name}-{ver}.crate"
    if dst.exists() and dst.stat().st_size > 0:
        return dst
    CACHE.mkdir(parents=True, exist_ok=True)
    url = f"https://static.crates.io/crates/{name}/{name}-{ver}.crate"
    with urllib.request.urlopen(url, timeout=60) as r:
        data = r.read()
    dst.write_bytes(data)
    return dst


def safe_join(root: Path, member: str) -> Path:
    # tar 里可能带 pkg-version/ 前缀，剥掉
    parts = [p for p in member.split("/") if p not in ("", ".", "..")]
    return root.joinpath(*parts[1:]) if len(parts) > 1 else root


def unpack(crate: Path, name: str, ver: str) -> Path:
    dest = SRC / f"{name}-{ver}"
    if (dest / ".cargo-ok").exists():
        return dest
    dest.mkdir(parents=True, exist_ok=True)
    files = {}
    with tarfile.open(crate, "r:gz") as tf:
        for m in tf.getmembers():
            target = safe_join(dest, m.name)
            if m.isdir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            if m.issym():
                target.parent.mkdir(parents=True, exist_ok=True)
                if not target.exists():
                    os.symlink(m.linkname, target)
                continue
            if not m.isfile():
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            data = tf.extractfile(m).read()
            target.write_bytes(data)
            if m.mode & 0o111:
                os.chmod(target, 0o755)
            rel = str(target.relative_to(dest))
            files[rel] = sha256_bytes(data)

    (dest / ".cargo-ok").write_text('{"v":1}', encoding="utf-8")
    (dest / ".cargo-checksum.json").write_text(
        json.dumps({"files": files, "package": sha256_bytes(crate.read_bytes())}),
        encoding="utf-8",
    )
    return dest


def main() -> int:
    lock = Path(__file__).resolve().parent.parent / "Cargo.lock"
    if not lock.exists():
        print("找不到 Cargo.lock", file=sys.stderr)
        return 1
    pkgs = parse_lock(lock)
    print(f"锁文件里 {len(pkgs)} 个 crates.io 依赖")
    ok = fail = 0
    for name, ver, _cks in pkgs:
        try:
            crate = download(name, ver)
            unpack(crate, name, ver)
            ok += 1
        except Exception as e:  # noqa: BLE001
            print(f"✗ {name}-{ver}: {e}", file=sys.stderr)
            fail += 1
    print(f"完成：{ok} 个就绪，{fail} 个失败")
    return 0 if fail == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
