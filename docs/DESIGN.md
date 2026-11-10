# Carboy — 设计文档 v0.1

> 项目名 **Carboy**（大玻璃酒桶 —— 一个桶装一堆瓶子，对应"一个工具管一堆 prefix"）。
> 一个 GTK4/libadwaita 的 Wine/Proton 前缀与应用管理器。
> 技术栈：Rust + gtk4-rs + libadwaita，全桌面通用（GNOME / KDE / XFCE / Cinnamon / Hyprland）。

**文档索引**

| 文档 | 内容 |
|---|---|
| 本文 | 定位、功能规格、架构、数据模型、启动方案、里程碑 |
| `desktop-integration.md` | Freedesktop 集成技术深挖：WM_CLASS 探测、图标提取、Exec 转义、故障对照表 |
| `REFERENCES.md` | 参考源码清单：读哪个仓库的哪几个文件、为什么读、许可证约束 |

---

## 0. 设计理念：轻

**参考系是 WineGUI，不是 Lutris。**

WineGUI 值得学的地方不是它的功能，而是它的**形态**：单机、单窗口、打开就能用、不联网、不常驻、不试图成为平台。Lutris 的问题不在于功能多，而在于为了那些功能引入了**一整套生态**：Python 依赖树、社区安装脚本的运行环境、下载器、模拟器后端、商店 API。这些东西 90% 的用户一辈子用不到一次，却要一直养着。

所以 Carboy 的第一原则是：**克制**。能不做就不做，做也只做一层薄薄的自动化。

### 0.1 五条铁律

| # | 铁律 | 具体含义 |
|---|---|---|
| 1 | **零常驻** | 不注册 systemd unit、不写 autostart、不留守护进程、不开托盘轮询。用完即退，进程表里看不到它 |
| 2 | **零运行时解释器** | 交付物是一个 Rust 二进制。不需要 Python、不需要 Node、不需要 WebView、不需要 `icoutils`/`winetricks` 才能启动 |
| 3 | **三页封顶** | 前缀 / 应用 / 诊断。设置塞进 libadwaita 的 PreferencesWindow。不做标签页套标签页 |
| 4 | **默认离线** | 唯一的网络行为是用户主动点「下载 Runner」。无遥测、无启动时检查更新（默认关闭）、无封面抓取 |
| 5 | **每个功能都要能回答「不做会不会死」** | 回答不了就砍进"拒绝清单"，而不是塞进 P2 |

### 0.2 拒绝清单（明确不做）

这些不是"以后再做"，是**设计上的拒绝**，写下来是为了防止功能蔓延：

- ❌ 封面墙 / 元数据抓取 / SteamGridDB
- ❌ 游戏库、下载器、商店账号（Epic/GOG/Amazon）
- ❌ 社区安装脚本市场（那是 Lutris 的重心，也是它的重量来源）
- ❌ 云同步、账号系统、成就、社交
- ❌ 内嵌 WebView / 内嵌终端模拟器
- ❌ 内置 winetricks 图形界面（最多提供一个"在终端里跑 winetricks"的按钮）
- ❌ 自动更新守护、崩溃上报

### 0.3 复杂度预算（可验收的硬指标）

这些数字进 CI，超标就是设计跑偏了：

| 指标 | 预算 | 说明 |
|---|---|---|
| 冷启动到可交互 | ≤ 300 ms | 读一份 state 文件 + 渲染三页 |
| 常驻内存（RSS） | ≤ 120 MB | GTK4 本身约 60–80 MB，留给我们的额度不多 |
| 安装体积 | ≤ 15 MB | 不含 GTK 运行库 |
| 运行时硬依赖 | ≤ 4 个 | `gtk4`、`libadwaita`、`xprop`(x11-utils)、`desktop-file-utils` |
| 第三方 crate | ≤ 25 个（直接依赖） | 不引入 tokio、不引入 web 框架、不引入 ORM |
| 核心 crate 行数 | ≤ 15 k | 超过说明该砍功能了 |

