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
