//! 外部世界：找 umu / wine、读窗口、查依赖、拼构建命令。全是子进程，不碰 GTK。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------- 基本

pub fn which(name: &str) -> Option<String> {
    if name.contains('/') {
        let p = Path::new(name);
        return is_exec(p).then(|| name.to_string());
    }
    for dir in std::env::var("PATH").unwrap_or_default().split(':') {
        if dir.is_empty() {
            continue;
        }
        let p = Path::new(dir).join(name);
        if is_exec(&p) {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}

fn is_exec(p: &Path) -> bool {
    fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn expand(value: &str) -> PathBuf {
    let v = value.trim();
    if let Some(rest) = v.strip_prefix("~/") {
        return super::desktop::home().join(rest);
    }
    if v == "~" {
        return super::desktop::home();
    }
    PathBuf::from(v)
}

// ---------------------------------------------------------------- umu

const UMU_CANDIDATES: [&str; 6] = [
    "umu-run",
    "bin/umu-run",
    "builddir/umu-run",
    "venv/bin/umu-run",
    ".venv/bin/umu-run",
    "tools/umu-run-posix",
];

pub fn find_umu() -> Option<String> {
    if let Some(p) = which("umu-run") {
        return Some(p);
    }
    let home = super::desktop::home();
    let cands = [
        home.join(".local/bin/umu-run"),
        home.join(".local/share/umu/umu-run"),
        home.join(".local/share/pipx/venvs/umu-launcher/bin/umu-run"),
        home.join(".local/share/umu-launcher/venv/bin/umu-run"),
        PathBuf::from("/usr/local/bin/umu-run"),
        PathBuf::from("/usr/bin/umu-run"),
    ];
    cands
        .iter()
        .find(|p| is_exec(p))
        .map(|p| p.to_string_lossy().to_string())
}

/// 目录 / 可执行文件 / 命令名 → 真正的 umu-run 路径
pub fn resolve_umu(value: &str) -> String {
    if value.trim().is_empty() {
        return String::new();
    }
    let p = expand(value);
    if p.is_dir() {
        for rel in UMU_CANDIDATES {
            let c = p.join(rel);
            if is_exec(&c) {
                return c.to_string_lossy().to_string();
            }
        }
        return String::new();
    }
    if p.is_file() {
        return p.to_string_lossy().to_string();
    }
    which(value.trim()).unwrap_or_default()
}

pub fn is_umu_source(d: &Path) -> bool {
    d.join("configure.sh").is_file() && d.join("pyproject.toml").is_file()
}

// ---------------------------------------------------------------- wine

const WINE_CANDIDATES: [&str; 5] = ["wine", "bin/wine", "build/wine", "wine64", "bin/wine64"];

pub fn is_wine_source(d: &Path) -> bool {
    d.join("configure").is_file() && d.join("dlls").is_dir() && d.join("loader").is_dir()
}

pub fn find_wine() -> Option<String> {
    if let Some(p) = which("wine") {
        return Some(p);
    }
    for p in [
        PathBuf::from("/usr/local/bin/wine"),
        PathBuf::from("/usr/bin/wine"),
        super::desktop::home().join(".local/bin/wine"),
    ] {
        if is_exec(&p) {
            return Some(p.to_string_lossy().to_string());
        }
    }
    // Lutris / Wine-GE 的 runner 目录，按名字倒序取最新
    for root in [
        super::desktop::home().join(".local/share/lutris/runners/wine"),
        super::desktop::home().join(".local/share/wine-ge"),
    ] {
        let Ok(rd) = fs::read_dir(&root) else { continue };
        let mut names: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        names.sort();
        names.reverse();
        for dir in names {
            let c = dir.join("bin/wine");
            if is_exec(&c) {
                return Some(c.to_string_lossy().to_string());
            }
        }
    }
    None
}

pub fn resolve_wine(value: &str) -> String {
    if value.trim().is_empty() {
        return String::new();
    }
    let p = expand(value);
    if p.is_dir() {
        for rel in WINE_CANDIDATES {
            let c = p.join(rel);
            if is_exec(&c) {
                return c.to_string_lossy().to_string();
            }
        }
        return String::new();
    }
    if p.is_file() {
        return p.to_string_lossy().to_string();
    }
    which(value.trim()).unwrap_or_default()
}

// ---------------------------------------------------------------- proton

/// umu 只认 Proton 根目录：`resolve_runtime()` 在里面找不到 `toolmanifest.vdf`
/// 就直接 FileNotFoundError。
pub fn is_proton_root(d: &Path) -> bool {
    d.join("toolmanifest.vdf").is_file()
}

/// Proton 目录 → 真正的 Proton 根目录。
///
/// 用户十有八九选的是 `~/.local/share/Steam/compatibilitytools.d` 这个**容器目录**
/// （里面装着好几个 Proton），直接当 PROTONPATH 传给 umu 是起不来的，
/// 所以这里往下探一层，按名字倒序取最新那个（和 find_wine 一个口味）。
/// 版本名（`GE-Proton11-12`）或压根不存在的路径原样返回，交给 umu 自己认。
pub fn resolve_proton(value: &str) -> String {
    let v = value.trim();
    if v.is_empty() {
        return String::new();
    }
    let p = expand(v);
    if !p.is_dir() {
        return v.to_string();
    }
    if is_proton_root(&p) {
        return p.to_string_lossy().to_string();
    }
    let mut hits: Vec<PathBuf> = fs::read_dir(&p)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|c| c.is_dir() && is_proton_root(c))
                .collect()
        })
        .unwrap_or_default();
    if hits.is_empty() {
        return p.to_string_lossy().to_string();
    }
    hits.sort();
    hits.pop()
        .map(|c| c.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string_lossy().to_string())
}