### 0.4 由"轻"推导出的三个技术决策

这一条约束直接改掉了初版设计里的三个选择：

1. **存储用单文件 JSON，不用 SQLite** —— 砍掉 `rusqlite`（含 bundled SQLite 约 1.5 MB 二进制 + 一整套迁移机制）。几十个应用的规模，JSON 完全够；而且用户能直接看、能备份、能进 git，符合"工具透明"的气质。详见 §5。
2. **不引入 tokio，用 `std::thread` + GLib 主循环** —— GTK 已经在跑 GLib 的 MainContext，再叠一个 async runtime 属于双事件循环打架，且编译时间与体积都涨。子进程走 `gio::Subprocess`（GLib 原生，能直接接主循环），耗时任务扔线程池，结果用 `async-channel` 回主线程。详见 §4.4。
3. **不做数据库驱动的"库"概念** —— 没有"库"，就只有"前缀列表"和"应用列表"两个扁平的视图。没有库就没有同步、没有索引、没有后台扫描。

---

## 1. 一句话定位

**别的工具解决「怎么把游戏跑起来」，Carboy 解决「跑起来之后的一切」。**

启动之后的自动化才是真正的痛点，也是现有工具的空白区：

1. 应用跑通了，**没有一个正确的 `.desktop`** —— 每次都要从终端敲命令
2. 有了 `.desktop`，**图标是空的 / 是默认的酒瓶 / 是错的**
3. 图标对了，**任务栏不认它** —— dash-to-dock 把它归到 `wine.desktop` 下，或者干脆单独一个无名图标

Carboy 的核心不是「又一个启动器」，而是 **Freedesktop 集成引擎**：探测真实窗口身份、从 PE 资源里抠出真图标、生成符合规范的桌面项、装进图标主题、并且能告诉你**为什么不对**。

---

## 2. 现有工具为什么不满足

| 工具 | 前缀管理 | 自动 .desktop | 图标提取 | WM_CLASS 探测 | umu/Proton 无 Steam | 诊断能力 |
|---|---|---|---|---|---|---|
| Lutris | ✅ 脚本驱动 | ⚠️ 部分 | ⚠️ 弱 | ❌ | ✅（自己调 wine） | ❌ |
| Bottles | ✅ 强 | ✅ | ✅ 一般 | ⚠️ 靠猜测 | ⚠️ | ❌ |
| Heroic | ⚠️ | ✅（仅商店游戏） | ✅（商店 API） | ⚠️ | ✅（内置） | ❌ |
| WineGUI | ⚠️ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Carboy** | ✅ | ✅ | ✅ | ✅ **实测探测** | ✅ **umu 一等公民** | ✅ **核心卖点** |

| 工具 | 形态 | 重量来源 |
|---|---|---|
| Lutris | Python + GTK3 | 安装脚本运行环境、下载器、商店与模拟器后端 |
| Bottles | Python + GTK4 | 大量 winetricks 依赖管理、依赖安装器 |
| Heroic | Electron | 整个 Chromium |
| WineGUI | Python + GTK3 | 轻，但功能面也窄 |
| **Carboy** | **Rust 单二进制 + GTK4** | **只有 GTK 本身** |

### WineGUI 的三个具体失效点

1. **不生成 `.desktop`** —— 只维护自己的 JSON 配置，桌面环境完全不知道这些应用存在。
2. **图标缺失** —— Wine 前缀里的 `.exe` 图标是 PE 资源（`RT_GROUP_ICON`），不解析它就没有图标；WineGUI 也不抓窗口的 `_NET_WM_ICON`。
3. **dash-to-dock 图标不正确** —— 生成的桌面项**没有 `StartupWMClass`**。GNOME Shell 拿到窗口的 `WM_CLASS`（`res_class`）后在已注册应用里找匹配项，找不到就 fallback：
   - 匹配到 `wine.desktop` → 显示 Wine 酒瓶图标，且所有游戏挤成一组
   - 或者完全不匹配 → 任务栏出现一个无图标、无名字的条目

   **正确做法不是「猜一个 class」，而是启动进程后去 X11 里读真实值。** 这是 Carboy 的技术护城河。

