//! 从 Windows PE 资源段抠图标（等价于 tools/peicon.py，不依赖 icoutils/Pillow）。
//!
//! 流程：PE 头 → 资源目录 → RT_GROUP_ICON(14) → 按 nID 取 RT_ICON(3) →
//! 取最大一档解码（PNG 直接透传，否则解 DIB）→ 输出 RGBA PNG。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;

#[derive(Debug, Clone, Copy)]
pub struct IconMeta {
    pub w: u32,
    pub h: u32,
    pub bpp: u16,
}

fn u16_at(d: &[u8], o: usize) -> Result<u16, String> {
    d.get(o..o + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| "读取越界".to_string())
}

fn u32_at(d: &[u8], o: usize) -> Result<u32, String> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "读取越界".to_string())
}

fn i32_at(d: &[u8], o: usize) -> Result<i32, String> {
    u32_at(d, o).map(|v| v as i32)
}

struct Pe<'a> {
    d: &'a [u8],
    rva_res: u32,
    /// (virtual_addr, span, raw_ptr, raw_size)
    sections: Vec<(u32, u32, u32, u32)>,
}

impl<'a> Pe<'a> {
    fn parse(d: &'a [u8]) -> Result<Self, String> {
        if d.get(0..2) != Some(b"MZ") {
            return Err("不是 PE 文件".into());
        }
        let e = u32_at(d, 0x3C)? as usize;
        if d.get(e..e + 4) != Some(b"PE\0\0") {
            return Err("PE 签名不对".into());
        }
        let coff = e + 4;
        let nsec = u16_at(d, coff + 2)? as usize;
        let optsz = u16_at(d, coff + 16)? as usize;
        let opt = coff + 20;
        let magic = u16_at(d, opt)?;
        let dd = opt + if magic == 0x10B { 96 } else { 112 };
        let rva_res = u32_at(d, dd + 2 * 8)?;

        let mut sections = Vec::with_capacity(nsec);
        let sec = opt + optsz;
        for i in 0..nsec {
            let o = sec + i * 40;
            let vsize = u32_at(d, o + 8)?;
            let vaddr = u32_at(d, o + 12)?;
            let rawsz = u32_at(d, o + 16)?;
            let rawptr = u32_at(d, o + 20)?;
            sections.push((vaddr, vsize.max(rawsz), rawptr, rawsz));
        }
        if rva_res == 0 {
            return Err("没有资源段".into());
        }
        Ok(Pe { d, rva_res, sections })
    }

    fn off(&self, rva: u32) -> Result<usize, String> {
        for (va, span, ptr, rawsz) in &self.sections {
            if *va <= rva && rva < va.wrapping_add(*span) {
                let delta = rva - va;
                if delta >= *rawsz {
                    return Err("RVA 落在未初始化区".into());
                }
                return Ok((ptr + delta) as usize);
            }
        }
        Err(format!("RVA 0x{rva:x} 未映射"))
    }

    fn dir_entries(&self, base: usize, rel: u32) -> Result<Vec<(u32, u32)>, String> {
        let o = base + rel as usize;
        let named = u16_at(self.d, o + 12)? as usize;
        let ids = u16_at(self.d, o + 14)? as usize;
        let mut out = Vec::with_capacity(named + ids);
        let mut p = o + 16;
        for _ in 0..(named + ids) {
            out.push((u32_at(self.d, p)?, u32_at(self.d, p + 4)?));
            p += 8;
        }
        Ok(out)
    }

    /// (类型, 名字) → 数据
    fn walk(&self) -> Result<BTreeMap<(u32, u32), Vec<u8>>, String> {
        let base = self.off(self.rva_res)?;
        let mut out = BTreeMap::new();
        for (tname, tdata) in self.dir_entries(base, 0)? {
            if tname & 0x8000_0000 != 0 || tdata & 0x8000_0000 == 0 {
                continue;
            }
            let tid = tname & 0xFFFF;
            for (nname, ndata) in self.dir_entries(base, tdata & 0x7FFF_FFFF)? {
                if nname & 0x8000_0000 != 0 || ndata & 0x8000_0000 == 0 {
                    continue;
                }
                let nid = nname & 0xFFFF;
                for (_lang, ldata) in self.dir_entries(base, ndata & 0x7FFF_FFFF)? {
                    if ldata & 0x8000_0000 != 0 {
                        continue;
                    }
                    let o = base + (ldata & 0x7FFF_FFFF) as usize;
                    let rva = u32_at(self.d, o)?;
                    let size = u32_at(self.d, o + 4)? as usize;
                    let start = self.off(rva)?;
                    if let Some(blob) = self.d.get(start..start + size) {
                        out.insert((tid, nid), blob.to_vec());
                    }
                }
            }
        }
        Ok(out)
    }
}

