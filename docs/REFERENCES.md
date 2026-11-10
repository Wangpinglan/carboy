# Carboy — 参考源码清单

> 原则：**只读需要的那几个文件**，不要全仓库啃。
> 带 ★ 的是"决定架构、必须在上手写代码前读完"的。

---

## 总览

| # | 仓库 | 优先级 | 为什么 |
|---|---|---|---|
| 1 | ★ umu-launcher | P0 | 我们要集成的对象，必须懂它怎么接管前缀 |
| 2 | ✅ wine | 已读 | 就在本地 `Proton/wine/`，结论已入档，不用 clone |
| 3 | ★ gnome-shell | P0 | 窗口↔应用匹配算法的最终解释权 |
| 4 | ★ Lutris | P0 | 桌面集成有现成实现 + 反面教材（重量从哪来） |
| 5 | ★ Heroic | P0 | 最成熟的"自动创建桌面项"实现之一 |
| 6 | Bottles | P0 | 同为 GTK4 的 Wine 管理器，UI 范式最接近 |
| 7 | WineGUI | P0 | 你的现用工具，明确它缺口的边界 |
| 8 | glib（gio） | P1 | `StartupWMClass` 的比较逻辑 |
| 9 | desktop-file-utils | P1 | `.desktop` 合法性校验的规则源头 |
| 10 | dash-to-dock | P1 | 确认它只消费 Shell 的关联结果 |
| 11 | ProtonUp-Qt | P1 | Runner 下载/安装，与我们的管理器几乎同构 |
| 12 | proton-ge-custom | P1 | release 命名与压缩包结构 |
| 13 | Proton | P1 | `proton` 脚本本体的 env 矩阵 |
| 14 | gtk4-rs | P1 | 骨架与 `gio::Subprocess` 用法 |
| 15 | loupe | P1 | 小而干净的 gtk4-rs + libadwaita 真实项目 |
| 16 | relm4 | P1 | 若要 MVVM |
| 17 | fractal | P1 | 较大型 gtk4-rs 的状态管理 |
| 18 | MangoHud | P1 | `MANGOHUD_CONFIG` 全部键值 |
| 19 | gamescope | P1 | 参数语义，写构造器要用 |
| 20 | kwin | P2 | KDE 的 resourceClass 匹配 |
| 21 | pelite | P2 | PE 资源解析（当依赖用） |
| 22 | umu-database | P2 | GAMEID 列表，可直接当数据 |
| 23 | winetricks | P2 | DLL 覆盖语义 / verbs |
| 24 | dxvk / vkd3d-proton | P2 | 只在做组件版本管理时 |

一键 clone 脚本：`refs/clone-refs.sh`（见文末）。

---

## 网络：没梯子也能拉（2026-11-10 实测）

| 源 | 实测 | 说明 |
|---|---|---|
| `raw.githubusercontent.com` | **200 / 0.87s** | 直连可用，单文件用 curl 最快 |
| `github.com` clone | 时快时慢 | 大仓库常被限速，用镜像前缀 |
| `gitlab.winehq.org` raw | 200 / **10.3s** | 能连，但慢，别反复拉大文件 |
| `gitlab.gnome.org` raw | 406 | 需要带 Accept 头，或者干脆用 `git clone` |
| `gh-proxy.com` | 200 / 0.49s | 可用 |
| `gitclone.com` | 200 / 0.40s | 可用 |

**优先"只取文件"，不要 clone 整个仓库：**

```bash
# 单文件直连（最快，umu_run.py 0.9 秒）
curl -sO https://raw.githubusercontent.com/Open-Wine-Components/umu-launcher/main/umu/umu_run.py

# gnome-shell 的 raw 返回 406，加个 Accept 头即可
curl -sH "Accept: */*" -o shell-window-tracker.c \
  "https://gitlab.gnome.org/GNOME/gnome-shell/-/raw/main/src/shell-window-tracker.c"
```

**必须 clone 时走镜像：**

```bash
MIRROR=https://gh-proxy.com/ ./refs/clone-refs.sh p0
```

脚本只会给 `github.com` 的地址套前缀，GitLab 的走直连（实测 GitLab 直连比镜像快）。

---

## P0：先读这几个，再动手

### 1. ★ umu-launcher — `github.com/Open-Wine-Components/umu-launcher`

**为什么**：我们的 `umu` 启动模式完全依赖它。它决定了前缀归谁管、环境变量怎么传、和 pressure-vessel 怎么交互。**这里没搞清，数据模型就是错的。**

必读文件：

- `umu/umu_run.py` —— 主流程。重点看：
  - `GAMEID` / `PROTONPATH` / `WINEPREFIX` 三个变量的解析顺序与默认值
  - 它**是否**覆盖用户传入的 `WINEPREFIX`（默认 `~/Games/umu/$GAMEID`）—— 这直接决定我们能不能复用自己管的前缀
  - `STEAM_COMPAT_*` 系列是它自己算的，还是要求调用方给