// ---------------------------------------------------------------- 前缀

/// 建完前缀要钉死的 Windows 版本。
///
/// Wine 11（dwproton-11.0-x）的 `wineboot` 默认建出来是 Windows 11 build 22000，
/// 而 WineGUI 建出来的 bottle 是 win10 build 19045 —— 它在 `BottleManager::new_bottle()`
/// 里建完 bottle 后**无条件**补跑一次 `winetricks win10`。原神这类游戏在
/// “Win11 build 22000 + ReleaseId 2009”这种混搭版本号上会直接闪退，所以这里照抄那一步。
///
/// winetricks 的 win10 动词最终就是 `winecfg -v win10`（见 Proton 自带的
/// `protonfixes/files/bin/winetricks` 里 `w_set_winver`），所以 Wine 模式不需要
/// 装 winetricks —— 这也守住了“零运行时解释器”那条铁律。
pub const PREFIX_WINVER: &str = "win10";

/// POSIX 单引号转义，`sh -c` 里安全。
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// 前缀初始化用到的 runner 信息。
pub struct PrefixInit {
    pub umu_mode: bool,
    /// umu-run 路径（umu 模式）
    pub umu: String,
    /// GAMEID（可空，空则 umu 用 umu-default）
    pub gameid: String,
    /// PROTONPATH（可空，空则 umu 自己下载 UMU-Proton）
    pub proton: String,
    /// wine 可执行文件（Wine 模式）
    pub wine: String,
}

impl PrefixInit {
    /// 按顺序要跑的命令：① 建前缀 ② 把 Windows 版本设成 win10。
    pub fn cmds(&self, prefix: &Path) -> Vec<String> {
        let win = shell_quote(&prefix.to_string_lossy());
        // 调用方环境里残留的 WINE / WINESERVER / WINELOADER 会把 Proton 自己的
        // wine 顶掉（WineGUI 跑 runner 前同样先 `env -u` 这三项）。
        let clean = "env -u WINE -u WINESERVER -u WINELOADER";
        if self.umu_mode {
            let mut env = format!("WINEPREFIX={win} GAMEID={}", shell_quote(&self.gameid));
            if !self.proton.is_empty() {
                env.push_str(&format!(" PROTONPATH={}", shell_quote(&self.proton)));
            }
            let umu = shell_quote(&self.umu);
            vec![
                // 上游 umu_run.py 认 `createprefix` 这个特殊命令名
                // （`is_createpfx = is_cmd and (not cmd or cmd == "createprefix")`）
                format!("{clean} {env} {umu} createprefix 2>&1"),
                // 走 umu 的 winetricks 动词，用的是 Proton 自带的那份 winetricks
                format!("{clean} {env} {umu} winetricks {PREFIX_WINVER} 2>&1"),
            ]
        } else {
            let w = shell_quote(&self.wine);
            vec![
                format!("{clean} WINEPREFIX={win} {w} wineboot -i 2>&1"),
                format!("{clean} WINEPREFIX={win} {w} winecfg -v {PREFIX_WINVER} 2>&1"),
            ]
        }
    }
}

