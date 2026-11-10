# Freedesktop 集成 — 技术深挖

> 本文回答一个问题：**为什么一个 `.desktop` 文件写好了，图标在 dash-to-dock 上还是错的？**
> 这是 Carboy 的核心技术资产，也是产品存在的理由。

---

## 1. 三个身份，必须对齐

一个窗口在桌面上"被认出是某个应用"，依赖三个身份标识符的匹配：

```
     ┌───────────────────────────┐
     │  窗口 (X11 / XWayland)     │
     │  WM_CLASS = (res_name, res_class)
     │  _NET_WM_PID = 12345       │
     └────────────┬──────────────┘
                  │  Shell 拿 res_class 去找应用
                  ▼
     ┌───────────────────────────┐
     │  .desktop 文件             │
     │  id        = carboy-hades.desktop
     │  StartupWMClass = "hades.exe"   ← 唯一的桥
     │  Icon = carboy-hades            ← 指向图标主题
     └────────────┬──────────────┘
                  │
                  ▼
     ┌───────────────────────────┐
     │  图标主题                  │
     │  hicolor/256x256/apps/carboy-hades.png
     └───────────────────────────┘
```

三条链路，**任何一条断了都会表现为"图标不对"**，而且症状看起来一样，所以必须分开诊断：

| 断点 | 症状 |
|---|---|
| ① 窗口 → 桌面项（`StartupWMClass` 缺失/写错） | dock 显示酒瓶图标、或独立一个无名条目、或多个游戏挤成一组 |
| ② 桌面项 → 图标（`Icon=` 名字解析不了） | 显示通用"可执行文件"图标 |
| ③ 图标主题（文件没装/没刷新缓存） | 图标名字能解析但渲染成空白或低清晰度 |

**关键结论：`WM_CLASS` 的取值由 Runner 决定，不是由游戏决定。**

---

## 1.1 WM_CLASS 到底等于什么（已核源码，2026-11-10）

这一节是全文档的地基。**两类 Runner 的行为完全不同**，之前的"class 就是 exe 名"或"class 不可预测"的说法都不准确。

### A. 上游 wine / Wine-GE —— 可预测

`dlls/winex11.drv/x11drv_main.c`（约 461–473 行）：

```c
appname = NtCurrentTeb()->Peb->ProcessParameters->ImagePathName.Buffer;
if ((p = wcsrchr( appname, '/' ))) appname = p + 1;    // 取 basename
if ((p = wcsrchr( appname, '\\' ))) appname = p + 1;
for (i = 0; appname[i]; i++) buffer[i] = RtlDowncaseUnicodeChar( appname[i] );  // 全小写
process_name = ... buffer ...
```

`dlls/winex11.drv/window.c`，`set_initial_wm_hints()`：

```c
class_hints->res_name  = process_name;
class_hints->res_class = process_name;
```

→ **class = exe 文件名，全小写，保留 `.exe`**。`Hades.exe` → `hades.exe`。

### B. Proton（含 dwproton / GE-Proton / proton-cachyos）—— 打了补丁，与 exe 名无关

同一个函数，Proton 分支（实测 `proton-11.0-2`）：

```c
static char steam_proton[] = "steam_proton";
const char *app_id = getenv("SteamAppId");
char proton_app_class[128];

if (app_id && *app_id) {
    snprintf(proton_app_class, sizeof(proton_app_class), "steam_app_%s", app_id);
    class_hints->res_name  = proton_app_class;
    class_hints->res_class = proton_app_class;
} else {
    class_hints->res_name  = steam_proton;
    class_hints->res_class = steam_proton;
}
```

→ **只有两种可能**：`steam_app_<SteamAppId>`，或者 `steam_proton`。**跟 exe 名、跟游戏毫无关系。**

### C. umu 怎么决定 `SteamAppId`

`umu/umu_run.py`（1054 行版本，约 275–282 行）：

```python
env["STEAM_COMPAT_APP_ID"] = "0"                                   # 默认
if "-" in env["UMU_ID"]:
    env["STEAM_COMPAT_APP_ID"] = env["UMU_ID"][env["UMU_ID"].find("-") + 1:]
env["SteamAppId"]  = env["STEAM_COMPAT_APP_ID"]
env["SteamGameId"] = env["SteamAppId"]
```