---

## 3. 功能规格

### 3.1 P0 — 没有它就不是这个产品

#### A. 前缀（Prefix）管理
- 创建：选 Runner + 架构（win32/win64）+ Windows 版本；`wineboot -u` 异步执行，实时日志
- 克隆 / 备份（tar.zst）/ 恢复 / 删除；显示磁盘占用
- 打开 `winecfg` / `regedit` / `explorer`（每项独立进程，不阻塞 UI）
- DLL 覆盖（`WINEDLLOVERRIDES`）可视化编辑器：库名 + `native/builtin/disabled` 三态 + native,builtin 组合
- 环境变量：前缀级 key-value 表，支持从模板继承
- 驱动器映射查看（`dosdevices/` 解析）
- 「在此前缀运行 exe」拖放区

#### B. Runner 管理
- 来源：系统 wine（`/usr/bin/wine`）、Proton-GE、Wine-GE、CachyOS/dwproton 等自定义 Proton 构建、用户指定目录
- 安装 = 下载 tarball → sha256 校验 → 解压到 `~/.local/share/carboy/runners/<id>/` → 写 manifest
- 更新检查（GitHub Releases API）；离线性：允许手动「从本地文件导入」
- 每个 Runner 探测：是否支持 umu、wine 版本号、是否为 Proton（有 `proton` 脚本 / `files/bin/wine`）

#### C. 应用条目与启动
字段：名称、exe 路径、参数、工作目录、前缀、Runner、**启动模式**、环境变量、启动前后钩子脚本。

**启动模式三选一（这是 umu 集成的核心）**：

| 模式 | 何时用 | 实际执行的命令 |
|---|---|---|
| `wine` | 系统 wine / Wine-GE，不依赖 Proton 运行时 | `wine <exe>`（env 里设 `WINEPREFIX`） |
| `umu` | **用 Proton 跑，但不打开 Steam** | `umu-run <exe>`（env 里设 `GAMEID` / `PROTONPATH` / `WINEPREFIX`） |
| `proton-raw` | 高级：直接调 Proton 的 `proton run`（需自己给全 `STEAM_COMPAT_*`） | 默认不暴露在 UI，仅 CLI |

`umu` 模式细节（**已核 `umu_run.py` 源码，2026-11-10**）：

- **`GAMEID` 必须唯一，禁止 `0`。** umu 会把 `GAMEID` 里第一个 `-` 之后的部分赋给 `SteamAppId`，而 Proton 的 wine 用 `SteamAppId` 决定 `WM_CLASS`（`steam_app_<值>`）。也就是说：**GAMEID 相同的两个游戏，`WM_CLASS` 必然相同，在任务栏上必然合并成一个图标，并且共用一个前缀。**
  - 优先选 umu-database 的真 GAMEID（如 `umu-2358720`，能拿到 protonfixes）
  - 否则由 Carboy 生成 `carboy-<slug>`（放弃 fix，换唯一 class）
  - UI 上不给「用 0」这个选项，这是个会连带毁掉桌面集成的陷阱。详见 `desktop-integration.md` §1.1
- **`PROTONPATH`**：可以是 Runner 目录绝对路径，也可以是 `GE-Proton` 这类 token（让 umu 自己下载）。umu 会先尝试把 `STEAM_COMPAT/<值>` 当目录解析，失败才当 token 处理。
- **前缀归属（结论已明确）**：umu **尊重**外部传入的 `WINEPREFIX`，只有在这个变量**不存在**时才回退到 `~/Games/umu/$GAMEID`。
  - 但 `WINEPREFIX` **必须是绝对路径**，相对路径会直接报错拒绝启动
  - 设为空字符串也会报错（`WINEPREFIX` 为空是硬错误，不是回退）
  - 结论：Carboy 自己管的前缀可以正常交给 umu 用，`PrefixKind::UmuManaged` 只保留给"用户之前用 umu 裸跑留下的 `~/Games/umu/*`"，作为只读引用。

