# Carboy

给 Wine / Proton 应用生成能被桌面环境正确识别的 `.desktop`。

| 目录 | 说明 |
|---|---|
| `rs/` | **正式实现**：Rust + gtk4-rs + libadwaita，打包脚本与产物都从这里出 |
| `gui/` `tools/` | 早期 Python 原型（功能等价，保留作对照，不再维护） |
| `docs/` | 设计文档：架构、Freedesktop 集成、参考源码清单 |
| `refs/` | 上游源码快照与克隆脚本（读思路，不抄代码） |
| `out/` | 构建产物：`carboy-0.1.0-1.fc45.x86_64.rpm`、`carboy_0.1.0_amd64.deb` |

## 快速开始

```bash
cd rs
make build && ./target/release/carboy     # 直接跑
make rpm && make deb                      # 出包（产物在 ../out）
```

依赖：`gtk4-devel` / `libgtk-4-dev`、`libadwaita-devel` / `libadwaita-1-dev`、`cargo`、`rustc`。

细节见 [rs/README.md](rs/README.md)。

## 踩过的坑

1. `GAMEID` 必须以 `umu-` 开头，否则 umu 把 `SteamAppId` 写 0，所有游戏共用 `steam_app_0`。
2. `StartupWMClass` 大小写敏感（GNOME 用 `g_str_hash` 精确比较）。
3. 图标装了不显示，多半是用户目录的 hicolor 缺 `index.theme`，`gtk-update-icon-cache` 直接罢工。
4. **建完前缀必须把 Windows 版本钉到 `win10`**。Wine 11 / dwproton 的 `wineboot` 默认建出来是
   Windows 11 build 22000，而 WineGUI 建的无条件补跑一次 `winetricks win10`（19045）——
   原神这类游戏在 Win11 build 22000 的前缀里直接闪退。见 `system::PREFIX_WINVER`。
5. `PROTONPATH` 必须是带 `toolmanifest.vdf` 的 Proton **根目录**；选成 `compatibilitytools.d`
   这种容器目录时 umu 会 `FileNotFoundError`，所以选完要往下探一层（`system::resolve_proton`）。