且 `UMU_ID = GAMEID`（274 行）；GAMEID 未设时 umu 自己补 `umu-default`（137–139 行）。

→ **`SteamAppId` = GAMEID 里第一个 `-` 之后的部分**；GAMEID 无 `-` 时就是 GAMEID 本身。

### D. 完整对照表

| 场景 | WM_CLASS |
|---|---|
| 系统 wine / Wine-GE，跑 `Hades.exe` | `hades.exe` |
| Proton + umu，`GAMEID=umu-2358720` | `steam_app_2358720` |
| Proton + umu，`GAMEID=carboy-hades` | `steam_app_hades` |
| **Proton + umu，`GAMEID=0`（所谓"通用"）** | **`steam_app_0`** ← 所有游戏撞成一个 |
| **Proton 直跑，未设 `SteamAppId`** | **`steam_proton`** ← 同上 |

### E. 这条事实带来的三个设计决策

1. **`GAMEID` 必须唯一，绝不允许 `0`。**
   用 umu 的默认 `GAMEID=0` 跑十个游戏，十个窗口的 class 全是 `steam_app_0`，GNOME 只能把它们合并成一个应用、显示一个图标 —— **这就是"多个游戏挤成一坨/图标不对"最常见的根因**，而且它同时还意味着十个游戏共用一个前缀 `~/Games/umu/0`。
   Carboy 必须给每个应用分配唯一 GAMEID：优先 umu-database 里的真值（能拿到 protonfixes），否则用 `carboy-<slug>`（放弃 fix，但换来唯一 class）。

2. **`StartupWMClass` 可以在启动前就写对。**
   Proton 模式下 class = `steam_app_<派生值>`，wine 模式下 class = exe 名小写 —— 两者都是纯函数。所以探测从"唯一手段"降级为"验证 + 兜底"。

3. **数据模型加一个派生字段**，不要只存实测值：

```rust
/// 从已知信息推导 WM_CLASS，不碰 IO，可单测
pub fn derive_wm_class(runner: &Runner, mode: LaunchMode, gameid: &str, exe: &Path) -> String {
    match (runner.is_proton, mode) {
        (true, LaunchMode::Umu) => {
            let appid = gameid.split_once('-').map(|(_, b)| b).unwrap_or(gameid);
            format!("steam_app_{appid}")
        }
        (true, LaunchMode::ProtonRaw) => "steam_proton".into(),
        (false, _) => exe.file_name().unwrap().to_string_lossy().to_lowercase(),
    }
}
```

UI 上显示「预计 class」，实测后覆盖，两者不一致就提示「Runner 行为与预期不同」—— 这本身就是一个很强的诊断信号。

---

## 1.2 有了 class 之后：gnome-shell 怎么找到应用

`src/shell-window-tracker.c`（main，823 行）的 `get_app_for_window()`，**命中即返回**，顺序是：

| 序 | 依据 | 我们能控制吗 |
|---|---|---|
| 1 | 已登记过的窗口缓存 | — |
| 2 | **`WM_CLASS`** | ✅ 全部靠这一步 |
| 3 | Flatpak sandbox app id | ✗（我们不从沙箱跑） |
| 4 | `_GTK_APPLICATION_ID`（GApplication） | ✗（Wine 窗口没有） |
| 5 | `_NET_WM_PID` | ⚠️ 部分（wineserver 场景不稳） |
| 6 | 启动通知 sequence id | ✗（Wine 不发） |
| 7 | 同 window group 的其它窗口 | ✗ |
| 8 | **兜底：凭空造一个假 app** | ✗ → 就是"无名无图标条目" |

第 2 步内部还有三级（源码注释原话："Check if the app's WM_CLASS specifies an app; this is canonical if it does"）：

1. `lookup_startup_wmclass(wm_class)` —— 拿 **`res_class`** 去比所有桌面项的 `StartupWMClass`
2. `lookup_desktop_wmclass(wm_instance)` —— 拿 **`res_name`** 去找 `<res_name>.desktop`
3. `lookup_desktop_wmclass(wm_class)` —— 拿 **`res_class`** 去找 `<res_class>.desktop`

### 对我们意味着什么