#### D. Freedesktop 集成（详见 `desktop-integration.md`）
- 一键生成/更新 `.desktop`，写入 `$XDG_DATA_HOME/applications/`
- **图标四级优先链**：窗口 `_NET_WM_ICON` → PE 资源提取 → 游戏目录里的 `.ico/.png` → 用户手选
- 多尺寸导出（16/24/32/48/64/128/256）到 `hicolor`，`Icon=<slug>` + 自动 `gtk-update-icon-cache`
- **`StartupWMClass` 自动探测**（实测，不猜）
- `update-desktop-database` 刷新；按桌面环境执行额外刷新（见 §7 矩阵）
- 桌面文件生命周期管理：列出、编辑、重新生成、删除（连带清理图标与缓存）、批量重建
- 「添加到桌面」：`~/.local/share/applications` + `gio set metadata::trusted true`（XFCE/Cinnamon 需要可执行位）

#### E. 诊断面板（差异化功能）
一个「为什么我的图标不对？」按钮，输出结构化检查结果：

```
✓ 桌面项已写入       /home/u/.local/share/applications/carboy-hades.desktop
✓ 桌面项语法合法     desktop-file-validate 通过
✓ 图标已安装         hicolor/256x256/apps/carboy-hades.png
✓ 图标名可解析       Icon=carboy-hades → /home/u/.local/share/icons/...
✓ WM_CLASS 已记录    StartupWMClass = "hades.exe"
✗ 冲突桌面项         另发现 2 个桌面项也声明 StartupWMClass=hades.exe
                     → 建议删除：lutris-hades.desktop, wine-hades.desktop
! Shell 缓存未刷新    GNOME Shell 在登录时索引应用
                     → [重启 Shell] [注销重登]
```

每项检查都是纯函数 + 可单测，UI 只负责渲染结果。

### 3.2 P1 — 薄功能（每个都必须能一句话说清价值，且实现只动 env 或几个文件）

- **图形栈开关**（每应用）：MangoHud、Gamescope 参数构造器、`PROTON_ENABLE_NVAPI`、`VKD3D_CONFIG`、`DXVK_ASYNC`。全部是往 env 里塞键值，零额外依赖。
- **lsfg-vk 集成**：读 `vulkan/implicit_layer.d/lsfg-vk.json` 判断装没装，一键注入 `LSFG_*`。纯文件读取 + env 注入，不碰你的 layer 本身。
- **时长统计**：等进程退出记时间差，数据本来就在 schema 里，几乎零成本。
- **CLI**（`carboy run/desktop/doctor`）：复用 core，不新增代码路径 —— 而且它是 M0/M1 的调试抓手。
- **前缀备份/恢复**：`tar --zstd`，一条命令。
- **日志查看**：本来就要为调试做，顺便给用户。
- **DXVK 组件「显示 + 手动指向」**：只解析前缀里现有 dll 的版本号，允许用户指向本地目录。**不做下载器**（那是 Lutris 的重量来源之一）。

### 3.3 P2 — 只有被反复提出才做

- **第三方库只读导入**：Lutris `games/*.yml`、Heroic `GamesConfig/*.json`、Steam `appmanifest_*.acf`。**单向、一次性、只读**，不做后台同步、不做监听。而且要**做成 CLI 子命令，不进 GUI** —— 保持 GUI 三页。
- 多语言（i18n 从第一天用 `gettext` 宏包裹，但只出 zh_CN / en）
- 打包：deb / rpm / AUR，外加一个静态构建

### 3.4 明确不做

见 §0.2 拒绝清单。这份清单是设计约束，不是待办。

---

## 4. 架构

### 4.1 Crate 划分

