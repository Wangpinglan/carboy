//! .desktop 的生成、解析、安装。纯 std，不依赖 GTK。
//!
//! 规则全部来自 wine / Proton / umu 源码实测，注释里标了出处，别再凭感觉改。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use super::system;

/// hicolor 支持的尺寸档位
pub const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// Desktop Entry Spec：需要引号包裹的字符
const RESERVED: &str = " \t\n\"'\\><~|&;$*?#()`";

// ---------------------------------------------------------------- 路径

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

pub fn data_home() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share")
}

pub fn apps_dir() -> PathBuf {
    data_home().join("applications")
}

pub fn icons_root() -> PathBuf {
    data_home().join("icons/hicolor")
}

pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join("carboy")
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("carboy")
}

// ---------------------------------------------------------------- 标识

/// 单个 Exec 参数的转义：先处理字面 `%`，再按规范转义引号内特殊字符。
pub fn esc(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }
    let mut body = String::with_capacity(arg.len() + 4);
    for ch in arg.chars() {
        match ch {
            '"' | '`' | '$' | '\\' => {
                body.push('\\');
                body.push(ch);
            }
            '%' => body.push_str("%%"),
            _ => body.push(ch),
        }
    }
    if arg.chars().any(|c| RESERVED.contains(c)) {
        format!("\"{body}\"")
    } else {
        body
    }
}

fn short_sha1(s: &str) -> String {
    let mut h = Sha1::new();
    h.update(s.as_bytes());
    let d = h.finalize();
    d.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// 文件名 / 图标名。纯 ASCII 化，中文名退化成 `app-<sha1 前 8 位>`。
pub fn slugify(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    let mut raw = String::with_capacity(lower.len());
    for ch in lower.chars() {
        raw.push(if ch.is_ascii_alphanumeric() { ch } else { '-' });
    }
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch == '-' && out.ends_with('-') {
            continue;
        }
        out.push(ch);
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        format!("app-{}", short_sha1(name.trim()))
    } else {
        out
    }
}

/// umu 里写死的一条正则：只有 `umu-xxx` 才会被拆成 SteamAppId。
/// 见 umu_run.py:279 `if match(r"^umu-[\d\w]+$", env["UMU_ID"])`
pub fn is_umu_gameid(gid: &str) -> bool {
    match gid.strip_prefix("umu-") {
        Some(rest) => !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        None => false,
    }
}

pub fn auto_gameid(name: &str) -> String {
    let s = slugify(name);
    let s = s.strip_prefix("app-").unwrap_or(&s);
    format!("umu-{s}")
}

// ---------------------------------------------------------------- 表单

#[derive(Debug, Clone, Default)]
pub struct Form {
    pub name: String,
    pub exe: String,
    pub args: Vec<String>,
    pub prefix: String,
    pub proton: String,
    pub wine: String,
    pub umu_run: String,
    pub gameid: String,
    pub wmclass: String,
    pub workdir: String,
    pub icon: String,
    pub env: Vec<(String, String)>,
    pub umu_mode: bool,
    pub slug: Option<String>,
}

impl Form {
    pub fn slug(&self) -> String {
        self.slug
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| slugify(if self.name.trim().is_empty() { "未命名" } else { &self.name }))
    }

    pub fn workdir(&self) -> String {
        if !self.workdir.is_empty() {
            return self.workdir.clone();
        }
        Path::new(&self.exe)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}

/// 返回 (wm_class, 来源说明)
pub fn derive_wmclass(f: &Form) -> (String, String) {
    if !f.wmclass.trim().is_empty() {
        return (f.wmclass.trim().to_string(), "手动指定 / 实测".to_string());
    }
    if f.umu_mode {
        let gid = f.gameid.trim();
        if gid.is_empty() {
            return (
                "steam_proton".to_string(),
                "Proton 但无 GAMEID，所有游戏会撞车".to_string(),
            );
        }
        if !is_umu_gameid(gid) {
            return (
                "steam_app_0".to_string(),
                format!("GAMEID={gid} 不以 umu- 开头，umu 会强制 SteamAppId=0"),
            );
        }
        let appid = gid.split_once('-').map(|(_, b)| b).unwrap_or(gid);
        return (
            format!("steam_app_{appid}"),
            "由 GAMEID 派生（Proton 补丁）".to_string(),
        );
    }
    if !f.exe.is_empty() {
        let base = Path::new(&f.exe)
            .file_name()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        return (base, "wine：exe 文件名小写".to_string());
    }
    (String::new(), "无法推导".to_string())
}