- **第 1 级就是我们写 `StartupWMClass` 的依据**，也是唯一"正规"的路。
- **第 3 级就是 §2.4 说的 Plan B**：把桌面文件命名成 `<class>.desktop` 也能命中。所以 `steam_app_hades.desktop` 这种丑名字在理论上是可用的兜底 —— 但它不可维护，只配当诊断手段。
- **第 8 级最关键**：匹配失败**不会报错**，Shell 会静默造一个假应用（无图标、无正式名字）。所以"图标不对"永远不会有系统提示，**只能靠 `doctor` 反查**。这也是这个产品存在的意义。
- 因为"命中即停"，只要有一个桌面项声明了 `steam_proton`，**所有** Proton 直跑的游戏都会命中它并合并成一组 —— 这就是很多人"游戏全挤在一个图标里"的机制。

### 两个补充事实（源码确认，容易踩）

**① 匹配是大小写敏感的。** 索引表用的是 `g_hash_table_new_full(g_str_hash, g_str_equal, ...)`，`ShellAppSystem.c:301` —— 精确字节比较，没有 `strcasecmp`。
所以 `StartupWMClass=Hades.exe` 和 `res_class=hades.exe` **匹配不上**。Carboy 生成时一律按实测原样写入，不做大小写"美化"。

**② 多个桌面项声明同一 class 时，仲裁规则是确定的**（`scan_startup_wm_class_to_id`，`ShellAppSystem.c:134-147`）：

1. 优先「id 与 class 完全对应」的那个 —— 比如 `foo.desktop` + `StartupWMClass=foo`
2. 其次优先 `NoDisplay` 不为 true 的（能出现在应用列表里的）
3. 都打平则先扫描到的赢（取决于 `applications/` 目录的枚举顺序）

这条规则可以直接写进 `doctor`：发现冲突时，告诉用户"Shell 会选 `foo.desktop`，不是你这个"，比只提示"有冲突"有用得多。

---

## 2. WM_CLASS 探测算法

既然 §1.1 已经能算出 class，为什么还要探测？因为**预测不等于真相**，下面这些情况预测会错：

- **wine 模式下的启动器**：`launcher.exe` 起主进程，实际窗口属于另一个 exe
- **自定义 Runner**：不确定它的 wine 是否打了 Proton 那个补丁，猜错就是全错
- **GAMEID 解析边界**：`carboy-a-b` 这种带多个 `-` 的，派生值可能与预期不同
- **别人的桌面项**：用户以前用 Lutris/Steam 跑过同一个游戏，已存在一个声明了同一 class 的桌面项

所以探测的定位是：**预测 → 启动 → 验证 → 不一致就报警**。预测让 90% 的场景免于实测，探测兜住剩下 10%。

### 2.1 前置：为什么不直接用 X11 库

Carboy 是 GTK4 应用，在 Wayland 会话下运行时自己是 Wayland 客户端，**无法直接调用 Xlib**（能连上 XWayland，但这会引入 `x11rb` 依赖且需要处理两套事件循环）。

**方案：调用 `xprop` 子进程。** 零链接依赖、XWayland 上工作正常、任何桌面都能用。

```sh
xprop -root _NET_CLIENT_LIST          # 所有顶层窗口 id
xprop -id 0x03400007 WM_CLASS         # → "hades.exe", "hades.exe"
xprop -id 0x03400007 _NET_WM_PID      # → 12345
xprop -id 0x03400007 _NET_WM_ICON     # → 图标 ARGB 数组
```

后端可替换（`xprop` → `xwininfo` → `xdotool` → 手填），用 trait 隔离：

```rust
pub trait WindowProbe {
    fn list_windows(&self) -> Result<Vec<WindowId>>;
    fn wm_class(&self, w: WindowId) -> Result<Option<(String, String)>>; // (res_name, res_class)
    fn pid(&self, w: WindowId) -> Result<Option<u32>>;
    fn icon(&self, w: WindowId) -> Result<Option<Vec<IconImage>>>;
}
```

### 2.2 流程