```
carboy/                      (cargo workspace)
├── crates/
│   ├── carboy-core/         领域层，零 UI 依赖，可单测，CLI 与 GUI 共用
│   │   ├── prefix/          创建/克隆/备份、wineboot、reg 读写、dosdevices
│   │   ├── runner/          发现/安装/校验/解压、版本探测
│   │   ├── launch/          LaunchPlan 构建（纯函数）、执行器、日志流、退出码
│   │   ├── desktop/         .desktop 序列化、Exec 转义、图标安装、索引刷新
│   │   ├── inspect/         WM_CLASS 探测、_NET_WM_ICON 抓取、PE 图标提取
│   │   ├── import/          第三方库只读导入（CLI only）
│   │   ├── doctor/          诊断规则集
│   │   └── state/           单文件 JSON 状态 + 原子写入 + 文件锁
│   ├── carboy-gtk/          GTK4 + libadwaita，只在 core 之上做编排与展示
│   └── carboy-cli/          `carboy` 二进制
└── data/                    GResource、图标、desktop 模板
```

**为什么把 `launch` 做成纯函数**：`LaunchPlan::build(app, prefix, runner, opts) -> LaunchPlan` 不碰 IO，环境变量矩阵、参数顺序、umu/wine 分支全部可以在测试里钉死。这类 bug 出了极难查，必须可测。

### 4.2 目录布局（XDG）

```
~/.local/share/carboy/          $XDG_DATA_HOME
├── state.json                  唯一状态文件（前缀/应用/Runner 元数据）
├── state.json.bak              上一版（原子写入时留的退路）
├── runners/<id>/               Runner 安装位置
├── icons/<slug>/               Carboy 内部保留的原始图标
├── logs/<app-slug>/            ★ 启动日志（轮转，保留最近 10 次）
└── cache/                      下载缓存、探测快照
~/.config/carboy/config.toml    $XDG_CONFIG_HOME
~/.local/share/applications/    生成的 .desktop（对外产物）
~/.local/share/icons/hicolor/   安装的图标（对外产物）
```

原则：**内部数据全部集中在 `carboy/` 下，对外只污染 `applications/` 和 `icons/hicolor/` 两处**，卸载时可干净回收。

### 4.3 首次启动一个应用的完整时序

```
UI                core                       OS / Desktop
 │ 点击「启动」      │
 ├────────────────>│ launch::build()          纯函数：拼 env/argv/cwd
 │                 │  → LaunchPlan
 │                 ├──> prefix.snapshot_windows()   xprop -root _NET_CLIENT_LIST
 │                 ├──> exec::spawn(plan, log_sink)
 │                 │        ├──────────────> umu-run / wine  → 游戏窗口出现
 │                 ├──> inspect::poll(60s, 200ms)
 │                 │        ├──> xprop -id <wid> WM_CLASS / _NET_WM_PID / _NET_WM_ICON
 │                 │        ├──> 候选窗口排序 + 置信度评分
 │ 弹「确认这个窗口吗」│<──────┤  （低置信度时才问，高置信度直接过）
 │<────────────────┤
 │ 确认             │
 ├────────────────>│ desktop::write(app, wm_class, icon)
 │                 │        ├──> 写 .desktop + StartupWMClass
 │                 │        ├──> 图标导多尺寸 + gtk-update-icon-cache
 │                 │        └──> update-desktop-database / kbuildsycoca
 │ 状态变绿         │<──────┤
 │                 ├──> 等待进程退出 → 记录时长 → 状态回 Idle
```

### 4.4 异步与进程模型（不引入 async runtime）

- **不用守护进程**。单进程 GUI，重活丢给 `std::thread`。
- **不用 tokio**。GTK 已经在跑 GLib 的 `MainContext`，再叠一个事件循环属于自己跟自己打架：增加编译时间、二进制体积、以及"为什么我的回调不执行"的调试成本。正确姿势是利用现有的事件循环：
  - 子进程用 `gio::Subprocess`（GLib 原生，`wait_async` 直接接 `MainContext`，还能逐行读 stdout/stderr）
  - 耗时纯计算（PE 图标解析、tar、备份）扔 `std::thread`，结果通过 `async-channel` 送回，`glib::MainContext::spawn_local` 落到 UI 线程
  - 需要取消的操作用 `gio::Cancellable`（GLib 自带，不用自己造）