/// 返回 (png 字节, (宽, 高), 该组所有档位)
pub fn extract(path: &Path) -> Result<(Vec<u8>, (u32, u32), Vec<IconMeta>), String> {
    let data = fs::read(path).map_err(|e| format!("读不了文件: {e}"))?;
    let pe = Pe::parse(&data)?;
    let res = pe.walk()?;

    let groups: Vec<u32> = res
        .keys()
        .filter(|(t, _)| *t == RT_GROUP_ICON)
        .map(|(_, n)| *n)
        .collect();
    if groups.is_empty() {
        return Err("这个 exe 没有图标资源".into());
    }

    let mut best: Option<(u32, Vec<u8>, Vec<IconMeta>, Vec<u8>)> = None;
    for gid in groups {
        let Some(grp) = res.get(&(RT_GROUP_ICON, gid)) else {
            continue;
        };
        if grp.len() < 6 {
            continue;
        }
        let count = u16_at(grp, 4).unwrap_or(0) as usize;
        let mut metas = Vec::new();
        let mut blobs: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut chosen: Option<(u32, Vec<u8>)> = None;

        for i in 0..count {
            let e = 6 + i * 14;
            let w = u32::from(grp.get(e).copied().unwrap_or(0));
            let h = u32::from(grp.get(e + 1).copied().unwrap_or(0));
            let bpp = u16_at(grp, e + 6).unwrap_or(0);
            let nid = u16_at(grp, e + 12).unwrap_or(0) as u32;
            let Some(blob) = res.get(&(RT_ICON, nid)) else {
                continue;
            };
            let (w, h) = (if w == 0 { 256 } else { w }, if h == 0 { 256 } else { h });
            metas.push(IconMeta { w, h, bpp });
            blobs.push((w, blob.clone()));
            if chosen.as_ref().map(|(cw, _)| w > *cw).unwrap_or(true) {
                chosen = Some((w, blob.clone()));
            }
        }
        let Some((w, blob)) = chosen else {
            continue;
        };
        if best.as_ref().map(|(bw, ..)| w > *bw).unwrap_or(true) {
            best = Some((w, blob, metas, Vec::new()));
            let _ = blobs;
        }
    }

    let Some((_w, blob, metas, _)) = best else {
        return Err("图标组里没有可用条目".into());
    };

    let (iw, ih, rgba) = decode_image(&blob)?;
    let png = encode_png(iw, ih, &rgba)?;
    Ok((png, (iw, ih), metas))
}

fn is_png(b: &[u8]) -> bool {
    b.len() > 8 && b[0] == 0x89 && &b[1..4] == b"PNG"
}

fn decode_image(blob: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if is_png(blob) {
        // 内嵌 PNG：用 png crate 解出来，统一成 RGBA
        let decoder = png::Decoder::new(std::io::Cursor::new(blob.to_vec()));
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let size = reader.output_buffer_size().unwrap_or(0);
        let mut buf = vec![0u8; size];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        let (w, h) = (info.width, info.height);
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
            png::ColorType::Rgb => buf[..info.buffer_size()]
                .chunks(3)
                .flat_map(|c| [c[0], c[1], c[2], 255])
                .collect(),
            _ => {
                return Err("不支持的 PNG 颜色类型".into());
            }
        };
        return Ok((w, h, rgba));
    }
    decode_dib(blob)
}