```
snapshot = probe.list_windows()                     ← 启动前快照
spawn(launch_plan)                                  ← 同时记下 child_pid 与启动时刻 t0

loop every 200ms, up to 60s:
    for w in probe.list_windows() - snapshot:
        score = 0
        class = probe.wm_class(w)
        pid   = probe.pid(w)

        if pid 在 child_pid 的子孙树内                    score += 30
        if basename(exe).eq_ignore_case(class.1)          score += 50
        if basename(exe) 是 class.0 的前缀                 score += 20
        if 窗口创建时间 ∈ [t0, now]                        score += 20
        if 窗口尺寸 < 200x200                              score -= 10   // 启动画面/托盘
        if class.1 属于 "已知桌面应用集合"                  score -= 100  // 误抓浏览器等

        candidates.push((score, w, class))

best = candidates.max_by(score)
if best.score >= 60  → 自动采用（静默）
if best.score in 30..60 → 弹窗让用户点选（或"就这个"确认）
if 60s 无候选 → 标记 wm_class_source = "manual"，走手填
```

### 2.3 三个必须处理的实际问题

**① wineserver 会 daemonize**
Proton/Wine 启动后 `wineserver` fork 到后台，`/proc` 里的 ppid 链会断。所以 PID 匹配**不能作为唯一依据**，必须双策略：PID 树优先，断裂时靠「时间窗内新窗口 + class 与 exe 名相似」兜底。

```rust
fn process_tree_contains(root: u32, pid: u32) -> bool {
    // 读 /proc/*/stat 的 ppid 建树，深度上限 32 层
    // 注意容错：进程可能在读取瞬间消失
}
```

**② 启动器窗口 vs 主窗口**
很多游戏先弹 launcher（class = `launcher.exe`），确认后才出主窗口。所以探测窗口**不能一拿到就关**，要持续到用户确认；并提供「重新探测」按钮，因为它可能在游戏进入全屏后变化。

**③ `res_class` 太通用**
有些 Unity/Unreal 游戏的 class 就是 `UnityPlayer` / `UnrealWindow`，多个游戏共用 → 必须警告，并建议改用 `res_name`（第一字段）或让用户用「以桌面文件名兜底」策略（见 §2.4）。

### 2.4 Plan B：用桌面文件名兜底

GNOME Shell 在 `StartupWMClass` 缺失时，还会尝试用 `<wm_class 小写>.desktop` 去查应用，KDE 也支持类似逻辑。所以有两种命名策略：

| 策略 | 桌面项 id | 优点 | 缺点 |
|---|---|---|---|
| A. slug 命名 + `StartupWMClass` | `carboy-hades.desktop` | id 好看、统一 | 依赖 `StartupWMClass` 写对 |
| B. class 命名 | `hades.exe.desktop` | 双保险，class 变了也能匹配 | id 丑、可能撞名、`.desktop` 里带点号易困惑 |

**建议：默认 A，但在诊断面板里提供「改用 B 再试一次」的一键切换** —— 这是排查关联问题时最有效的实测手段。

---

## 3. 图标提取：四级优先链

不同游戏把图标藏在不同地方。**顺序已按源码实测调整过**（原版把窗口图标排第一，是错的，原因见 ①）。

### ① PE 资源提取（首选）

Windows exe 的图标在资源段里，结构是：

```
RT_GROUP_ICON (type 14)  →  GRPICONDIR { count, GRPICONDIRENTRY[count] }
                                 每个 entry 里有 nID
RT_ICON (type 3)         →  按 nID 取裸图（DIB 或内嵌 PNG）
```

用 `pelite` crate 直接读，无需外部工具：

```rust
let pe = pelite::PeFile::from_bytes(&std::fs::read(exe)?)?;
let resources = pe.resources()?;
// 枚举 type=14 的 RT_GROUP_ICON，组装 ICO，再交给 image crate 解码
```

（快捷替代：`wrestool -x -t 14 app.exe > app.ico && icotool -x app.ico`。但 `icoutils` 不必需，不该成为硬依赖。）

**坑**：许多现代 exe 的 group icon 只含 16/32/48 小图，256 那档在 `RT_ICON` 里是 PNG 压缩的（DIB header 前 8 字节是 PNG 签名）。两种格式都要判。

**为什么排第一**：这是**唯一能稳定拿到 256×256 高清图标的来源**。dock 在 4K/放大状态下需要 128/256，其余来源给不了。