fn set_pair(pairs: &mut Vec<(String, String)>, key: &str, value: &str) {
    if let Some(slot) = pairs.iter_mut().find(|(k, _)| k == key) {
        slot.1 = value.to_string();
    } else {
        pairs.push((key.to_string(), value.to_string()));
    }
}

pub fn build_exec(f: &Form) -> String {
    let mut parts: Vec<String> = Vec::new();
    let wants_env = f.umu_mode || !f.prefix.is_empty() || !f.gameid.is_empty();
    if wants_env {
        let mut pairs: Vec<(String, String)> = Vec::new();
        if !f.prefix.is_empty() {
            set_pair(&mut pairs, "WINEPREFIX", &f.prefix);
        }
        for (k, v) in &f.env {
            if !k.trim().is_empty() {
                set_pair(&mut pairs, k.trim(), v);
            }
        }
        if f.umu_mode {
            if !f.gameid.is_empty() {
                set_pair(&mut pairs, "GAMEID", &f.gameid);
            }
            if !f.proton.is_empty() {
                set_pair(&mut pairs, "PROTONPATH", &f.proton);
            }
        }
        parts.push("env".to_string());
        for (k, v) in pairs {
            parts.push(format!("{k}={}", esc(&v)));
        }
    }

    let launcher = if f.umu_mode {
        let resolved = system::resolve_umu(&f.umu_run);
        if !resolved.is_empty() {
            resolved
        } else if f.umu_run.trim().is_empty() {
            "umu-run".to_string()
        } else {
            f.umu_run.clone()
        }
    } else if !f.wine.trim().is_empty() {
        let resolved = system::resolve_wine(&f.wine);
        if !resolved.is_empty() {
            resolved
        } else {
            f.wine.clone()
        }
    } else {
        "wine".to_string()
    };
    parts.push(esc(&launcher));

    if !f.exe.is_empty() {
        parts.push(esc(&f.exe));
    }
    for a in &f.args {
        parts.push(esc(a));
    }
    parts.join(" ")
}

pub fn desktop_text(f: &Form, wm_class: &str) -> String {
    let slug = f.slug();
    let mut lines: Vec<String> = vec![
        "[Desktop Entry]".into(),
        "Version=1.5".into(),
        "Type=Application".into(),
        format!("Name={}", if f.name.trim().is_empty() { "未命名" } else { f.name.trim() }),
        format!("Exec={}", build_exec(f)),
    ];
    let wd = f.workdir();
    if !wd.is_empty() {
        lines.push(format!("Path={wd}"));
    }
    lines.push(format!("Icon={slug}"));
    lines.push("Terminal=false".into());
    lines.push("Categories=Game;".into());
    // Wine 不发 startup-complete，写 true 会让 dock 一直转
    lines.push("StartupNotify=false".into());
    if !wm_class.is_empty() {
        lines.push(format!("StartupWMClass={wm_class}"));
    }
    if !f.gameid.is_empty() {
        lines.push(format!("X-Carboy-GameId={}", f.gameid));
    }
    if !f.prefix.is_empty() {
        lines.push(format!("X-Carboy-Prefix={}", f.prefix));
    }
    lines.push(format!("X-Carboy-Generated={}", env!("CARGO_PKG_VERSION")));
    lines.join("\n") + "\n"
}

// ---------------------------------------------------------------- 解析