- `umu/umu_consts.py` —— 环境变量常量表，抄一份做对照，别自己编名字
- `umu/steamutil.py` —— 它怎么构造兼容数据目录、怎么找 proton
- `docs/` 或 `README.md` 里的 `PROTONPATH` 传值说明（绝对路径 vs `GE-Proton` 别名）

**落地动作**：读完立刻写一个 `tests/umu_probe.rs`，跑 `umu-run` 并观察实际生效的 `WINEPREFIX`。

---

### 2. ✅ wine —— **不需要 clone，你本地就有**

**结论已拿到（2026-11-10 核实），本节从"待读"改为"已读"。**

`~/项目/LinkToProton → /mnt/fn_nas/Main/dyingAfish/Proton` 这份 Proton 仓库的 `.gitmodules` 里有：

```
[submodule "wine"]
	path = wine
	url = ../wine
```

也就是说 **wine 源码就在 `Proton/wine/`**，而且是 Valve 的 fork —— 比上游 wine 更有价值，因为**游戏实际跑的就是这一份**。直接读：

```bash
W=~/项目/LinkToProton/wine/dlls/winex11.drv
sed -n '1236,1263p' $W/window.c          # set_initial_wm_hints()：Proton 的 class 补丁
sed -n '455,480p'   $W/x11drv_main.c     # process_name 的取值（上游行为）
```

读出来的结论已经写进 `desktop-integration.md` §1.1，摘要：

- **上游 wine / Wine-GE**：`res_class` = exe 文件名小写（含 `.exe`）
- **Proton（含 dwproton / GE-Proton）**：打了补丁，`res_class` = `steam_app_<SteamAppId>` 或 `steam_proton`，**与 exe 名无关**
- 这是"多个游戏挤成一个图标"的根因

还剩一个小尾巴没查：`_NET_WM_ICON` 的尺寸档位（`window.c` 里 `set_net_wm_icon` / `window_set_net_wm_icon`）。实现图标抓取前补一下。

---

### 3. ✅ gnome-shell —— 匹配逻辑已读完

**结论已入档**（`desktop-integration.md` §1.2）。GitHub 镜像比 GitLab 快得多，raw 直连即可：

```bash
curl -sO https://raw.githubusercontent.com/GNOME/gnome-shell/main/src/shell-window-tracker.c
curl -sO https://raw.githubusercontent.com/GNOME/gnome-shell/main/src/shell-app-system.c
```

`shell-window-tracker.c` 的 `get_app_for_window()` 给出 6 级回退链；`shell-app-system.c` 的 `lookup_startup_wmclass()` 告诉我们两件事：

- 索引表是 `g_str_hash / g_str_equal` → **大小写敏感，精确匹配**（这条此前写错了）
- 多个桌面项抢同一 class 时，优先「id 与 class 完全对应」的那个

还剩 `_NET_WM_PID` 的具体实现没细看（`get_app_from_window_pid`），做探测兜底时再补。

---

### 4. ★ Lutris — `github.com/lutris/lutris`

**为什么**：两用。① 桌面集成有现成实现可对照；② 它是"重"的标本，读它就知道重量从哪来，避免重蹈。

- `lutris/util/desktop.py` —— 生成 `.desktop`、写 icon、调 `update-desktop-database` 的完整实现。对照我们的 §5 转义规则看它漏了什么
- `lutris/runners/wine.py` —— env 矩阵、DXVK/VKD3D 版本管理、`WINEDLLOVERRIDES` 拼装方式。这是它最有价值的部分
- `lutris/game.py` —— 看它的数据模型有多庞大（反思：我们只需要它的 1/10）
- `lutris/installer/` —— **只需要知道它存什么格式**，不用读实现。这就是"重量"的来源：一整套社区脚本解释器

---

### 5. ★ Heroic — `github.com/Heroic-Games-Launcher/HeroicGamesLauncher`

**为什么**：Linux 下"自动创建桌面快捷方式 + 图标"最成熟的 Electron 实现，踩过的坑都写在代码里了。

- `src/backend/shortcuts/` —— 整个目录
  - Linux 分支里 `.desktop` 的模板、`Icon=` 的写法、icon 下载与落盘位置（`~/.local/share/icons/...`）
  - 它怎么处理**重复快捷方式**与**删除时清理**
- `src/backend/utils.ts` —— `createNeutralinoWindow`? 不，找 `getSteamInstallPath` / 路径工具那部分

**重点抄思路**：它的 icon 落盘路径命名规则，以及它对 `StartupWMClass` 的处理（它踩过同样的坑）。

---

### 6. Bottles — `github.com/bottlesdevs/Bottles`

**为什么**：同为 GTK4 + libadwaita 的 Wine 管理器，**UI 范式与交互密度最接近我们要做的东西**。

- `bottles/frontend/windows/` —— 窗口组织方式（它用 GtkStack + 侧栏，正好是"三页封顶"的参考）
- `bottles/frontend/views/` —— 列表项、详情页的信息层级
- `bottles/backend/wine/` —— 它怎么管 wine 版本、怎么执行 wine 命令
- `bottles/backend/managers/` —— 配置持久化方式（对比我们的 JSON 方案）

**反思点**：Bottles 也很重（Python 依赖 + winetricks 全家桶），正好用它验证"哪些功能可以不做"。