### ② 窗口 `_NET_WM_ICON`（补充，通常是低分辨率）

EWMH 规范里这个属性**可以**放多档尺寸，但 Wine 的实际行为是另一回事。看 `dlls/winex11.drv/window.c` 的 `X11DRV_SetWindowIcon`：

```c
bits = get_bitmap_argb( hDC, ii->hbmColor, ii->hbmMask, &size );   /* 大图标 */
if (bits && icon_small) {
    /* 只有尺寸不同时，才把「小图标」追加到同一数组后面 */
    bits = realloc( bits, (size + size_small) * sizeof(unsigned long) );
    ...
}
data->icon_bits = bits;
data->icon_size = size;
```

结论：

- **最多两档**：`WM_SETICON` 的大图标 + 小图标（且两者尺寸相同时只有一档）
- 尺寸由**应用自己**决定，常见是 32×32 / 16×16，很少给 256
- **应用没设窗口图标时，Wine 会直接删掉这个属性**（`XDeleteProperty`），`xprop` 什么也读不到

```sh
xprop -id 0x03400007 _NET_WM_ICON
# 常见输出：32, 32, <1024 个数字>  —— 就这一档
```

所以它是"游戏运行起来后顺手捡的一张图"，**不能当高清图标源**。价值在于：PE 里没图标、但应用运行时自绘了图标的那种怪游戏。

格式：`[w, h, ARGB_0 ... ARGB_{w*h-1}]` 重复 N 组。解析：

```rust
// 每个像素是 CARDINAL(32bit)，AARRGGBB
fn parse_net_wm_icon(data: &[u32]) -> Vec<IconImage> {
    let mut out = vec![]; let mut i = 0;
    while i + 2 <= data.len() {
        let (w, h) = (data[i] as usize, data[i+1] as usize); i += 2;
        let n = w * h;
        if n == 0 || i + n > data.len() { break; }
        let pixels: Vec<u8> = data[i..i+n].iter()
            .flat_map(|p| { let a = p>>24; let r = p>>16; let g = p>>8; let b = p;
                            [r as u8, g as u8, b as u8, a as u8] })
            .collect();
        out.push(IconImage { w, h, rgba: pixels });
        i += n;
    }
    out
}
```

**坑**：`xprop` 输出的是十进制整数文本，一档 256×256 就是 65538 个数字。必须流式解析，别一次性 `split` 成 `Vec<String>` 撑爆内存。

### ③ 游戏目录扫描
`*.ico` / `icon.png` / `logo.png` / `steam_icon_*.jpg`（Steam 导出的缩略图）。启发式按文件名+尺寸过滤。

### ④ 用户手选
拖入 PNG/SVG。永远保留这条后路，UI 上要显眼。

**最终产物**：一张或多尺寸 PNG，存 `~/.local/share/carboy/icons/<slug>/`，然后按 §4 安装。

**尺寸策略**：从 PE 里能拿到几档就导几档，缺的档位用最大那张**降采样**补齐（比让 dock 自己去拉伸质量好）。低于 128 的源不做放大 —— 放大只会更糊，宁可让 dock 用 128 显示时略显粗糙。

---

## 4. 图标安装：两件事都要做

### 4.1 装进主题目录（多尺寸）

```
~/.local/share/icons/hicolor/16x16/apps/carboy-hades.png
                                24x24/apps/...
                                32x32/apps/...
                                48x48/apps/...
                                64x64/apps/...
                                128x128/apps/...
                                256x256/apps/...
```

为什么不只用 `Icon=/absolute/path.png`？
- **能用**，GNOME 认绝对路径，但 dock 放大/缩放时只有一个尺寸，会糊
- KDE 的 `KIconLoader` 对主题名支持更好
- 主题名让 `Icon=` 在移动前缀/换机器后依然有效

所以：**装主题（主）+ 绝对路径（兜底）**，两者都写：`Icon=carboy-hades`，装失败时降级成绝对路径。

### 4.2 刷新缓存（否则等于没装）

```sh
gtk-update-icon-cache -f -t ~/.local/share/icons/hicolor
update-desktop-database ~/.local/share/applications
```