/// 类 shell 拆分：支持单双引号与反斜杠转义。
pub fn split_quoted(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut quote: Option<char> = None;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else if c == '\\' && q == '"' {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                } else {
                    cur.push(c);
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    has = true;
                }
                '\\' => {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                        has = true;
                    }
                }
                c if c.is_whitespace() => {
                    if has || !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                        has = false;
                    }
                }
                c => {
                    cur.push(c);
                    has = true;
                }
            },
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[derive(Debug, Default, Clone)]
pub struct ExecParts {
    pub env: Vec<(String, String)>,
    pub launcher: String,
    pub exe: Option<String>,
    pub args: Vec<String>,
}

pub fn parse_exec(s: &str) -> ExecParts {
    let parts = split_quoted(s);
    let mut i = 0;
    let mut env = Vec::new();
    if parts.first().map(|p| p == "env").unwrap_or(false) {
        i = 1;
        while i < parts.len() && parts[i].contains('=') {
            let (k, v) = parts[i].split_once('=').unwrap();
            env.push((k.to_string(), v.to_string()));
            i += 1;
        }
    }
    let rest = &parts[i.min(parts.len())..];
    ExecParts {
        env,
        launcher: rest.first().cloned().unwrap_or_default(),
        exe: rest.get(1).cloned(),
        args: if rest.len() > 2 { rest[2..].to_vec() } else { Vec::new() },
    }
}

