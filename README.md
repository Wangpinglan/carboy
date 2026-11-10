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

## 三条踩过的坑

1. `GAMEID` 必须以 `umu-` 开头，否则 umu 把 `SteamAppId` 写 0，所有游戏共用 `steam_app_0`。
2. `StartupWMClass` 大小写敏感（GNOME 用 `g_str_hash` 精确比较）。
3. 图标装了不显示，多半是用户目录的 hicolor 缺 `index.theme`，`gtk-update-icon-cache` 直接罢工。