**注意**：`gtk-update-icon-cache` 在某些情况下会因 mtime 判断跳过更新，`-f` 必须加。这是最常见的"我明明装了图标却不显示"原因。

---

## 5. `.desktop` 序列化与 Exec 转义

### 5.1 生成样例

```ini
[Desktop Entry]
Type=Application
Version=1.5
Name=Hades
Name[zh_CN]=哈迪斯
Comment=Run with Carboy (Proton-GE 9-20)
Exec=env GAMEID=0 PROTONPATH=/home/u/.local/share/carboy/runners/proton-ge-9-20 WINEPREFIX=/home/u/Games/hades umu-run "/home/u/Games/hades/drive_c/Games/Hades/Hades.exe"
Path=/home/u/Games/hades/drive_c/Games/Hades
Icon=carboy-hades
Terminal=false
Categories=Game;
Keywords=carboy;wine;proton;
StartupNotify=false
StartupWMClass=hades.exe
X-Carboy-App-Id=01J8Z...
X-Carboy-Generated=2026-11-10T13:40:00+08:00
```

字段决策说明：

| 字段 | 值 | 为什么 |
|---|---|---|
| `StartupWMClass` | 实测 `res_class` | 关联的唯一钥匙 |
| `StartupNotify` | **`false`** | Wine/umu 不会发 `_NET_STARTUP_ID` 完成通知，设 `true` 会导致 dock 上的启动光标一直转 |
| `Terminal` | `false` | 需要终端时改用 wrapper 脚本 |
| `Categories` | `Game;` | 进"游戏"分类；也可加 `Emulator;` |
| `Path` | 游戏目录 | 很多游戏靠 cwd 找资源文件，不设必崩 |
| `X-Carboy-*` | 自有 id | 规范允许 `X-` 前缀扩展，方便我们反查和批量重建 |

### 5.2 Exec 转义（这是静默失败的元凶）

Desktop Entry 规范规定：`Exec` 里的参数含保留字符时用双引号包裹，**引号内**的 `"` `` ` `` `$` `\` 需反斜杠转义；字面 `%` 要写成 `%%`。

```rust
/// 严格按 Desktop Entry Spec 转义一个 Exec 参数
pub fn escape_exec_arg(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s.chars().any(|c| matches!(c,
            ' ' | '\t' | '\n' | '"' | '\'' | '\\' | '>' | '<' | '~' | '|'
            | '&' | ';' | '$' | '*' | '?' | '#' | '(' | ')' | '`'));
    let escaped = s.chars().flat_map(|c| match c {
        '"' | '`' | '$' | '\\' => vec!['\\', c],
        '%' => vec!['%', '%'],           // 字面百分号
        c => vec![c],
    }).collect::<String>();
    if needs_quote { format!("\"{}\"", escaped) } else { escaped }
}
```

必测用例（写成 `#[test]`，一个都不能漏）：

```
/home/u/Games/Hades/Hades.exe            → 原样
/home/u/My Games/Hades/Hades.exe         → "/home/u/My Games/Hades/Hades.exe"
/home/u/100% Orange Juice/g.exe          → "/home/u/100%% Orange Juice/g.exe"
/home/u/a$b/g.exe                        → "/home/u/a\\$b/g.exe"
/home/u/quote"x/g.exe                    → "/home/u/quote\\\"x/g.exe"
```

**另一个坑**：`env A=1 B="x y" prog` 里的 `env` 是普通程序，它的参数也要走同一个转义函数。别手动拼字符串，全部走 `Vec<OsString>` → 转义 → join。

### 5.3 桌面文件夹快捷方式

`~/Desktop/xxx.desktop` 额外需要：
```sh
chmod +x ~/Desktop/xxx.desktop
gio set ~/Desktop/xxx.desktop metadata::trusted true   # GNOME 需要
```
XFCE / Cinnamon 只认可执行位。

---

## 6. 各桌面差异矩阵