// ---------------------------------------------------------------- 前缀工具

/// 一份已有前缀是谁建的 —— 决定跑 winetricks 时该用哪份 wine。
#[derive(Debug, Clone, PartialEq)]
pub enum PrefixRunner {
    /// Proton 前缀：root 是建它的那份 Proton 的根目录
    Proton { root: String },
    /// 普通 Wine 前缀
    Wine { wine: String },
}

#[derive(Debug, Clone)]
pub struct PrefixInfo {
    pub prefix: PathBuf,
    pub runner: PrefixRunner,
    /// 版本标识（Proton 前缀才有，比如 dwproton-11.0-17），界面显示用
    pub label: String,
}

/// 有 system.reg + drive_c 才算一个能用的前缀
pub fn is_prefix(dir: &Path) -> bool {
    dir.join("system.reg").is_file() && dir.join("drive_c").is_dir()
}

fn prefix_version(dir: &Path) -> String {
    fs::read_to_string(dir.join("version"))
        .map(|s| s.lines().next().unwrap_or("").trim().to_string())
        .unwrap_or_default()
}

fn dir_name(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string())
}

/// 这份前缀是哪份 Proton 建的：Proton 的 `proton` 脚本会在前缀里留 `config_info`，
/// 第二行就是 `<proton根>/files/share/fonts/`，比 `version` 里那串版本号靠谱得多
/// （GE-Proton 的 version 写的是 `11.0-100`，在磁盘上对不上号）。
pub fn proton_of_prefix(prefix: &Path) -> Option<String> {
    let text = fs::read_to_string(prefix.join("config_info")).ok()?;
    let line = text.lines().nth(1)?;
    let root = line.trim().strip_suffix("files/share/fonts/")?;
    let root = root.trim_end_matches('/');
    is_proton_root(Path::new(root)).then(|| root.to_string())
}