pub fn parse_desktop_file(p: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(text) = fs::read_to_string(p) else {
        return out;
    };
    for raw in text.lines() {
        let s = raw.trim();
        if s.is_empty() || s.starts_with('#') || s.starts_with('[') {
            continue;
        }
        if let Some((k, v)) = s.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

// ---------------------------------------------------------------- 管理已生成

#[derive(Debug, Clone)]
pub struct Entry {
    pub slug: String,
    pub info: BTreeMap<String, String>,
}

impl Entry {
    pub fn get(&self, key: &str) -> &str {
        self.info.get(key).map(|s| s.as_str()).unwrap_or("")
    }

    pub fn name(&self) -> &str {
        let n = self.get("Name");
        if n.is_empty() {
            &self.slug
        } else {
            n
        }
    }

    pub fn path(&self) -> PathBuf {
        apps_dir().join(format!("{}.desktop", self.slug))
    }

    /// 由已生成的条目反填表单（编辑用）
    pub fn to_form(&self) -> Form {
        let pe = parse_exec(self.get("Exec"));
        let mut env = pe.env.clone();
        let take = |env: &mut Vec<(String, String)>, k: &str| -> String {
            if let Some(pos) = env.iter().position(|(key, _)| key == k) {
                env.remove(pos).1
            } else {
                String::new()
            }
        };
        let env_prefix = take(&mut env, "WINEPREFIX");
        let env_proton = take(&mut env, "PROTONPATH");
        let env_gid = take(&mut env, "GAMEID");

        let launcher_name = Path::new(&pe.launcher)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let umu_mode = launcher_name.starts_with("umu");

        let prefix = first_nonempty(&[self.get("X-Carboy-Prefix"), &env_prefix]);
        let proton = first_nonempty(&[self.get("X-Carboy-Proton"), &env_proton]);
        let gameid = first_nonempty(&[self.get("X-Carboy-GameId"), &env_gid]);

        let icon = {
            let f256 = icons_root().join("256x256/apps").join(format!("{}.png", self.slug));
            if f256.exists() {
                f256.to_string_lossy().to_string()
            } else if self.get("Icon").starts_with('/') {
                self.get("Icon").to_string()
            } else {
                String::new()
            }
        };

        Form {
            name: self.get("Name").to_string(),
            exe: pe.exe.unwrap_or_default(),
            args: pe.args,
            prefix,
            proton,
            wine: if umu_mode { String::new() } else { pe.launcher.clone() },
            umu_run: if umu_mode { pe.launcher.clone() } else { String::new() },
            gameid,
            wmclass: self.get("StartupWMClass").to_string(),
            workdir: self.get("Path").to_string(),
            icon,
            env,
            umu_mode,
            slug: Some(self.slug.clone()),
        }
    }
}

fn first_nonempty(cands: &[&str]) -> String {
    cands
        .iter()
        .find(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

pub fn list_generated() -> Vec<Entry> {
    let dir = apps_dir();
    let Ok(rd) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().map(|x| x != "desktop").unwrap_or(true) {
            continue;
        }
        let info = parse_desktop_file(&p);
        if info.contains_key("X-Carboy-Generated") {
            out.push(Entry {
                slug: p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                info,
            });
        }
    }
    out.sort_by(|a, b| a.name().to_lowercase().cmp(&b.name().to_lowercase()));
    out
}

pub fn remove_generated(entry: &Entry) -> Vec<PathBuf> {
    let mut gone = Vec::new();
    let root = icons_root();
    for n in SIZES {
        let f = root.join(format!("{n}x{n}/apps")).join(format!("{}.png", entry.slug));
        if f.exists() && fs::remove_file(&f).is_ok() {
            gone.push(f);
        }
    }
    let svg = root.join("scalable/apps").join(format!("{}.svg", entry.slug));
    if svg.exists() && fs::remove_file(&svg).is_ok() {
        gone.push(svg);
    }
    let p = entry.path();
    if p.exists() && fs::remove_file(&p).is_ok() {
        gone.push(p);
    }
    refresh();
    gone
}

// ---------------------------------------------------------------- 图标安装

pub fn install_icon(src: &Path, slug: &str) -> Vec<PathBuf> {
    let mut done = Vec::new();
    let root = icons_root();
    if src.extension().map(|e| e.eq_ignore_ascii_case("svg")).unwrap_or(false) {
        let dir = root.join("scalable/apps");
        let _ = fs::create_dir_all(&dir);
        let dst = dir.join(format!("{slug}.svg"));
        if fs::copy(src, &dst).is_ok() {
            done.push(dst);
        }
        return done;
    }

    let magick = which_first(&["magick", "convert"]);
    for n in SIZES {
        let dir = root.join(format!("{n}x{n}/apps"));
        let _ = fs::create_dir_all(&dir);
        let dst = dir.join(format!("{slug}.png"));
        let ok = match &magick {
            Some(bin) => Command::new(bin)
                .args([
                    src.to_string_lossy().as_ref(),
                    "-resize",
                    &format!("{n}x{n}"),
                    "-background",
                    "none",
                    "-gravity",
                    "center",
                    "-extent",
                    &format!("{n}x{n}"),
                    dst.to_string_lossy().as_ref(),
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false),
            None => false,
        };
        if !ok {
            let _ = fs::copy(src, &dst);
        }
        done.push(dst);
    }
    done
}

fn which_first(names: &[&str]) -> Option<String> {
    for n in names {
        if let Some(p) = system::which(n) {
            return Some(p);
        }
    }
    None
}

pub fn ensure_index_theme() {
    let idx = icons_root().join("index.theme");
    if idx.exists() {
        return;
    }
    let src = Path::new("/usr/share/icons/hicolor/index.theme");
    if src.is_file() {
        if let Some(parent) = idx.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::copy(src, &idx);
    }
}

/// 刷新图标缓存与桌面项索引，返回实际跑过的命令名。
pub fn refresh() -> Vec<String> {
    let mut done = Vec::new();
    if system::which("update-desktop-database").is_some() {
        let _ = Command::new("update-desktop-database")
            .arg(apps_dir())
            .output();
        done.push("update-desktop-database".to_string());
    }
    ensure_index_theme();
    if system::which("gtk-update-icon-cache").is_some() && icons_root().join("index.theme").exists() {
        let _ = Command::new("gtk-update-icon-cache")
            .args(["-f", "-t"])
            .arg(icons_root())
            .output();
        done.push("gtk-update-icon-cache".to_string());
    }
    done
}

/// 写入桌面项；`install` 为 false 时只写文件不动图标。
pub fn write_entry(f: &Form, wm: &str) -> std::io::Result<PathBuf> {
    let dir = apps_dir();
    fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{}.desktop", f.slug()));
    fs::write(&target, desktop_text(f, wm))?;
    Ok(target)
}

// ---------------------------------------------------------------- 偏好

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub hint: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub proton: String,
    #[serde(default)]
    pub workdir: String,
    #[serde(default)]
    pub umu: String,
    #[serde(default)]
    pub wine: String,
    #[serde(default)]
    pub env_presets: Vec<Preset>,
}

pub fn default_presets() -> Vec<Preset> {
    [
        ("MANGOHUD", "1", "MangoHud 覆盖层"),
        ("DXVK_HUD", "fps,frametimes", "DXVK 自带 HUD"),
        ("DXVK_ASYNC", "1", "dxvk-async（部分分支）"),
        ("PROTON_ENABLE_NVAPI", "1", "Proton 下开启 NVAPI"),
        ("VKD3D_CONFIG", "dxr", "VKD3D 光线追踪"),
        ("LSFG_MULTIPLIER", "2", "lsfg-vk 帧生成倍数"),
        ("WINEDLLOVERRIDES", "d3d11=n", "DLL 覆盖"),
    ]
    .into_iter()
    .map(|(k, v, h)| Preset {
        key: k.into(),
        value: v.into(),
        hint: h.into(),
    })
    .collect()
}

pub fn load_settings() -> Settings {
    let p = config_dir().join("settings.json");
    match fs::read_to_string(&p) {
        Ok(text) => {
            let mut s: Settings = serde_json::from_str(&text).unwrap_or_default();
            if s.env_presets.is_empty() {
                s.env_presets = default_presets();
            }
            s
        }
        Err(_) => Settings {
            env_presets: default_presets(),
            ..Default::default()
        },
    }
}

pub fn save_settings(s: &Settings) {
    let dir = config_dir();
    let _ = fs::create_dir_all(&dir);
    let p = dir.join("settings.json");
    let tmp = dir.join("settings.json.tmp");
    if let Ok(text) = serde_json::to_string_pretty(s) {
        if fs::write(&tmp, text).is_ok() {
            let _ = fs::rename(&tmp, &p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_specials() {
        assert_eq!(esc("/tmp/a b/x.exe"), "\"/tmp/a b/x.exe\"");
        assert_eq!(esc("100%"), "100%%");
    }

    #[test]
    fn gameid_prefix_matters() {
        assert!(is_umu_gameid("umu-abc"));
        assert!(!is_umu_gameid("carboy-abc"));
        assert_eq!(auto_gameid("Hades"), "umu-hades");
    }

    #[test]
    fn remove_generated_cleans_desktop_and_icons() {
        let root = std::env::temp_dir().join(format!("carboy-del-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("applications")).unwrap();
        fs::create_dir_all(root.join("icons/hicolor/256x256/apps")).unwrap();
        std::env::set_var("XDG_DATA_HOME", &root);

        let desktop = root.join("applications/app-deadbeef.desktop");
        fs::write(
            &desktop,
            "[Desktop Entry]\nName=T\nIcon=app-deadbeef\nX-Carboy-Generated=0.1.0\n",
        )
        .unwrap();
        let icon = root.join("icons/hicolor/256x256/apps/app-deadbeef.png");
        fs::write(&icon, b"png").unwrap();

        let entries = list_generated();
        assert_eq!(entries.len(), 1, "应该只认出带标记的那一条");
        let gone = remove_generated(&entries[0]);
        assert!(gone.len() >= 2, "桌面项与图标都该删掉");
        assert!(!desktop.exists());
        assert!(!icon.exists());

        std::env::remove_var("XDG_DATA_HOME");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn wmclass_rules() {
        let mut f = Form {
            umu_mode: true,
            gameid: "umu-1234".into(),
            ..Default::default()
        };
        assert_eq!(derive_wmclass(&f).0, "steam_app_1234");
        f.gameid = "carboy-1".into();
        assert_eq!(derive_wmclass(&f).0, "steam_app_0");
        f.umu_mode = false;
        f.exe = "/games/Hades.exe".into();
        assert_eq!(derive_wmclass(&f).0, "hades.exe");
    }
}