- 日志视图用**环形缓冲**（5000 行）避免长时间运行内存膨胀。
- 长任务（wineboot、下载、解压、探测）全部 `Cancellable`，取消即杀进程组（`setsid` + `kill(-pgid)`，否则 wineserver 会留下孤儿）。
- **wineserver 会 fork 到后台**，这是所有超时逻辑的坑：不能靠「子进程退出」判断游戏结束。正确做法是等窗口消失 或 `wineserver -w`（等前缀内所有进程结束）。

---

## 5. 数据模型（单文件 JSON）

### 5.1 为什么不是 SQLite

初版设计用了 SQLite，按 §0 的"轻"原则被否掉了：

| 维度 | SQLite | 单文件 JSON（采用） |
|---|---|---|
| 二进制体积 | +~1.5 MB（bundled） | +~100 KB（serde_json） |
| 迁移机制 | 需要版本号 + 迁移脚本 | 一个 `schema_version` 字段 + 手写升级函数 |
| 可读/可改 | 需要 sqlite3 工具 | 文本编辑器直接看 |
| 备份 | 拷文件（写时需注意 WAL） | 拷文件 + `git diff` 能看懂 |
| 并发 | 内置 | 单进程 + 文件锁，够用 |
| 规模上限 | 百万级 | 几千条应用毫无压力 |

**决策**：`state.json`，原子写入（写 `.tmp` → `fsync` → `rename`），启动时若解析失败自动回滚 `state.json.bak` 并告知用户。
若将来真的出现性能问题（>5000 条），再引入 SQLite —— 那时 JSON 也能一键迁移，不存在锁死。

### 5.2 Schema 草案

```jsonc
{
  "schema_version": 1,
  "runners": [
    {
      "id": "proton-ge-9-20",
      "kind": "proton-ge",              // system-wine | wine-ge | proton-ge | custom-proton
      "display_name": "GE-Proton 9-20",
      "version": "9-20",
      "path": "/home/u/.local/share/carboy/runners/proton-ge-9-20",
      "is_proton": true,
      "source_url": "https://github.com/GloriousEggroll/proton-ge-custom/releases/...",
      "installed_at": "2026-11-10T13:40:00+08:00"
    }
  ],
  "prefixes": [
    {
      "id": "01J8Z...",
      "name": "Hades",
      "path": "/home/u/Games/hades",
      "kind": "managed",                // managed | umu-managed | external
      "runner_id": "proton-ge-9-20",
      "arch": "win64",
      "win_version": "win10",
      "env": {},                        // 前缀级覆盖
      "dll_overrides": { "d3d11": "native" },
      "created_at": "...", "last_used_at": "...",
      "size_bytes": 2147483648
    }
  ],
  "apps": [
    {
      "id": "01J8Z...",
      "prefix_id": "01J8Z...",
      "name": "Hades",
      "slug": "hades",                  // → desktop id 与图标名
      "exe_path": "/home/u/Games/hades/drive_c/Games/Hades/Hades.exe",
      "args": "",
      "workdir": "/home/u/Games/hades/drive_c/Games/Hades",
      "env": {},
      "launch_mode": "umu",             // wine | umu
      "gameid": "0",                    // umu 用
      "wm_class": "hades.exe",          // ★ 实测结果，核心字段
      "wm_class_source": "probed",      // probed | manual | inferred
      "desktop_path": "/home/u/.local/share/applications/carboy-hades.desktop",
      "desktop_hash": "sha256:...",     // 检测外部篡改 / 是否需要重新生成
      "icon_path": "/home/u/.local/share/carboy/icons/hades/256.png",
      "icon_source": "window",          // window | pe | file | manual
      "playtime_secs": 7250,
      "last_played_at": "..."
    }
  ]
}
```