/// `config_info` 丢了的兜底：拿 version 里的版本串去 compatibilitytools.d 里认人。
fn proton_by_version(ver: &str) -> Option<String> {
    if ver.is_empty() {
        return None;
    }
    let home = super::desktop::home();
    let roots = [
        home.join(".local/share/Steam/compatibilitytools.d"),
        home.join(".local/share/umu/compatibilitytools.d"),
    ];
    for root in roots {
        let Ok(rd) = fs::read_dir(&root) else { continue };
        let mut cands: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_proton_root(p))
            .collect();
        cands.sort();
        cands.reverse();
        for c in cands {
            let v = fs::read_to_string(c.join("version")).unwrap_or_default();
            if v.contains(ver) {
                return Some(c.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// 认一份前缀。`wine_hint` 是界面上已经配好的 wine，普通 Wine 前缀就用它。
pub fn inspect_prefix(dir: &Path, wine_hint: &str) -> Option<PrefixInfo> {
    if !is_prefix(dir) {
        return None;
    }
    let ver = prefix_version(dir);
    if let Some(root) = proton_of_prefix(dir).or_else(|| proton_by_version(&ver)) {
        let label = if ver.is_empty() { dir_name(&root) } else { ver };
        return Some(PrefixInfo {
            prefix: dir.to_path_buf(),
            runner: PrefixRunner::Proton { root },
            label,
        });
    }
    let wine = if wine_hint.is_empty() {
        find_wine().unwrap_or_default()
    } else {
        wine_hint.to_string()
    };
    Some(PrefixInfo {
        prefix: dir.to_path_buf(),
        runner: PrefixRunner::Wine { wine },
        label: String::new(),
    })
}

impl PrefixInfo {
    /// WINE / WINELOADER / WINESERVER 全指到这份前缀自己的 wine 上，
    /// 不然 winetricks 会拿 PATH 里的 wine 去动别的前缀（版本对不上会写坏注册表）。
    pub fn wine_env(&self) -> Option<Vec<(String, String)>> {
        match &self.runner {
            PrefixRunner::Proton { root } => {
                let bin = Path::new(root).join("files/bin");
                let wine = bin.join("wine");
                if !is_exec(&wine) {
                    return None;
                }
                let wine = wine.to_string_lossy().to_string();
                let mut env = vec![
                    ("WINE".to_string(), wine.clone()),
                    ("WINELOADER".to_string(), wine),
                ];
                let ws = bin.join("wineserver");
                if is_exec(&ws) {
                    env.push(("WINESERVER".to_string(), ws.to_string_lossy().to_string()));
                }
                Some(env)
            }
            PrefixRunner::Wine { wine } => {
                if !is_exec(Path::new(wine)) {
                    return None;
                }
                let mut env = vec![("WINE".to_string(), wine.clone())];
                if let Some(dir) = Path::new(wine).parent() {
                    let ws = dir.join("wineserver");
                    if is_exec(&ws) {
                        env.push(("WINESERVER".to_string(), ws.to_string_lossy().to_string()));
                    }
                }
                Some(env)
            }
        }
    }

    /// winetricks 本体：只用这份前缀所属 Proton **自带**的那份
    /// （`protonfixes/winetricks`，umu 的 winetricks 动词用的也是它）。
    /// 系统里装的那份版本未必配合适这份前缀，不拿它当后备。
    pub fn winetricks(&self) -> Option<String> {
        let PrefixRunner::Proton { root } = &self.runner else {
            return None;
        };
        for rel in ["protonfixes/winetricks", "protonfixes/files/bin/winetricks"] {
            let p = Path::new(root).join(rel);
            if is_exec(&p) {
                return Some(p.to_string_lossy().to_string());
            }
        }
        None
    }

    /// 在这份前缀上跑 winetricks 的完整命令
    pub fn command(&self) -> Result<String, String> {
        let env = self.wine_env().ok_or_else(|| match &self.runner {
            PrefixRunner::Proton { root } => format!("这份 Proton 里没有 files/bin/wine：{root}"),
            PrefixRunner::Wine { .. } => "没找到可用的 wine".to_string(),
        })?;
        let wt = self.winetricks().ok_or_else(|| match &self.runner {
            PrefixRunner::Proton { root } => {
                format!("这份 Proton 里没有自带 winetricks：{root}")
            }
            PrefixRunner::Wine { .. } => {
                "这个前缀不是 Proton 建的，没有配套的 winetricks".to_string()
            }
        })?;
        let mut parts = vec!["env".to_string()];
        for (k, v) in env {
            parts.push(format!("{k}={}", shell_quote(&v)));
        }
        parts.push(format!(
            "WINEPREFIX={}",
            shell_quote(&self.prefix.to_string_lossy())
        ));
        parts.push(shell_quote(&wt));
        Ok(parts.join(" "))
    }

    /// 工作目录：Proton 前缀跟着 Proton 根目录走（上游也是这么跑的），Wine 用 $HOME
    pub fn cwd(&self) -> PathBuf {
        match &self.runner {
            PrefixRunner::Proton { root } => PathBuf::from(root),
            PrefixRunner::Wine { .. } => {
                let h = super::desktop::home();
                if h.as_os_str().is_empty() {
                    PathBuf::from("/")
                } else {
                    h
                }
            }
        }
    }
}

// ---------------------------------------------------------------- 窗口

#[derive(Debug, Clone)]
pub struct WinInfo {
    pub id: String,
    pub title: String,
    pub wm_class: String,
}

/// 读 X11 / XWayland 的窗口列表。Wayland 原生窗口看不到，这是 EWMH 的边界。
pub fn list_x11_windows() -> Vec<WinInfo> {
    if which("xprop").is_none() {
        return Vec::new();
    }
    let root = Command::new("xprop")
        .args(["-root", "_NET_CLIENT_LIST"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let mut out = Vec::new();
    for id in root.split(|c: char| c == ',' || c.is_whitespace()) {
        let id = id.trim();
        if !id.starts_with("0x") {
            continue;
        }
        let cls = Command::new("xprop")
            .args(["-id", id, "WM_CLASS"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        let name = Command::new("xprop")
            .args(["-id", id, "_NET_WM_NAME"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        out.push(WinInfo {
            id: id.to_string(),
            title: quoted(&name).unwrap_or_default(),
            wm_class: class_pair(&cls).unwrap_or_default(),
        });
    }
    out
}

fn quoted(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let rest = &line[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// `WM_CLASS(STRING) = "res_name", "res_class"` → res_class
fn class_pair(line: &str) -> Option<String> {
    let eq = line.find('=')?;
    let body = &line[eq + 1..];
    let mut parts = body.split('"').skip(1).step_by(2);
    let _res_name = parts.next()?;
    parts.next().map(|s| s.to_string())
}

// ---------------------------------------------------------------- 依赖

#[derive(Debug, Clone)]
pub struct Dep {
    pub name: String,
    pub required: bool,
    pub found: bool,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Umu,
    Wine,
}

fn has_module(m: &str) -> bool {
    Command::new("pkg-config")
        .args(["--exists", m])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn pe_compiler() -> (bool, String) {
    if which("x86_64-w64-mingw32-gcc").is_some() {
        return (true, "mingw-w64".into());
    }
    let clang = which("clang").is_some();
    let lld = which("ld.lld").is_some() || which("lld").is_some();
    let dlltool = which("llvm-dlltool").is_some();
    if clang && lld && dlltool {
        (true, "clang + lld + llvm-dlltool".into())
    } else {
        let mut miss = Vec::new();
        if !clang {
            miss.push("clang");
        }
        if !lld {
            miss.push("lld");
        }
        if !dlltool {
            miss.push("llvm-dlltool");
        }
        (false, format!("缺 {}", miss.join(" / ")))
    }
}

fn static_libc() -> bool {
    [
        "/usr/lib64/libc.a",
        "/usr/lib/libc.a",
        "/usr/lib/x86_64-linux-gnu/libc.a",
        "/usr/lib/aarch64-linux-gnu/libc.a",
    ]
    .iter()
    .any(|p| Path::new(p).is_file())
}

pub fn dep_status(kind: Kind) -> Vec<Dep> {
    let mut out = Vec::new();
    match kind {
        Kind::Umu => {
            for (n, req, note) in [
                ("make", true, "构建入口，跑 Makefile"),
                ("python3", true, "把 umu 打成 zipapp"),
                ("cargo", false, "编 Rust 版 umu-shim（zipapp 路线用不到）"),
                ("rustc", false, "同上"),
                ("scdoc", false, "生成 man page"),
            ] {
                out.push(Dep {
                    name: n.into(),
                    required: req,
                    found: which(n).is_some(),
                    note: note.into(),
                });
            }
        }
        Kind::Wine => {
            for (n, note) in [
                ("gcc", "C 编译器"),
                ("g++", "C++ 编译器"),
                ("make", "构建入口"),
                ("flex", "词法分析器"),
                ("bison", "语法分析器"),
                ("pkg-config", "查找依赖"),
            ] {
                out.push(Dep {
                    name: n.into(),
                    required: true,
                    found: which(n).is_some(),
                    note: note.into(),
                });
            }
            let (ok, note) = pe_compiler();
            out.push(Dep {
                name: "PE 交叉编译器".into(),
                required: true,
                found: ok,
                note,
            });
            let libc = static_libc();
            out.push(Dep {
                name: "静态 libc (libc.a)".into(),
                required: true,
                found: libc,
                note: "preloader 需要 -static-pie".into(),
            });
            for (m, note) in [
                ("freetype2", "字体"),
                ("fontconfig", "字体匹配"),
                ("x11", "X11"),
                ("xext", "X11 扩展"),
                ("xrender", "渲染"),
                ("xrandr", "分辨率"),
                ("xi", "输入"),
                ("xcursor", "光标"),
                ("xfixes", "XFix"),
                ("xinerama", "多屏"),
                ("xcomposite", "合成"),
                ("gnutls", "TLS"),
                ("alsa", "声音"),
                ("libpulse", "PulseAudio"),
                ("vulkan", "Vulkan"),
            ] {
                out.push(Dep {
                    name: m.into(),
                    required: true,
                    found: has_module(m),
                    note: note.into(),
                });
            }
        }
    }
    out
}

fn pkg_for(name: &str, fedora: bool) -> String {
    let m = if fedora {
        [
            ("gcc", "gcc"),
            ("g++", "gcc-c++"),
            ("make", "make"),
            ("flex", "flex"),
            ("bison", "bison"),
            ("pkg-config", "pkgconf-pkg-config"),
            ("python3", "python3"),
            ("cargo", "cargo"),
            ("rustc", "rust"),
            ("scdoc", "scdoc"),
            ("PE 交叉编译器", "lld clang llvm"),
            ("静态 libc (libc.a)", "glibc-static"),
            ("freetype2", "freetype-devel"),
            ("fontconfig", "fontconfig-devel"),
            ("x11", "libX11-devel"),
            ("xext", "libXext-devel"),
            ("xrender", "libXrender-devel"),
            ("xrandr", "libXrandr-devel"),
            ("xi", "libXi-devel"),
            ("xcursor", "libXcursor-devel"),
            ("xfixes", "libXfixes-devel"),
            ("xinerama", "libXinerama-devel"),
            ("xcomposite", "libXcomposite-devel"),
            ("gnutls", "gnutls-devel"),
            ("alsa", "alsa-lib-devel"),
            ("libpulse", "pulseaudio-libs-devel"),
            ("vulkan", "vulkan-loader-devel"),
        ]
    } else {
        [
            ("gcc", "gcc"),
            ("g++", "g++"),
            ("make", "build-essential"),
            ("flex", "flex"),
            ("bison", "bison"),
            ("pkg-config", "pkg-config"),
            ("python3", "python3"),
            ("cargo", "cargo"),
            ("rustc", "rustc"),
            ("scdoc", "scdoc"),
            ("PE 交叉编译器", "clang lld llvm"),
            ("静态 libc (libc.a)", "libc6-dev"),
            ("freetype2", "libfreetype-dev"),
            ("fontconfig", "libfontconfig-dev"),
            ("x11", "libx11-dev"),
            ("xext", "libxext-dev"),
            ("xrender", "libxrender-dev"),
            ("xrandr", "libxrandr-dev"),
            ("xi", "libxi-dev"),
            ("xcursor", "libxcursor-dev"),
            ("xfixes", "libxfixes-dev"),
            ("xinerama", "libxinerama-dev"),
            ("xcomposite", "libxcomposite-dev"),
            ("gnutls", "libgnutls28-dev"),
            ("alsa", "libasound2-dev"),
            ("libpulse", "libpulse-dev"),
            ("vulkan", "libvulkan-dev"),
        ]
    };
    m.iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| name.to_string())
}

pub fn install_hint(names: &[String]) -> String {
    let fed = names.iter().map(|n| pkg_for(n, true)).collect::<Vec<_>>().join(" ");
    let deb = names.iter().map(|n| pkg_for(n, false)).collect::<Vec<_>>().join(" ");
    format!("Fedora:  sudo dnf install {fed}\nDebian:  sudo apt install {deb}")
}

// ---------------------------------------------------------------- 构建

#[derive(Debug, Clone)]
pub struct Step {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub label: String,
}

pub fn plan_umu(src: &Path) -> Vec<Step> {
    let jobs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    vec![
        Step {
            program: "./configure.sh".into(),
            args: vec!["--user-install".into()],
            cwd: src.to_path_buf(),
            label: "configure".into(),
        },
        Step {
            program: "make".into(),
            args: vec![format!("-j{jobs}")],
            cwd: src.to_path_buf(),
            label: "make".into(),
        },
        Step {
            program: "make".into(),
            args: vec!["install".into()],
            cwd: src.to_path_buf(),
            label: "install".into(),
        },
    ]
}

pub fn plan_wine(src: &Path, i386: bool) -> Vec<Step> {
    let jobs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    let build = src.join("build");
    let archs = if i386 { "i386,x86_64" } else { "x86_64" };
    vec![
        Step {
            program: "../configure".into(),
            args: vec![format!("--enable-archs={archs}")],
            cwd: build,
            label: "configure".into(),
        },
        Step {
            program: "make".into(),
            args: vec![format!("-j{jobs}")],
            cwd: src.join("build"),
            label: "make".into(),
        },
    ]
}

pub fn plan_result(kind: Kind, src: &Path) -> PathBuf {
    match kind {
        Kind::Umu => super::desktop::home().join(".local/bin/umu-run"),
        Kind::Wine => src.join("build/wine"),
    }
}

/// 去掉 tput / make 吐出来的 ANSI 颜色码，否则日志里全是 `[32;1m::[0m`
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_wraps_and_escapes() {
        assert_eq!(shell_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn prefix_cmds_umu_sets_winver_after_create() {
        let init = PrefixInit {
            umu_mode: true,
            umu: "/home/u/.local/bin/umu-run".into(),
            gameid: "umu-0".into(),
            proton: "/p/GE-Proton11-12".into(),
            wine: String::new(),
        };
        let cmds = init.cmds(Path::new("/pfx"));
        assert_eq!(cmds.len(), 2);
        assert!(cmds[0].contains("createprefix"), "{}", cmds[0]);
        assert!(cmds[1].contains(&format!("winetricks {PREFIX_WINVER}")), "{}", cmds[1]);
        assert!(cmds[1].contains("PROTONPATH='/p/GE-Proton11-12'"), "{}", cmds[1]);
        assert!(cmds[1].contains("WINEPREFIX='/pfx'"), "{}", cmds[1]);
        assert!(cmds[1].starts_with("env -u WINE"), "{}", cmds[1]);
    }

    #[test]
    fn prefix_cmds_wine_uses_winecfg() {
        let init = PrefixInit {
            umu_mode: false,
            umu: String::new(),
            gameid: String::new(),
            proton: String::new(),
            wine: "/usr/bin/wine".into(),
        };
        let cmds = init.cmds(Path::new("/pfx"));
        assert_eq!(cmds.len(), 2);
        assert!(cmds[0].ends_with("wineboot -i 2>&1"), "{}", cmds[0]);
        assert!(cmds[1].ends_with("winecfg -v win10 2>&1"), "{}", cmds[1]);
        assert!(!cmds[1].contains("PROTONPATH"), "{}", cmds[1]);
    }

    #[test]
    fn prefix_recognises_wine_and_proton() {
        let root = std::env::temp_dir().join(format!("carboy-pfx-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);

        // 普通 wine 前缀：只有 drive_c + system.reg
        let wine_pfx = root.join("wine");
        fs::create_dir_all(wine_pfx.join("drive_c")).unwrap();
        fs::write(wine_pfx.join("system.reg"), "").unwrap();
        assert!(is_prefix(&wine_pfx));
        assert!(proton_of_prefix(&wine_pfx).is_none());
        let info = inspect_prefix(&wine_pfx, "/opt/fake/wine").unwrap();
        assert_eq!(
            info.runner,
            PrefixRunner::Wine { wine: "/opt/fake/wine".into() }
        );

        // Proton 前缀：config_info 第二行指回 Proton 根目录
        let proton = root.join("proton");
        fs::create_dir_all(&proton).unwrap();
        fs::write(proton.join("toolmanifest.vdf"), "").unwrap();
        let pfx = root.join("proton-pfx");
        fs::create_dir_all(pfx.join("drive_c")).unwrap();
        fs::write(pfx.join("system.reg"), "").unwrap();
        fs::write(pfx.join("version"), "my-proton-1.0\n").unwrap();
        fs::write(
            pfx.join("config_info"),
            format!(
                "my-proton-1.0\n{}/files/share/fonts/\n",
                proton.to_string_lossy()
            ),
        )
        .unwrap();
        let info = inspect_prefix(&pfx, "").unwrap();
        assert_eq!(
            info.runner,
            PrefixRunner::Proton {
                root: proton.to_string_lossy().to_string()
            }
        );
        assert_eq!(info.label, "my-proton-1.0");
        assert_eq!(info.cwd(), proton);

        // 空目录不算前缀
        let empty = root.join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(inspect_prefix(&empty, "").is_none());

        let _ = fs::remove_dir_all(&root);
    }
}

