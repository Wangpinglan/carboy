# Carboy

为 Wine / Proton 应用生成可用的 `.desktop`。Rust + gtk4-rs + libadwaita，单窗口、两页签。

它只解决一件事：**桌面项能不能被 GNOME / KDE / XFCE 正确识别**。所以核心不是"写一个文件"，
而是三件容易断的事：

| 环节 | 做法 |
|---|---|
| 窗口身份 | `WM_CLASS` 由 Proton 的补丁决定，写成纯函数推导，启动后还能一键抓真实值 |
| 图标 | 从 exe 的 PE 资源里抠（`RT_GROUP_ICON` → `RT_ICON`），不依赖 icoutils |
| 缓存 | 装进 hicolor 七档尺寸 + 补 `index.theme` + `gtk-update-icon-cache -f` |

## 构建

```bash
sudo dnf install gtk4-devel libadwaita-devel cargo rust     # Fedora
sudo apt install libgtk-4-dev libadwaita-1-dev cargo rustc  # Debian / Ubuntu

cargo build --release
./target/release/carboy
```

## 打包

```bash
./packaging/build-rpm.sh     # → ../out/carboy-0.1.0-1.<dist>.x86_64.rpm
./packaging/build-deb.sh     # → ../out/carboy_0.1.0_amd64.deb
```

`build-rpm.sh` 自己打源码 tarball 再 `rpmbuild -bb`；`build-deb.sh` 用 `ar` + `tar`
手工组装（Fedora 上没有 dpkg-deb 也能出包）。两个脚本都会把产物放到仓库根的 `out/`。

本机 `/tmp` 是 10MB 的 tmpfs，脚本已把 `TMPDIR` 指到 `/var/tmp`，否则 cargo 会写满。

## 代码结构

```
src/
├── main.rs                 应用入口，固定 prgname = app id（XWayland 的 WM_CLASS 靠它）
├── engine/                 不依赖 GTK，可单测
│   ├── desktop.rs          Exec 转义、slug、GAMEID→WM_CLASS、.desktop 读写、偏好
│   ├── icons.rs            PE 资源解析 + DIB/PNG 解码 + hicolor 安装
│   ├── system.rs           umu/wine 定位、xprop 读窗口、依赖检查、构建计划
│   └── selfinstall.rs      把 Carboy 自己装进应用菜单
└── ui/
    ├── window.rs           主窗口：左表单 / 右预览、已生成列表
    └── dialogs.rs          构建对话框、窗口标识抓取、自定义预设
```

`engine` 里没有任何 GTK 依赖，逻辑改动可以直接 `cargo test` 验证。

## 两条硬规则

1. **GAMEID 必须以 `umu-` 开头。**
   `umu_run.py:279` 是 `if match(r"^umu-[\d\w]+$", UMU_ID)`，不匹配时 `SteamAppId` 保持 `0`，
   Proton 会把窗口 class 全写成 `steam_app_0` —— 所有游戏在任务栏合并成一个。
2. **`StartupWMClass` 大小写敏感。**
   GNOME 用 `g_str_hash` 精确比较，`Hades.exe` 与 `hades.exe` 不相等。代码里不做任何大小写美化。

## 许可证

MIT。参考过 Lutris / Heroic / Bottles / umu-launcher / wine / gnome-shell 的实现思路，
但没有复制它们的代码（那些是 GPL）。