**设计要点**：
- 只存**相对事实**（路径、实测值、用户意图），不存派生数据（如冲突列表）—— 派生数据每次诊断时现算，避免脏状态。
- `desktop_hash` 让"用户手工改过桌面项"这种情况可被检测：hash 不符就提示「你的修改会被覆盖，是否备份」，而不是静默覆盖。
- 所有时间用 RFC3339 字符串，人类可读。

---

## 6. 启动方案与 umu 集成

### 6.1 环境变量构建优先级（低 → 高）

```
1. Runner 默认值      （Proton 自带的、GE 建议的）
2. 前缀级 env         （WINEDLLOVERRIDES、WINEPREFIX）
3. 应用级 env         （用户的 per-game 覆盖）
4. 图形栈开关         （MANGOHUD、DXVK_*、LSFG_*、gamescope 注入）
5. 系统级强制项       （Carboy 自己需要的：WINEPREFIX 最终值、日志级别）
```

同一 key 高优先级覆盖低优先级；UI 上显示「最终生效值」与来源，避免「我设了为什么没用」。

### 6.2 umu 模式关键变量

```sh
# umu 模式，.desktop 里的 Exec 最终形态
Exec=env GAMEID=0 PROTONPATH=/home/u/.local/share/carboy/runners/proton-ge-9-20 \
     WINEPREFIX=/home/u/Games/hades umu-run "/home/u/Games/hades/drive_c/Games/Hades/Hades.exe"
```

**已核实的 umu 行为**（源码 `umu/umu_run.py`，1054 行版本）：

| 变量 | 行为 |
|---|---|
| `WINEPREFIX` 未设置 | 前缀 = `~/Games/umu/$GAMEID` |
| `WINEPREFIX` 已设置 | **直接使用**，但必须是绝对路径；相对路径或空字符串都会报错退出 |
| `GAMEID` 未设置 | umu 自己补 `umu-default` 并打 warning（不是错误） |
| `SteamAppId` | 由 GAMEID 派生并**强制覆盖**环境里传入的值 —— 想直接控制 class 只能改 GAMEID |
| `PROTONPATH` 未设置 | 若 `UMU_NO_PROTON != 1`，umu 会自己下载 `UMU-Latest` |

**仍需实测的部分**：
- `UMU_RUNTIME_UPDATE` / `UMU_NO_RUNTIME` 对离线用户的影响（运行时容器是否每次检查更新）
- umu 未安装时的降级路径：提示安装 + 一键切到 `wine` 模式

### 6.3 gamescope / MangoHud 包装

包装层在 argv 前插入，不在 env 里：`gamescope -W 2560 -H 1440 -r 144 -f -- <程序>`。
设计上 `LaunchPlan` 用 `Vec<WrapLayer>` 而不是字符串拼接，避免引号地狱。

---

## 7. 桌面集成摘要

完整原理、算法、各桌面差异矩阵、Exec 转义规则、故障对照表见 **`desktop-integration.md`**。三个要点：

1. **窗口−应用关联的唯一可靠钥匙是 `WM_CLASS` 的 `res_class` 字段**，写进 `StartupWMClass`。
2. **它的取值由 Runner 决定，可预测但必须验证**：上游 wine 用 exe 名小写；Proton 用 `steam_app_<GAMEID 派生值>` 或 `steam_proton`。**因此每个应用必须有唯一 GAMEID，否则 class 撞车、任务栏必然合并。** 详见 `desktop-integration.md` §1.1。
3. 图标必须**同时**做两件事：文件装进主题目录（多尺寸 + 刷新缓存）+ `Icon=` 名字可解析。只做一件都会「差一点」。

---

## 8. 风险与已知的坑