| 桌面 | 窗口↔应用关联依据 | 图标查找 | 生成后需刷新 | Shell 重启方式 |
|---|---|---|---|---|
| **GNOME / dash-to-dock** | `WM_CLASS`(`res_class`) ↔ `StartupWMClass`（**精确匹配，大小写敏感**），缺失时退化为 `<class>.desktop` | `GIcon` via hicolor | `gtk-update-icon-cache` + `update-desktop-database` | X11: `killall -HUP gnome-shell`；Wayland: **只能注销重登** |
| **KDE Plasma** | KWin 的 `resourceClass` ↔ `StartupWMClass`；也认 desktop id | `KIconLoader`（认 hicolor） | `kbuildsycoca6`（Plasma 6）/ `kbuildsycoca5`，图标另需 cache | 无需重启，`kbuildsycoca` 后即时生效 |
| **XFCE** | libwnck + `StartupWMClass` | hicolor | 同上 + `xfce4-panel -r` | `xfce4-panel -r` |
| **Cinnamon** | 同 GNOME（基于 Muffin/libwnck） | hicolor | 同上 | 注销重登 |
| **Hyprland / sway** | 无任务栏；`class` 用于 `windowrule` / `windowrulev2` | launcher（wofi/fuzzix/fuzzel）扫 desktop 文件 | 无缓存，即时 | 不需要 |

**结论**：刷新动作必须按桌面探测（读 `XDG_CURRENT_DESKTOP`），不能一律 `gnome-shell`。Hyprland 用户看到"正在刷新 GNOME Shell"会觉得这软件不懂行。

---

## 7. 故障对照表（直接实现成 `doctor` 规则）

| 症状 | 最可能原因 | 检查方法 | 修法 |
|---|---|---|---|
| dock 显示 Wine 酒瓶 | `StartupWMClass` 缺失，匹配到 `wine.desktop` | 桌面项里有没有该字段 | 补上实测 class |
| dock 出现无名/无图标条目 | class 没匹配任何桌面项 | 该 class 在 `applications/` 里搜不到 | 写桌面项 or 补 `StartupWMClass` |
| 多个游戏合并成一个图标 | 两个桌面项 `StartupWMClass` 相同，或都缺失被归到 wine | 扫全部桌面项找重复 class | 删除冲突项 |
| 图标显示"齿轮/空白文档" | `Icon=` 名解析失败 | `gtk_icon_theme_has_icon()` 或 `Icon=` 绝对路径是否存在 | 装主题 + 刷新 |
| 图标装了但 blank | 缓存未刷新 / 尺寸目录写错（`256x256` vs `256`） | 看 `hicolor/<N>x<N>/apps/` | `-f` 强制刷新 |
| 图标糊 | 只有 48px | 目录里最大尺寸 | 补 128/256 |
| 双击桌面图标没反应 | 无执行位 / 未 trusted | `stat -c %a` | `chmod +x` + `gio set` |
| 应用列表里搜不到 | 桌面项不在 `XDG_DATA_DIRS` 路径 / `NoDisplay=true` / 语法错 | `desktop-file-validate` | 修语法 |
| 启动后光标一直转 | `StartupNotify=true` 但 Wine 不发完成通知 | 看字段 | 改 `false` |

`doctor` 对每个应用跑这套规则，输出结构化结果（✓/✗/!），每条带「一键修复」。这就是产品最直观的价值。

---

## 8. 实现 Checklist（M1 验收）

- [ ] `xprop` 后端：列窗口、读 `WM_CLASS`/`_NET_WM_PID`/`_NET_WM_ICON`
- [ ] 备选后端 `xwininfo` / `xdotool`，以及"全都不行"时的手填
- [ ] 探测循环：200ms 轮询、60s 超时、置信度评分、用户确认 UI
- [ ] `_NET_WM_ICON` 流式解析 → PNG（多尺寸）
- [ ] PE `RT_GROUP_ICON` + `RT_ICON` 解析（含 PNG 压缩条目）
- [ ] 图标导出 hicolor 7 档 + `gtk-update-icon-cache -f -t`
- [ ] `.desktop` 序列化 + `escape_exec_arg` 单测全绿
- [ ] `update-desktop-database` + 按桌面刷新的分派
- [ ] `doctor` 9 条规则 + 一键修复
- [ ] 冲突桌面项扫描与清理

**M1 的验收标准只有一条：在 GNOME + dash-to-dock 上，用 Carboy 添加一个非 Steam 游戏，dock 里显示的图标和名字，与手工安装的 Steam 游戏没有任何可见差别。**