fn decode_dib(b: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let hdr = u32_at(b, 0)? as usize;
    let w = i32_at(b, 4)?;
    let h2 = i32_at(b, 8)?;
    let bpp = u16_at(b, 14)? as usize;
    let comp = u32_at(b, 16)?;
    let clr_used = u32_at(b, 32).unwrap_or(0) as usize;

    if comp != 0 && comp != 3 {
        return Err(format!("不支持的 DIB 压缩 {comp}"));
    }
    let w = w as u32 as usize;
    let h = (h2.unsigned_abs() as usize) / 2;
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return Err("DIB 尺寸异常".into());
    }

    let mut palette = Vec::new();
    if bpp <= 8 {
        let n = if clr_used == 0 { 1usize << bpp } else { clr_used };
        let p = hdr;
        for i in 0..n {
            let o = p + i * 4;
            let (b0, g0, r0) = (
                b.get(o).copied().unwrap_or(0),
                b.get(o + 1).copied().unwrap_or(0),
                b.get(o + 2).copied().unwrap_or(0),
            );
            palette.push([r0, g0, b0]);
        }
    }

    let xor_off = hdr + palette.len() * 4;
    let xor_stride = ((w * bpp + 31) / 32) * 4;
    let mask_off = xor_off + xor_stride * h;
    let mask_stride = ((w + 31) / 32) * 4;

    let mut rgba = vec![0u8; w * h * 4];
    let mut any_alpha = false;

    for y in 0..h {
        let src_y = h - 1 - y; // BMP 自下而上
        let row = xor_off + src_y * xor_stride;
        let mrow = mask_off + src_y * mask_stride;
        for x in 0..w {
            let (r, g, b_, a) = match bpp {
                32 => {
                    let o = row + x * 4;
                    let (bb, gg, rr, aa) = (
                        b.get(o).copied().unwrap_or(0),
                        b.get(o + 1).copied().unwrap_or(0),
                        b.get(o + 2).copied().unwrap_or(0),
                        b.get(o + 3).copied().unwrap_or(0),
                    );
                    if aa != 0 {
                        any_alpha = true;
                    }
                    (rr, gg, bb, aa)
                }
                24 => {
                    let o = row + x * 3;
                    (
                        b.get(o + 2).copied().unwrap_or(0),
                        b.get(o + 1).copied().unwrap_or(0),
                        b.get(o).copied().unwrap_or(0),
                        255,
                    )
                }
                8 => {
                    let idx = b.get(row + x).copied().unwrap_or(0) as usize;
                    let p = palette.get(idx).copied().unwrap_or([0, 0, 0]);
                    (p[0], p[1], p[2], 255)
                }
                4 => {
                    let byte = b.get(row + x / 2).copied().unwrap_or(0);
                    let idx = if x % 2 == 0 { byte >> 4 } else { byte & 0x0F } as usize;
                    let p = palette.get(idx).copied().unwrap_or([0, 0, 0]);
                    (p[0], p[1], p[2], 255)
                }
                1 => {
                    let byte = b.get(row + x / 8).copied().unwrap_or(0);
                    let idx = ((byte >> (7 - (x % 8))) & 1) as usize;
                    let p = palette.get(idx).copied().unwrap_or([0, 0, 0]);
                    (p[0], p[1], p[2], 255)
                }
                other => return Err(format!("不支持的位深 {other}")),
            };
            // AND 掩码：1 表示透明
            let transparent = b
                .get(mrow + x / 8)
                .map(|byte| (byte >> (7 - (x % 8))) & 1 == 1)
                .unwrap_or(false);
            let alpha = if bpp == 32 && any_alpha {
                a
            } else if transparent {
                0
            } else if bpp == 32 {
                a.max(255)
            } else {
                255
            };
            let o = (y * w + x) * 4;
            rgba[o] = r;
            rgba[o + 1] = g;
            rgba[o + 2] = b_;
            rgba[o + 3] = alpha;
        }
    }

    // 32bpp 但整张 alpha 全 0（很常见）：当作不透明
    if bpp == 32 && !any_alpha {
        for px in rgba.chunks_mut(4) {
            px[3] = 255;
        }
    }
    Ok((w as u32, h as u32, rgba))
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// 抠图并落到 `~/.cache/carboy/icons/<slug>.png`
pub fn extract_to_cache(exe: &Path, slug: &str) -> Result<(std::path::PathBuf, (u32, u32), Vec<IconMeta>), String> {
    let (png, size, metas) = extract(exe)?;
    let dir = super::desktop::cache_dir().join("icons");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = dir.join(format!("{slug}.png"));
    fs::write(&out, png).map_err(|e| e.to_string())?;
    Ok((out, size, metas))
}