---

### 7. WineGUI — `gitlab.melroy.org/melroy/winegui`

**为什么**：你的现用工具。读它是为了**精确画出现有缺口**，而不是重造它已有的东西。

- 看它怎么存 prefix 元数据（大概在 `winegui/` 下的 config 模块）
- 看它构造 wine 命令行的位置 —— 对照我们的 `LaunchPlan`
- 确认：**它确实没有生成 `.desktop` 的代码路径**（验证你的痛点判断，也确认它没有我们没发现的隐藏功能）

---

## P1：实现到对应模块时再翻

| 仓库 | 直接去看 |
|---|---|
| `gitlab.gnome.org/GNOME/glib` | `gio/gdesktopappinfo.c`：`g_desktop_app_info_match_wmclass()`（比较是否大小写敏感）、`get_startup_wm_class()`、`g_desktop_app_info_new()` 的查找顺序 |
| `gitlab.freedesktop.org/xdg/desktop-file-utils` | `src/validate.c` —— `.desktop` 每条规则的实现，写序列化时逐条对照 |
| `github.com/micheleg/dash-to-dock` | `appIcons.js`、`docking.js` —— 确认它只是消费 `Shell.App`，自己不算关联（所以修好 Shell 层就通吃） |
| `github.com/DavidoTek/ProtonUp-Qt` | 下载 → 校验 → 解压 → 安装到 `compatibilitytools.d` 的流程，我们的 Runner 管理器几乎同构 |
| `github.com/GloriousEggroll/proton-ge-custom` | release 命名（`GE-Proton9-20.tar.gz`）与包内目录结构 |
| `github.com/ValveSoftware/Proton` | 根目录 `proton` 脚本 —— `STEAM_COMPAT_DATA_PATH` 等变量的必需项。你本地有 dwproton 可对照 |
| `github.com/gtk-rs/gtk4-rs` | `examples/` 全过一遍，重点 `gio_subprocess`、`application`、`preferences_window` |
| `gitlab.gnome.org/GNOME/loupe` | 真实项目骨架：`src/` 目录怎么切模块、`resources/` 怎么组织 GResource |
| `github.com/Relm4/relm4` | 若用 MVVM：`examples/` 里的 `todo` 与 `components` |
| `gitlab.gnome.org/GNOME/fractal` | 复杂状态管理与异步的组织方式（我们暂时用不上，看结构即可） |
| `github.com/flightlessmango/MangoHud` | `MANGOHUD_CONFIG` 的完整键值表 |
| `github.com/ValveSoftware/gamescope` | `--help` 与 `src/main.cpp` 的参数解析 |

---

## P2：需要时再说

- `invent.kde.org/plasma/kwin` → `src/x11window.cpp`（`resourceClass` / `resourceName`）—— 只做 KDE 深适配时
- `github.com/CasualX/pelite` → 当依赖用，看 `examples/resources.rs`
- `github.com/Open-Wine-Components/umu-database` → GAMEID 列表，可直接拉来当数据文件
- `github.com/Winetricks/winetricks` → DLL 覆盖语义、verbs 定义
- `github.com/doitsujin/dxvk`、`github.com/HansKristian-Work/vkd3d-proton` → 组件版本管理时
- `github.com/sonic2kk/steamtinkerlaunch` → per-game wrapper 组合的极端案例，思路参考

---

## 许可证警告（重要）

| 仓库 | 许可证 | 约束 |
|---|---|---|
| Lutris / Bottles / Heroic | GPL-3.0 | **读思路可以，抄代码会传染**，整个项目必须变 GPL |
| gnome-shell | GPL-2.0+ | 同上 |
| wine | LGPL-2.1+ | 链接才传染，读不受限 |
| umu-launcher | GPL-3.0 | 我们是"调用"它（子进程），不链接，不受传染 |

**建议**：Carboy 采用 **MIT 或 Apache-2.0**，参考仓库**只读不抄**。如果哪天想直接搬 Lutris 的 `desktop.py`，那一刻就得决定是否转 GPL —— 提前想好。

---

## 第一周的阅读路线

| 天 | 读什么 | 状态 |
|---|---|---|
| 1 | `umu-launcher` 的 `umu_run.py` | ✅ 已完成 —— `WINEPREFIX` 被尊重（须绝对路径）、`GAMEID` 派生出 `SteamAppId` |
| 2 | `wine` 的 `dlls/winex11.drv/` | ✅ 已完成 —— Proton 把 class 改成 `steam_app_*`，见 §1.1 |
| 3 | `gnome-shell` 的 `shell-window-tracker.c` + `shell-app-system.c` | ✅ 已完成 —— 6 级回退、**大小写敏感**、冲突仲裁规则，见 §1.2 |
| 4 | Lutris `util/desktop.py` + Heroic `shortcuts/` | 待做：看它们漏了哪几个字段 |
| 5 | **动手**：20 行脚本启动一个 exe，`xprop` 打出 class | 待做：用实测验证前几天的结论 |

第 5 天那 20 行脚本，就是 M0 的第一个验收物。
