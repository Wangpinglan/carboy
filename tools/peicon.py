#!/usr/bin/python3
"""从 Windows PE 文件里抠图标（不依赖 icoutils）。

流程：解析 PE 资源段 → 取 RT_GROUP_ICON(14) → 按 nID 取 RT_ICON(3) →
拼成标准 .ico → 交给 Pillow 解码 → 存 PNG。

用法:
    ./peicon.py game.exe              # 列出所有图标档位
    ./peicon.py game.exe out.png      # 导出最大的一档
"""

import io
import struct
import sys
from pathlib import Path

RT_ICON = 3
RT_GROUP_ICON = 14


class PE:
    def __init__(self, data: bytes):
        self.d = data
        if data[:2] != b"MZ":
            raise ValueError("不是 PE 文件")
        e = struct.unpack_from("<I", data, 0x3C)[0]
        if data[e:e + 4] != b"PE\0\0":
            raise ValueError("PE 签名不对")
        coff = e + 4
        nsec = struct.unpack_from("<H", data, coff + 2)[0]
        optsz = struct.unpack_from("<H", data, coff + 16)[0]
        opt = coff + 20
        magic = struct.unpack_from("<H", data, opt)[0]
        dd = opt + (96 if magic == 0x10B else 112)
        self.rva_res = struct.unpack_from("<I", data, dd + 2 * 8)[0]
        self.sections = []
        sec = opt + optsz
        for i in range(nsec):
            o = sec + i * 40
            vaddr = struct.unpack_from("<I", data, o + 12)[0]
            vsize = struct.unpack_from("<I", data, o + 8)[0]
            rawsz = struct.unpack_from("<I", data, o + 16)[0]
            rawptr = struct.unpack_from("<I", data, o + 20)[0]
            self.sections.append((vaddr, max(vsize, rawsz), rawptr, rawsz))
        if not self.rva_res:
            raise ValueError("没有资源段")

    def off(self, rva: int) -> int:
        for va, sz, ptr, rawsz in self.sections:
            if va <= rva < va + sz:
                delta = rva - va
                if delta >= rawsz:
                    raise ValueError("RVA 落在未初始化区")
                return ptr + delta
        raise ValueError(f"RVA 0x{rva:x} 未映射")

    # ---- 资源目录遍历 ----
    def _dir_entries(self, base: int, rel: int):
        o = base + rel
        named, ids = struct.unpack_from("<HH", self.d, o + 12)
        out = []
        p = o + 16
        for _ in range(named + ids):
            name, data = struct.unpack_from("<II", self.d, p)
            out.append((name, data))
            p += 8
        return out

    def _data_entry(self, base: int, rel: int):
        o = base + rel
        rva, size = struct.unpack_from("<II", self.d, o)
        return rva, size

    def walk(self):
        """返回 {(type_id, name_id): (bytes, 语言)}"""
        base = self.off(self.rva_res)
        out = {}
        for tname, tdata in self._dir_entries(base, 0):
            if tname & 0x80000000 or not tdata & 0x80000000:
                continue
            tid = tname & 0xFFFF
            for nname, ndata in self._dir_entries(base, tdata & 0x7FFFFFFF):
                if nname & 0x80000000 or not ndata & 0x80000000:
                    continue
                nid = nname & 0xFFFF
                for lname, ldata in self._dir_entries(base, ndata & 0x7FFFFFFF):
                    if ldata & 0x80000000:
                        continue
                    rva, size = self._data_entry(base, ldata & 0x7FFFFFFF)
                    out[(tid, nid)] = (self.d[self.off(rva):self.off(rva) + size], lname & 0xFFFF)
        return out


def build_ico(res: dict, group_id: int):
    """把一组 RT_GROUP_ICON + RT_ICON 拼成 .ico 字节串，返回 (bytes, [(w,h,bpp)])"""
    key = (RT_GROUP_ICON, group_id)
    if key not in res:
        raise ValueError(f"没有 RT_GROUP_ICON id={group_id}")
    grp = res[key][0]
    reserved, typ, count = struct.unpack_from("<HHH", grp, 0)
    if typ != 1:
        raise ValueError("不是图标组")

    heads, blobs, metas = [], [], []
    offset = 6 + 16 * count
    for i in range(count):
        w, h, cc, rsv, planes, bpp, nbytes, nid = struct.unpack_from("<BBBBHHIH", grp, 6 + i * 14)
        k = (RT_ICON, nid)
        if k not in res:
            continue
        blob = res[k][0]
        heads.append(struct.pack("<BBBBHHII", w, h, cc, rsv, planes, bpp, len(blob), offset))
        offset += len(blob)
        blobs.append(blob)
        metas.append((w or 256, h or 256, bpp))

    if not heads:
        raise ValueError("图标组里一个可用条目都没有")
    out = struct.pack("<HHH", 0, 1, len(heads)) + b"".join(heads) + b"".join(blobs)
    return out, metas


def list_groups(pe: PE):
    return sorted(nid for (t, nid) in pe.walk() if t == RT_GROUP_ICON)


def extract(path: Path):
    """返回 (png 字节, 尺寸元组, 档位列表)。需要 Pillow。"""
    from PIL import Image

    data = path.read_bytes()
    pe = PE(data)
    groups = list_groups(pe)
    if not groups:
        raise ValueError("这个 exe 没有图标资源")
    res = pe.walk()

    best = None
    for gid in groups:
        try:
            ico, metas = build_ico(res, gid)
            im = Image.open(io.BytesIO(ico))
            im.load()
            if best is None or im.size[0] > best[0].size[0]:
                best = (im, metas, gid)
        except Exception:
            continue
    if best is None:
        raise ValueError("图标解码失败")

    im, metas, gid = best
    im = im.convert("RGBA")
    buf = io.BytesIO()
    im.save(buf, format="PNG")
    return buf.getvalue(), im.size, metas


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    src = Path(sys.argv[1])
    png, size, metas = extract(src)
    print(f"图标组: {len(list_groups(PE(src.read_bytes())))} 个，导入最大 {size[0]}x{size[1]}")
    print("档位: " + ", ".join(f"{w}x{h}@{bpp}bpp" for w, h, bpp in metas))
    if len(sys.argv) > 2:
        Path(sys.argv[2]).write_bytes(png)
        print(f"已写入 {sys.argv[2]}（{len(png)} 字节）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