| 风险 | 说明 | 对策 |
|---|---|---|
| Wayland 下查不了 X11 | GTK4 应用跑在 Wayland 上，无法直接用 Xlib | **用 `xprop` 子进程**（连 XWayland），零依赖、跨桌面通用；探测模块不引入 x11 binding |
| `xprop` 未安装 | `x11-utils` 不是所有发行版默认装 | `doctor` 检测并提示；同时用 `xwininfo`/`xdotool` 做备选后端；最后兜底手填 |
| wineserver 后台化 | 进程树断裂，PID 关联失效 | 双策略：PID 树优先，退化到「时间窗内新窗口」启发式 + 用户点选确认 |
| 重复桌面项 | 多个工具写同一 `StartupWMClass`，Shell 随意选一个 | 诊断时扫描 `applications/*.desktop` 现算冲突（不落库）+ 一键删除 |
| Proton 改了 `WM_CLASS` | Proton 的 wine 把 class 硬编码成 `steam_app_<SteamAppId>` 或 `steam_proton`，与 exe 名无关；GAMEID 重复必然撞车 | 强制唯一 GAMEID；派生 class 用纯函数算，启动后验证，不一致就报警 |
| 图标缓存 | 改完图标 dock 不刷新 | 写完立即 `gtk-update-icon-cache -f -t`；提供「刷新 Shell」按钮 |
| Flatpak 沙箱 | 沙箱内写 `~/.local/share/applications` 对外不可见，等于白写 | **首发不做 Flatpak**，走原生包（AUR / deb / rpm）。必须做时用 `flatpak-spawn --host` |
| Exec 引号地狱 | 路径含空格/`%`/`$`/引号 → 桌面项静默失效 | 统一走一个 `escape_exec_arg()` 纯函数 + 单测覆盖 Desktop Entry 规范所有保留字符 |
| Proton 前缀被 umu 接管 | 用户在前缀管理里改了不生效 | 用 `PrefixKind::UmuManaged` 明确区分，UI 上只读 |

---

## 9. 里程碑

| 阶段 | 内容 | 验收标准 |
|---|---|---|
| **M0 骨架** | workspace、SQLite、Runner 发现、读 `xprop` 探测一个手工启动的游戏 | CLI 能打印出 `WM_CLASS` |
| **M1 集成核心** | `.desktop` 生成 + 图标提取 + 安装 + 刷新；`carboy desktop sync` | 手动写一个游戏条目，GNOME dock 显示正确图标且能合并窗口 |
| **M2 GUI** | 前缀/应用/Runner 三页 + 诊断面板 | 全程无终端操作完成「创建前缀→装游戏→启动→生成桌面项」 |
| **M3 启动器** | umu 模式、env 矩阵、日志视图、时长统计 | 用 Proton-GE 通过 umu 跑通一个非 Steam 游戏，桌面项一键启动 |
| **M4 打磨** | 图形栈开关、lsfg-vk、时长统计、打包；可选 CLI 导入 | 装一个 deb，全程无终端完成日常工作；冷启动 ≤ 300ms |

**先做 M0+M1 的 CLI，再做 GUI。** 这个产品的难点全在 M1，把它在无 UI 的情况下打磨到「换个游戏也能对」，GUI 只是壳。

---

## 10. 命名：已定为 Carboy

**Carboy** —— 大玻璃酒桶。一个桶装一堆酒瓶，正好对应"一个工具管一堆 prefix"；短、好打、辨识度高、无同名软件冲突。

命名一致性（三处必须统一，改名前先看这张表）：

| 位置 | 值 |
|---|---|
| crate 前缀 | `carboy-core` / `carboy-gtk` / `carboy-cli` |
| 数据目录 | `~/.local/share/carboy/` |
| 配置 | `~/.config/carboy/config.toml` |
| desktop id | `carboy-<slug>.desktop` |
| 图标名 | `carboy-<slug>` |
| 扩展字段 | `X-Carboy-App-Id` |
| CLI 命令 | `carboy run` / `carboy desktop sync` / `carboy doctor` |

（此前的候选 Decanter / Vintner 已废弃。）
