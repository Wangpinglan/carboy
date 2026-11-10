#!/usr/bin/python3
"""Carboy · 桌面项生成器（GTK4 + libadwaita）

左边填表，右边实时看到最终 .desktop。
图标可以直接从 .exe 里抠（PE 资源，不需要 icoutils）。
umu-launcher 填「目录」就行，会自动在里面找 umu-run。

运行：/usr/bin/python3 gui/mkdesktop-gui.py
"""

import shlex
import sys
import threading
from argparse import Namespace
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("GdkPixbuf", "2.0")
gi.require_version("Pango", "1.0")
from gi.repository import Adw, GdkPixbuf, Gio, GLib, Gtk, Pango  # noqa: E402

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "tools"))
import mkdesktop as core  # noqa: E402
import selfinstall  # noqa: E402
import umubuild  # noqa: E402
import winebuild  # noqa: E402

MAX_LOG_LINES = 3000

CACHE = Path.home() / ".cache" / "carboy" / "icons"
APPS = Path.home() / ".local/share/applications"
ICONS = Path.home() / ".local/share/icons/hicolor"

# 这几个由上面的字段生成，环境变量里再写会重复
RESERVED_ENV = {"WINEPREFIX", "GAMEID", "PROTONPATH"}

ENV_PRESETS = [
    ("MANGOHUD", "1", "MangoHud 覆盖层"),
    ("DXVK_HUD", "fps,frametimes", "DXVK 自带 HUD"),
    ("DXVK_ASYNC", "1", "dxvk-async（部分分支）"),
    ("PROTON_ENABLE_NVAPI", "1", "Proton 下开启 NVAPI"),
    ("VKD3D_CONFIG", "dxr", "VKD3D 光线追踪"),
    ("LSFG_MULTIPLIER", "2", "lsfg-vk 帧生成倍数"),
    ("WINEDLLOVERRIDES", "d3d11=n", "DLL 覆盖"),
]


def scaled_pixbuf(path, size):
    """按像素缩放，避免 Gtk.Picture 用原图尺寸把行撑开。"""
    try:
        return GdkPixbuf.Pixbuf.new_from_file_at_scale(str(path), size, size, True)
    except GLib.Error:
        return None


class BuildFlowDialog(Adw.Window):
    """通用的「查依赖 → 选源码目录 → 构建」对话框。umu 与 wine 共用。"""

    def __init__(self, parent, title, status_fn, hint_fn, source_ok, build_fn,
                 on_done, quick_label=None, quick_fn=None, extra_rows=None,
                 src_label="选择源码目录"):
        super().__init__(transient_for=parent, modal=True, title=title)
        self.set_default_size(820, 600)
        self._on_done = on_done
        self._status_fn = status_fn
        self._hint_fn = hint_fn
        self._source_ok = source_ok
        self._build_fn = build_fn
        self._quick_fn = quick_fn
        self._quick_label = quick_label
        self._src_label = src_label
        self._extra_rows = extra_rows or []

        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.head = Adw.HeaderBar()
        self.spinner = Gtk.Spinner()
        self.head.pack_start(self.spinner)
        b_close = Gtk.Button(label="关闭")
        b_close.connect("clicked", lambda *_: self.close())
        self.head.pack_end(b_close)
        box.append(self.head)

        self.stack = Gtk.Stack()
        self.stack.set_vexpand(True)
        self.stack.add_named(self._check_page(), "check")
        self.stack.add_named(self._log_page(), "log")
        box.append(self.stack)
        self.set_content(box)
        self._closed = False
        self.connect("close-request", self._on_close)

        b_reload = Gtk.Button(icon_name="view-refresh-symbolic")
        b_reload.set_tooltip_text("重新检查")
        b_reload.add_css_class("flat")
        b_reload.connect("clicked", lambda *_: self.refresh_check())
        self.head.pack_start(b_reload)

        self.refresh_check()

    def _on_close(self, *_):
        self._closed = True
        return False

    # ---------- 依赖检查 ----------
    def _check_page(self):
        wrap = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        page = Adw.PreferencesPage()

        self.g_deps = Adw.PreferencesGroup(title="依赖检查")
        page.add(self.g_deps)
        wrap.append(page)

        self.lbl_hint = Gtk.Label(xalign=0, wrap=True, selectable=True)
        self.lbl_hint.set_margin_top(8)
        self.lbl_hint.set_margin_bottom(8)
        self.lbl_hint.set_margin_start(16)
        self.lbl_hint.set_margin_end(16)
        self.lbl_hint.add_css_class("monospace")
        wrap.append(self.lbl_hint)

        bar = Gtk.Box(spacing=8)
        bar.set_margin_start(16)
        bar.set_margin_end(16)
        bar.set_margin_bottom(16)
        self.lbl_src = Gtk.Label(label="尚未选择源码目录", xalign=0, hexpand=True)
        self.lbl_src.add_css_class("dim-label")
        self.lbl_src.set_ellipsize(Pango.EllipsizeMode.MIDDLE)
        bar.append(self.lbl_src)

        if self._quick_fn:
            b_quick = Gtk.Button(label=self._quick_label or "装现成的")
            b_quick.connect("clicked", lambda *_: self.start_quick())
            bar.append(b_quick)

        self.b_pick = Gtk.Button(label=self._src_label)
        self.b_pick.add_css_class("suggested-action")
        self.b_pick.connect("clicked", lambda *_: self.pick_source())
        bar.append(self.b_pick)
        wrap.append(bar)
        return wrap

    def refresh_check(self):
        for row in getattr(self, "_dep_rows", []):
            self.g_deps.remove(row)
        self._dep_rows = []

        status = self._status_fn()
        bad = [t["name"] for t in status if t["required"] and not t["found"]]

        for t in status:
            row = Adw.ActionRow(title=t["name"])
            row.set_subtitle(t["note"] + ("" if t["required"] else "（可选）"))
            img = Gtk.Image.new_from_icon_name(
                "object-select-symbolic" if t["found"] else "dialog-warning-symbolic")
            img.add_css_class("success" if t["found"] else "warning")
            row.add_prefix(img)
            self.g_deps.add(row)
            self._dep_rows.append(row)

        for w in self._extra_rows:
            self.g_deps.add(w)
            self._dep_rows.append(w)

        if bad:
            self.lbl_hint.set_text(
                f"缺 {len(bad)} 项，装完点标题栏的刷新：\n\n" + self._hint_fn(bad))
        else:
            self.lbl_hint.set_text("依赖齐全，选源码目录开始构建。")
        self.b_pick.set_sensitive(not bad)

    def copy_hint(self, names):
        text = self._hint_fn(names)
        self.get_clipboard().set(text)
        self.lbl_hint.set_text(text)

    # ---------- 日志 ----------
    def _log_page(self):
        self.buf = Gtk.TextBuffer()
        tv = Gtk.TextView(buffer=self.buf)
        tv.set_monospace(True)
        tv.set_editable(False)
        tv.set_left_margin(12)
        tv.set_top_margin(8)
        self.sc = Gtk.ScrolledWindow()
        self.sc.set_child(tv)
        self.sc.set_vexpand(True)
        return self.sc

    def log(self, line):
        if getattr(self, "_closed", False):
            return
        GLib.idle_add(self._append, line)

    def _append(self, line):
        if getattr(self, "_closed", False):
            return False
        self.buf.insert(self.buf.get_end_iter(), line + "\n")
        # 构建日志动辄几万行，只保留最后 MAX_LOG_LINES 行
        n = self.buf.get_line_count()
        if n > MAX_LOG_LINES:
            r = self.buf.get_iter_at_line(n - MAX_LOG_LINES)
            cut = r[1] if isinstance(r, tuple) else r   # PyGObject 把 out 参数打包成元组
            self.buf.delete(self.buf.get_start_iter(), cut)
        adj = self.sc.get_vadjustment()
        GLib.idle_add(self._scroll_bottom, adj)
        return False

    @staticmethod
    def _scroll_bottom(adj):
        adj.set_value(adj.get_upper() - adj.get_page_size())
        return False

    # ---------- 动作 ----------
    def pick_source(self):
        dlg = Gtk.FileDialog()
        dlg.set_title("选择源码目录")
        dlg.select_folder(self, None, self._source_picked)

    def _source_picked(self, dlg, res):
        try:
            gfile = dlg.select_folder_finish(res)
        except GLib.Error:
            return
        src = Path(gfile.get_path())
        if not self._source_ok(src):
            self.lbl_src.set_text(f"{src} 不像源码目录")
            return
        self.lbl_src.set_text(str(src))
        self.start(lambda log: self._build_fn(src, log))

    def start_quick(self):
        self.start(self._quick_fn)

    def start(self, runner):
        self.stack.set_visible_child_name("log")
        self.spinner.start()
        self.buf.set_text("")
        self.log("$ 开始 …")
        threading.Thread(target=self._work, args=(runner,), daemon=True).start()

    def _work(self, runner):
        try:
            path = runner(self.log)
            GLib.idle_add(self._finish, path, None)
        except Exception as e:  # noqa: BLE001
            GLib.idle_add(self._finish, None, e)

    def _finish(self, path, err):
        self.spinner.stop()
        if err is not None:
            msg = str(err)
            if hasattr(err, "hint"):
                msg += "\n\n" + err.hint()
            self.log("\n✗ " + msg)
        else:
            self.log(f"\n✓ 完成：{path}\n已填回对应输入框。")
            self._on_done(path)
        return False

class WindowPickDialog(Adw.Window):
    """列出现有窗口，挑一个把它的 WM_CLASS 用作 StartupWMClass。"""

    def __init__(self, parent, wins, on_pick):
        super().__init__(transient_for=parent, modal=True, title="选择窗口")
        self.set_default_size(620, 460)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        head = Adw.HeaderBar()
        b_close = Gtk.Button(label="取消")
        b_close.connect("clicked", lambda *_: self.close())
        head.pack_end(b_close)
        box.append(head)

        page = Adw.PreferencesPage()
        g = Adw.PreferencesGroup(title="正在运行的窗口")
        g.set_description("点右侧「使用」把它的窗口标识写进这条启动项")
        for w in wins:
            row = Adw.ActionRow(title=w["title"] or "(无标题)")
            row.set_subtitle(w["wm_class"] or "没有 WM_CLASS")
            b = Gtk.Button(label="使用")
            b.add_css_class("flat")
            b.set_valign(Gtk.Align.CENTER)
            b.set_sensitive(bool(w["wm_class"]))
            b.connect("clicked", lambda _b, c=w["wm_class"]: (on_pick(c), self.close()))
            row.add_suffix(b)
            g.add(row)
        page.add(g)
        box.append(page)
        self.set_content(box)


class PresetDialog(Adw.Window):
    """新增一条环境变量预设。"""

    def __init__(self, parent, on_save):
        super().__init__(transient_for=parent, modal=True, title="添加预设")
        self.set_default_size(440, 280)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        head = Adw.HeaderBar()
        b_ok = Gtk.Button(label="保存")
        b_ok.add_css_class("suggested-action")
        b_ok.connect("clicked", lambda *_: self._save(on_save))
        head.pack_end(b_ok)
        box.append(head)

        page = Adw.PreferencesPage()
        g = Adw.PreferencesGroup(title="变量")
        self.e_k = Adw.EntryRow(title="KEY")
        self.e_v = Adw.EntryRow(title="VALUE")
        g.add(self.e_k)
        g.add(self.e_v)
        page.add(g)
        box.append(page)
        self.set_content(box)

    def _save(self, cb):
        k = self.e_k.get_text().strip()
        if not k:
            return
        cb(k, self.e_v.get_text())
        self.close()


class Window(Adw.ApplicationWindow):
    def __init__(self, app):
        super().__init__(application=app, title="Carboy · 桌面项生成器")
        self.set_default_size(1120, 760)

        self.f = {"exe": "", "workdir": "", "prefix": "", "proton": "", "icon": "",
                  "wine": "", "wmclass": ""}
        self.env_rows = []
        self._editing = None
        self._prefs = core.load_settings()
        raw_presets = self._prefs.get("env_presets")
        self.presets = raw_presets if isinstance(raw_presets, list) and raw_presets else [
            {"key": k, "value": v, "hint": h} for k, v, h in ENV_PRESETS]

        root = Adw.ToolbarView()
        header = Adw.HeaderBar()

        self.stack = Adw.ViewStack()
        self.switcher = Adw.ViewSwitcher(stack=self.stack,
                                         policy=Adw.ViewSwitcherPolicy.WIDE)
        header.set_title_widget(self.switcher)
        root.add_top_bar(header)

        self.stack.add_titled(self.build_body(), "gen", "生成")
        self.stack.add_titled(self.build_manager(), "manage", "已生成")
        for name, icon in (("gen", "document-new-symbolic"),
                           ("manage", "view-list-bullet-symbolic")):
            self.stack.get_page(self.stack.get_child_by_name(name)).set_icon_name(icon)
        self.stack.connect("notify::visible-child-name", self.on_page_changed)

        self.toast = Adw.ToastOverlay()
        self.toast.set_child(self.stack)
        root.set_content(self.toast)
        self.set_content(root)

        self._gid_manual = False
        self._setting_gid = False
        self._save_timer = None

        prefs = self._prefs
        for key in ("prefix", "proton", "workdir", "wine"):
            val = prefs.get(key) or ""
            if key == "wine" and not val:
                val = winebuild.find_wine() or ""
            if val:
                self.f[key] = val
                getattr(self, f"row_{key}").set_subtitle(val)
        umu = prefs.get("umu") or umubuild.find_umu() or ""
        if umu:
            self.r_umu.set_text(umu)

        self.sync_gameid()
        self.refresh()
        self.connect("close-request", self._on_close)

    # ---------------- 偏好设置 ----------------
    def _prefs_payload(self) -> dict:
        return {
            "prefix": self.f.get("prefix", ""),
            "proton": self.f.get("proton", ""),
            "workdir": self.f.get("workdir", ""),
            "umu": self.r_umu.get_text().strip(),
            "wine": self.f.get("wine", ""),
            "env_presets": self.presets,
        }

    def schedule_save(self):
        if self._save_timer:
            GLib.source_remove(self._save_timer)
        self._save_timer = GLib.timeout_add(700, self._do_save)

    def _do_save(self):
        self._save_timer = None
        core.save_settings(self._prefs_payload())
        return False

    def _on_close(self, *_):
        if self._save_timer:
            GLib.source_remove(self._save_timer)
            self._save_timer = None
        core.save_settings(self._prefs_payload())
        return False

    # ---------------- 界面 ----------------
    def build_body(self):
        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        box.set_homogeneous(True)

        page = Adw.PreferencesPage()
        page.set_size_request(460, -1)

        # ---- 应用 ----
        g_app = Adw.PreferencesGroup(title="应用")
        self.r_name = Adw.EntryRow(title="显示名称")
        self.r_name.set_tooltip_text("任务栏、应用菜单里显示的名字")
        self.r_name.connect("changed", self.on_name_changed)
        g_app.add(self.r_name)
        g_app.add(self.pick_row("游戏程序（.exe）", "exe", folder=False, on_pick=self.after_exe))
        page.add(g_app)

        # ---- 运行环境 ----
        g_run = Adw.PreferencesGroup(title="运行环境")
        self.r_mode = Adw.ComboRow(title="启动方式")
        self.r_mode.set_model(Gtk.StringList.new(
            ["umu + Proton（推荐，不需要 Steam）", "直接用 Wine"]))
        self.r_mode.connect("notify::selected", lambda *_: self.refresh())
        g_run.add(self.r_mode)
        g_run.add(self.pick_row("Wine 前缀（WINEPREFIX）", "prefix", folder=True))
        g_run.add(self.pick_row("Proton 版本", "proton", folder=True))
        self.row_wine = self.pick_row("Wine 程序", "wine", folder=False, on_pick=self.after_wine)
        g_run.add(self.row_wine)
        page.add(g_run)

        # ---- 图标 ----
        g_icon = Adw.PreferencesGroup(title="图标")
        self.row_icon = self.pick_row("图标", "icon", folder=False)
        g_icon.add(self.row_icon)
        self.r_install_icons = Adw.SwitchRow(title="安装到系统图标主题")
        self.r_install_icons.set_active(True)
        g_icon.add(self.r_install_icons)
        page.add(g_icon)

        # ---- 高级 ----
        g_adv = Adw.PreferencesGroup(title="高级")
        g_adv.set_description("一般不用改。改错只会让这条启动项失效，删掉重来即可。")

        self.r_umu = Adw.EntryRow(title="umu-run 路径")
        self.r_umu.connect("changed", lambda *_: self.refresh())
        b_find = Gtk.Button(icon_name="system-search-symbolic")
        b_find.set_valign(Gtk.Align.CENTER)
        b_find.add_css_class("flat")
        b_find.set_tooltip_text("自动查找")
        b_find.connect("clicked", self.auto_find_umu)
        b_file = Gtk.Button(icon_name="document-open-symbolic")
        b_file.set_valign(Gtk.Align.CENTER)
        b_file.add_css_class("flat")
        b_file.set_tooltip_text("手动选择")
        b_file.connect("clicked", lambda *_: self.pick_file_to_entry(self.r_umu))
        self.r_umu.add_suffix(b_find)
        self.r_umu.add_suffix(b_file)
        g_adv.add(self.r_umu)

        self.r_gameid = Adw.EntryRow(title="窗口标识（GAMEID）")
        self.r_gameid.set_tooltip_text("决定任务栏能否认出这个游戏，自动生成，改了就另算一个")
        self.r_gameid.connect("changed", self.on_gameid_changed)
        b_reset = Gtk.Button(icon_name="edit-undo-symbolic")
        b_reset.set_valign(Gtk.Align.CENTER)
        b_reset.add_css_class("flat")
        b_reset.set_tooltip_text("按名称重新生成")
        b_reset.connect("clicked", self.reset_gameid)
        b_grab = Gtk.Button(icon_name="system-search-symbolic")
        b_grab.set_valign(Gtk.Align.CENTER)
        b_grab.add_css_class("flat")
        b_grab.set_tooltip_text("从正在运行的窗口抓取真实标识")
        b_grab.connect("clicked", lambda *_: self.on_pick_window())
        self.r_gameid.add_suffix(b_grab)
        self.r_gameid.add_suffix(b_reset)
        g_adv.add(self.r_gameid)

        self.r_args = Adw.EntryRow(title="启动参数（可选）")
        self.r_args.set_tooltip_text("跟在 .exe 后面的参数，例如 -windowed")
        self.r_args.connect("changed", lambda *_: self.refresh())
        g_adv.add(self.r_args)

        g_adv.add(self.pick_row("工作目录（可选）", "workdir", folder=True))

        self.row_env = Adw.ExpanderRow(title="环境变量")
        self.row_env.set_subtitle("0 个")
        b_add = Gtk.Button(icon_name="list-add-symbolic")
        b_add.add_css_class("flat")
        b_add.set_tooltip_text("添加变量")
        b_add.connect("clicked", lambda *_: self.add_env_row())
        self.row_env.add_suffix(b_add)

        self.row_preset = Adw.ActionRow(title="从预设添加")
        self.row_preset.set_subtitle("常用变量一键加")
        b_pre = Gtk.MenuButton(icon_name="pan-down-symbolic")
        b_pre.add_css_class("flat")
        b_pre.set_valign(Gtk.Align.CENTER)
        self.preset_pop = Gtk.Popover()
        self.preset_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        self.preset_box.set_margin_top(6)
        self.preset_box.set_margin_bottom(6)
        self.preset_box.set_margin_start(6)
        self.preset_box.set_margin_end(6)
        self.preset_pop.set_child(self.preset_box)
        b_pre.set_popover(self.preset_pop)
        self.row_preset.add_suffix(b_pre)
        self.row_env.add_row(self.row_preset)
        g_adv.add(self.row_env)
        self.fill_preset_popover()

        row_build = Adw.ActionRow(title="安装或编译 umu-launcher")
        row_build.set_subtitle("先检查依赖，再选源码目录")
        b_build = Gtk.Button(label="打开")
        b_build.add_css_class("flat")
        b_build.set_valign(Gtk.Align.CENTER)
        b_build.connect("clicked", self.on_build_flow)
        row_build.add_suffix(b_build)
        g_adv.add(row_build)

        row_winebuild = Adw.ActionRow(title="编译 Wine")
        row_winebuild.set_subtitle("从源码构建到 build/wine；Proton 用现成的就行")
        b_wb = Gtk.Button(label="打开")
        b_wb.add_css_class("flat")
        b_wb.set_valign(Gtk.Align.CENTER)
        b_wb.connect("clicked", self.on_wine_flow)
        row_winebuild.add_suffix(b_wb)
        g_adv.add(row_winebuild)

        row_refresh = Adw.ActionRow(title="刷新图标与桌面索引")
        row_refresh.set_subtitle("图标改了不显示时点这里；Wayland 下仍需注销重登")
        b_rf = Gtk.Button(label="刷新")
        b_rf.add_css_class("flat")
        b_rf.set_valign(Gtk.Align.CENTER)
        b_rf.connect("clicked", lambda *_: self.do_refresh_all())
        row_refresh.add_suffix(b_rf)
        g_adv.add(row_refresh)

        self.row_self = Adw.ActionRow(title="把 Carboy 装进应用菜单")
        self.row_self.set_subtitle(self._self_state())
        b_self = Gtk.Button(label="安装")
        b_self.add_css_class("flat")
        b_self.set_valign(Gtk.Align.CENTER)
        b_self.connect("clicked", lambda *_: self.do_install_self())
        self.row_self.add_suffix(b_self)
        g_adv.add(self.row_self)
        page.add(g_adv)

        box.append(page)

        # ---- 右侧：成果预览 ----
        right = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        right.set_size_request(460, -1)
        right.add_css_class("card")

        hero = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        hero.set_margin_top(32)
        hero.set_margin_start(24)
        hero.set_margin_end(24)

        self.pic = Gtk.Image.new_from_icon_name("application-x-executable-symbolic")
        self.pic.set_pixel_size(96)
        self.pic.set_halign(Gtk.Align.CENTER)
        self.pic.add_css_class("dim-label")
        hero.append(self.pic)

        self.lbl_name = Gtk.Label(label="未命名", xalign=0.5, wrap=True)
        self.lbl_name.set_justify(Gtk.Justification.CENTER)
        self.lbl_name.add_css_class("title-2")
        hero.append(self.lbl_name)

        self.lbl_sub = Gtk.Label(label="", xalign=0.5, wrap=True)
        self.lbl_sub.add_css_class("dim-label")
        hero.append(self.lbl_sub)

        self.lbl_warn = Gtk.Label(label="", xalign=0.5, wrap=True)
        self.lbl_warn.set_justify(Gtk.Justification.CENTER)
        self.lbl_warn.add_css_class("caption")
        self.lbl_warn.set_margin_top(4)
        hero.append(self.lbl_warn)

        right.append(hero)

        btns = Gtk.Box(spacing=8)
        btns.set_halign(Gtk.Align.CENTER)
        btns.set_margin_top(20)
        btns.set_margin_bottom(20)
        self.btn_install = Gtk.Button(label="生成并安装")
        self.btn_install.add_css_class("suggested-action")
        self.btn_install.add_css_class("pill")
        self.btn_install.connect("clicked", lambda *_: self.generate(True))
        self.btn_save = Gtk.Button(label="仅保存")
        self.btn_save.add_css_class("pill")
        self.btn_save.connect("clicked", lambda *_: self.generate(False))
        btns.append(self.btn_install)
        btns.append(self.btn_save)
        right.append(btns)

        self.expander = Gtk.Expander(label="查看 .desktop 内容")
        self.expander.set_margin_start(16)
        self.expander.set_margin_end(16)
        self.expander.set_margin_bottom(16)
        self.expander.set_vexpand(True)
        exp_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        self.lbl_path = Gtk.Label(label="", xalign=0)
        self.lbl_path.add_css_class("caption")
        self.lbl_path.add_css_class("dim-label")
        self.lbl_path.set_ellipsize(Pango.EllipsizeMode.MIDDLE)
        exp_box.append(self.lbl_path)

        self.view = Gtk.TextView()
        self.view.set_monospace(True)
        self.view.set_editable(False)
        self.view.set_wrap_mode(Gtk.WrapMode.NONE)
        self.view.set_left_margin(6)
        self.view.set_top_margin(4)
        self.buf = self.view.get_buffer()
        sc = Gtk.ScrolledWindow()
        sc.set_child(self.view)
        sc.set_size_request(-1, 240)
        exp_box.append(sc)
        self.expander.set_child(exp_box)
        right.append(self.expander)

        box.append(right)
        return box

    # ---------------- 已生成 ----------------
    def build_manager(self):
        page = Adw.PreferencesPage()
        self.manage_group = Adw.PreferencesGroup()
        b_reload = Gtk.Button(icon_name="view-refresh-symbolic")
        b_reload.add_css_class("flat")
        b_reload.set_tooltip_text("重新扫描")
        b_reload.connect("clicked", lambda *_: self.reload_manager())
        self.manage_group.set_header_suffix(b_reload)
        page.add(self.manage_group)
        self._mrows = []
        return page

    def on_page_changed(self, *_):
        if self.stack.get_visible_child_name() == "manage":
            self.reload_manager()

    def reload_manager(self):
        for r in self._mrows:
            self.manage_group.remove(r)
        self._mrows.clear()

        items = core.list_generated(APPS)
        self.manage_group.set_title(f"已生成的桌面项（{len(items)}）")

        if not items:
            row = Adw.ActionRow(title="还没有生成过")
            row.set_subtitle("切到「生成」页做一个，这里就会出现")
            self.manage_group.add(row)
            self._mrows.append(row)
            return

        for info in items:
            slug = info["slug"]
            name = info.get("Name", slug)
            pe = core.parse_exec(info.get("Exec", "")) or {}
            lname = Path(pe.get("launcher", "")).name
            mode_txt = "umu" if lname.startswith("umu") else ("wine" if lname else "—")
            row = Adw.ExpanderRow(title=name,
                                  subtitle=f"{mode_txt} · {slug}.desktop")
            pic = Gtk.Image.new_from_icon_name("application-x-executable-symbolic")
            pic.set_pixel_size(20)
            for n in (256, 128, 64, 48, 32):
                f = ICONS / f"{n}x{n}" / "apps" / f"{slug}.png"
                if f.exists():
                    pb = scaled_pixbuf(f, 20)
                    if pb:
                        pic.set_from_pixbuf(pb)
                    break
            pic.set_valign(Gtk.Align.CENTER)
            row.add_prefix(pic)

            b_edit = Gtk.Button(icon_name="document-edit-symbolic")
            b_edit.add_css_class("flat")
            b_edit.set_valign(Gtk.Align.CENTER)
            b_edit.set_tooltip_text("编辑")
            b_edit.connect("clicked", lambda *_, i=info: self.load_for_edit(i))
            row.add_suffix(b_edit)

            b_del = Gtk.Button(icon_name="user-trash-symbolic")
            b_del.add_css_class("flat")
            b_del.set_valign(Gtk.Align.CENTER)
            b_del.set_tooltip_text("删除（含图标）")
            b_del.connect("clicked", lambda *_, i=info: self.ask_delete(i))
            row.add_suffix(b_del)

            for label, key in (("Exec", "Exec"), ("图标", "Icon"),
                               ("WM_CLASS", "StartupWMClass"),
                               ("文件", "path")):
                sub = Adw.ActionRow(title=label)
                sub.set_subtitle(info.get(key, "—"))
                sub.set_subtitle_selectable(True)
                row.add_row(sub)
            self.manage_group.add(row)
            self._mrows.append(row)

    def load_for_edit(self, info):
        """把一个已生成的桌面项反填回表单，slug 固定，保存时覆盖原文件。"""
        self._editing = {"slug": info["slug"]}
        self.r_name.set_text(info.get("Name", ""))

        pe = core.parse_exec(info.get("Exec", "")) or {}
        envs = dict(pe.get("env", {}))
        launcher = pe.get("launcher", "")
        if launcher:
            self.r_umu.set_text(launcher)
        name_l = Path(launcher).name if launcher else ""
        self.r_mode.set_selected(0 if name_l.startswith("umu") else 1)
        if name_l and not name_l.startswith("umu"):
            self.f["wine"] = launcher
            self.row_wine.set_subtitle(launcher)

        self.f["exe"] = pe.get("exe") or ""
        self.row_exe.set_subtitle(self.f["exe"] or "未选择")
        self.f["workdir"] = info.get("Path", "") or ""
        self.row_workdir.set_subtitle(self.f["workdir"] or "未选择")
        # 先无条件取出来，不能让 or 的短路跳过后面的 pop
        env_prefix = envs.pop("WINEPREFIX", "")
        env_proton = envs.pop("PROTONPATH", "")
        env_gid = envs.pop("GAMEID", "")

        self.f["prefix"] = info.get("X-Carboy-Prefix") or env_prefix or ""
        self.row_prefix.set_subtitle(self.f["prefix"] or "未选择")
        self.f["proton"] = info.get("X-Carboy-Proton") or env_proton or ""
        self.row_proton.set_subtitle(self.f["proton"] or "未选择")

        gid = info.get("X-Carboy-GameId") or env_gid
        self._setting_gid = True
        self.r_gameid.set_text(gid)
        self._setting_gid = False
        self._gid_manual = True

        self.r_args.set_text(" ".join(pe.get("args", [])))

        for r, _k, _v in self.env_rows[:]:
            self.row_env.remove(r)
        self.env_rows.clear()
        for k, v in envs.items():
            self.add_env_row(k, v)
        self.update_env_count()

        icon_path = ""
        f256 = ICONS / "256x256" / "apps" / f"{info['slug']}.png"
        if f256.exists():
            icon_path = str(f256)
        elif info.get("Icon", "").startswith("/"):
            icon_path = info["Icon"]
        self.f["icon"] = icon_path
        self.row_icon.set_subtitle(icon_path or "未选择")
        pb = scaled_pixbuf(icon_path, 96) if icon_path else None
        if pb:
            self.pic.set_from_pixbuf(pb)

        self.stack.set_visible_child_name("gen")
        self.refresh()
        self.toast.add_toast(Adw.Toast(
            title=f"已载入「{info.get('Name', '')}」，改完点「生成并安装」覆盖原文件", timeout=5))

    def ask_delete(self, info):
        dlg = Adw.AlertDialog(
            heading=f"删除「{info.get('Name', info['slug'])}」？",
            body="会删掉这个 .desktop、它安装的图标，然后刷新缓存。不可撤销。")
        dlg.add_response("cancel", "取消")
        dlg.add_response("delete", "删除")
        dlg.set_response_appearance("delete", Adw.ResponseAppearance.DESTRUCTIVE)
        dlg.connect("response", self.on_delete_response, info)
        dlg.present(self)

    def on_delete_response(self, dlg, resp, info):
        if resp != "delete":
            return
        removed = core.remove_generated(Path(info["path"]), ICONS, APPS)
        self.toast.add_toast(Adw.Toast(title=f"已删除（{len(removed)} 个文件）", timeout=4))
        self.reload_manager()

    def pick_row(self, title, key, folder, on_pick=None):
        row = Adw.ActionRow(title=title)
        row.set_subtitle(self.f.get(key) or "未选择")
        btn = Gtk.Button(icon_name="folder-open-symbolic")
        btn.add_css_class("flat")
        btn.set_valign(Gtk.Align.CENTER)
        btn.connect("clicked", lambda *_: self.pick(key, row, folder, on_pick))
        row.add_suffix(btn)
        setattr(self, f"row_{key}", row)
        return row

    # ---------------- 选择器 ----------------
    def pick(self, key, row, folder, on_pick=None):
        dlg = Gtk.FileDialog()
        dlg.set_title("选择" + ("目录" if folder else "文件"))
        if folder:
            dlg.select_folder(self, None, self._picked, (key, row, True, on_pick))
        else:
            dlg.open(self, None, self._picked, (key, row, False, on_pick))

    def pick_dir_to_entry(self, entry):
        dlg = Gtk.FileDialog()
        dlg.set_title("选择目录")
        dlg.select_folder(self, None, self._picked_entry, entry)

    def _picked_entry(self, dlg, res, entry):
        try:
            gfile = dlg.select_folder_finish(res)
        except GLib.Error:
            return
        entry.set_text(gfile.get_path() or "")

    def pick_file_to_entry(self, entry):
        dlg = Gtk.FileDialog()
        dlg.set_title("选择 umu-run")
        dlg.open(self, None, self._picked_file_entry, entry)

    def _picked_file_entry(self, dlg, res, entry):
        try:
            gfile = dlg.open_finish(res)
        except GLib.Error:
            return
        entry.set_text(gfile.get_path() or "")

    def _picked(self, dlg, res, data):
        key, row, folder, on_pick = data
        try:
            gfile = dlg.select_folder_finish(res) if folder else dlg.open_finish(res)
        except GLib.Error:
            return
        self.f[key] = gfile.get_path() or ""
        row.set_subtitle(self.f[key] or "未选择")
        if on_pick:
            on_pick()
        self.refresh()

    def auto_find_umu(self, *_):
        p = umubuild.find_umu()
        if p:
            self.r_umu.set_text(p)
        else:
            self.toast.add_toast(Adw.Toast(
                title="没找到 umu-run：先装 umu-launcher，或手动指定它的目录", timeout=5))

    def on_build_flow(self, *_):
        BuildFlowDialog(
            self, "安装或编译 umu-launcher",
            status_fn=umubuild.tool_status,
            hint_fn=umubuild.install_hint,
            source_ok=umubuild.is_source_dir,
            build_fn=lambda src, log: umubuild.build_source(src, log),
            on_done=self._after_install,
            quick_label="装现成的（uv/pipx）",
            quick_fn=lambda log: umubuild.install_prebuilt(log),
            src_label="选择 umu-launcher 源码目录",
        ).present()

    def on_wine_flow(self, *_):
        sw = Adw.SwitchRow(title="同时编译 32 位（i386）")
        sw.set_subtitle("需要额外的 32 位开发库；只跑 64 位游戏可以不选")
        BuildFlowDialog(
            self, "编译 Wine",
            status_fn=winebuild.tool_status,
            hint_fn=winebuild.install_hint,
            source_ok=winebuild.is_source_dir,
            build_fn=lambda src, log: winebuild.build(
                src, log, arch32=sw.get_active()),
            on_done=self._after_wine_built,
            extra_rows=[sw],
            src_label="选择 wine 源码目录",
        ).present()

    def on_pick_window(self):
        wins = core.list_x11_windows()
        if not wins:
            self.toast.add_toast(Adw.Toast(title="没抓到窗口（游戏要先跑起来）", timeout=4))
            return
        WindowPickDialog(self, wins, self._apply_wmclass).present()

    def _apply_wmclass(self, wm_class):
        self.f["wmclass"] = wm_class
        self.refresh()
        self.toast.add_toast(Adw.Toast(title=f"窗口标识已实测：{wm_class}", timeout=4))

    def _self_state(self) -> str:
        entry = selfinstall.data_home() / "applications" / f"{selfinstall.APP_ID}.desktop"
        return "已安装；dock 会按 app-id 认出这个窗口" if entry.exists() else \
            "没装的话，dock 里这个窗口只有 app-id 和一个齿轮图标"

    def do_install_self(self):
        try:
            selfinstall.install(gui=Path(__file__).resolve())
        except Exception as e:  # noqa: BLE001
            self.toast.add_toast(Adw.Toast(title=f"安装失败：{e}", timeout=6))
            return
        self.row_self.set_subtitle(self._self_state())
        self.toast.add_toast(Adw.Toast(
            title="已装进应用菜单，注销重登后 dock 图标就正常了", timeout=6))

    def do_refresh_all(self):
        done = core.refresh(ICONS, APPS)
        self.toast.add_toast(Adw.Toast(
            title="已刷新：" + ("、".join(done) if done else "没有可用的命令"), timeout=5))

    def _after_wine_built(self, path):
        self.f["wine"] = str(path)
        self.row_wine.set_subtitle(str(path))
        self.r_mode.set_selected(1)
        self.refresh()
        self.toast.add_toast(Adw.Toast(title=f"Wine 已就绪：{path}", timeout=5))

    def _after_install(self, path):
        if path:
            self.r_umu.set_text(str(path))
            self.toast.add_toast(Adw.Toast(title=f"已就绪：{path}", timeout=5))

    def after_wine(self):
        v = self.f.get("wine") or ""
        r = winebuild.resolve_wine(v)
        if r:
            self.f["wine"] = r
            self.row_wine.set_subtitle(r)
        elif v and winebuild.is_wine_source(Path(v).expanduser()):
            self.row_wine.set_subtitle(f"{v} —— 源码树，还没编译")
        elif v:
            self.row_wine.set_subtitle(f"{v} —— 不是可执行的 wine")

    # ---------------- 抠图标 ----------------
    def after_exe(self, force=False):
        exe = self.f.get("exe")
        if not exe:
            if force:
                self.toast.add_toast(Adw.Toast(title="先选 .exe", timeout=3))
            return
        slug = core.slugify(self.r_name.get_text().strip() or Path(exe).stem)
        try:
            png, size, metas = core.extract_icon(exe)
        except Exception as e:
            self.row_icon.set_subtitle(f"抠失败：{e}")
            return
        CACHE.mkdir(parents=True, exist_ok=True)
        out = CACHE / f"{slug}.png"
        out.write_bytes(png)
        self.f["icon"] = str(out)
        self.row_icon.set_subtitle(f"{out}　{size[0]}×{size[1]}（共 {len(metas)} 档，来自 exe）")
        pb = scaled_pixbuf(out, 96)
        if pb:
            self.pic.set_from_pixbuf(pb)
        self.refresh()

    # ---------------- 环境变量 ----------------
    def add_env_row(self, key="", value=""):
        row = Adw.ActionRow()
        box = Gtk.Box(spacing=6)
        e_k = Gtk.Entry(placeholder_text="KEY", width_chars=14)
        e_v = Gtk.Entry(placeholder_text="VALUE", hexpand=True)
        e_k.set_text(key)
        e_v.set_text(value)
        for e in (e_k, e_v):
            e.connect("changed", lambda *_: self.refresh())
        box.append(e_k)
        box.append(e_v)
        row.set_child(box)
        b_del = Gtk.Button(icon_name="user-trash-symbolic")
        b_del.add_css_class("flat")
        b_del.set_valign(Gtk.Align.CENTER)
        b_del.connect("clicked", lambda *_, r=row: self.del_env_row(r))
        row.add_suffix(b_del)
        self.row_env.add_row(row)
        self.env_rows.append((row, e_k, e_v))
        self.update_env_count()
        self.refresh()

    # ---------------- 预设 ----------------
    def fill_preset_popover(self):
        box = self.preset_box
        child = box.get_first_child()
        while child:
            nxt = child.get_next_sibling()
            box.remove(child)
            child = nxt

        for i, p in enumerate(self.presets):
            row = Gtk.Box(spacing=2)
            b = Gtk.Button(label=f"{p['key']}={p['value']}")
            b.add_css_class("flat")
            b.set_hexpand(True)
            b.set_halign(Gtk.Align.FILL)
            if p.get("hint"):
                b.set_tooltip_text(p["hint"])
            b.connect("clicked", self.add_preset, p["key"], p["value"], self.preset_pop)
            row.append(b)
            d = Gtk.Button(icon_name="window-close-symbolic")
            d.add_css_class("flat")
            d.set_valign(Gtk.Align.CENTER)
            d.set_tooltip_text("删除这个预设")
            d.connect("clicked", self.delete_preset, i)
            row.append(d)
            box.append(row)

        box.append(Gtk.Separator())
        b_add = Gtk.Button(label="添加预设…")
        b_add.add_css_class("flat")
        b_add.connect("clicked", lambda *_: self.open_preset_dialog())
        box.append(b_add)

    def delete_preset(self, _btn, index):
        if 0 <= index < len(self.presets):
            gone = self.presets.pop(index)
            self.fill_preset_popover()
            self._do_save()
            self.toast.add_toast(Adw.Toast(title=f"已删除预设 {gone['key']}", timeout=3))

    def open_preset_dialog(self):
        self.preset_pop.popdown()
        PresetDialog(self, self.add_new_preset).present()

    def add_new_preset(self, key, value):
        if key.upper() in RESERVED_ENV:
            self.toast.add_toast(Adw.Toast(
                title=f"{key} 由上方字段生成，不能存成预设", timeout=4))
            return
        for p in self.presets:
            if p["key"] == key:
                p["value"] = value
                break
        else:
            self.presets.append({"key": key, "value": value, "hint": "自定义"})
        self.fill_preset_popover()
        self._do_save()
        self.toast.add_toast(Adw.Toast(title=f"预设已保存：{key}", timeout=3))

    def add_preset(self, _btn, key, value, pop):
        pop.popdown()
        if key.upper() in RESERVED_ENV:
            self.toast.add_toast(Adw.Toast(
                title=f"{key} 由上方字段生成，不能当额外变量", timeout=4))
            return
        for _r, e_k, e_v in self.env_rows:
            if e_k.get_text().strip() == key:
                e_v.set_text(value)
                return
        self.add_env_row(key, value)

    def del_env_row(self, row):
        for i, (r, _k, _v) in enumerate(self.env_rows):
            if r is row:
                self.env_rows.pop(i)
                break
        self.row_env.remove(row)
        self.update_env_count()
        self.refresh()

    def collect_env(self):
        out = []
        for _r, e_k, e_v in self.env_rows:
            k = e_k.get_text().strip()
            if k and k.upper() not in RESERVED_ENV:
                out.append(f"{k}={e_v.get_text()}")
        return out

    def update_env_count(self):
        keys = [e_k.get_text().strip() for _r, e_k, _e in self.env_rows]
        live = [k for k in keys if k and k.upper() not in RESERVED_ENV]
        ignored = [k for k in keys if k and k.upper() in RESERVED_ENV]
        sub = f"{len(live)} 个 · 会写进 Exec 的 env 段"
        if ignored:
            sub += f"（{len(ignored)} 个由上方字段接管，已忽略）"
        self.row_env.set_subtitle(sub)

    # ---------------- GAMEID 跟随名称 ----------------
    def on_name_changed(self, *_):
        self.sync_gameid()
        self.refresh()

    def sync_gameid(self, *_):
        if self._gid_manual:
            return
        name = self.r_name.get_text().strip()
        self._setting_gid = True
        self.r_gameid.set_text(core.auto_gameid(name) if name else "")
        self._setting_gid = False

    def on_gameid_changed(self, *_):
        if not self._setting_gid:
            self._gid_manual = True
        self.refresh()

    def reset_gameid(self, *_):
        self.f["wmclass"] = ""
        self._gid_manual = False
        self.sync_gameid()
        self.refresh()

    # ---------------- 逻辑 ----------------
    def ns(self) -> Namespace:
        umu = self.r_mode.get_selected() == 0
        name = self.r_name.get_text().strip() or "未命名"
        gid = self.r_gameid.get_text().strip()
        if umu and not gid:
            gid = core.auto_gameid(name)
        raw_args = self.r_args.get_text().strip() if hasattr(self, "r_args") else ""
        try:
            args = shlex.split(raw_args) if raw_args else None
        except ValueError:
            args = raw_args.split() if raw_args else None
        return Namespace(
            name=name,
            name_zh=None, comment="",
            exe=self.f["exe"] or None,
            arg=args,
            prefix=self.f["prefix"] or None,
            umu_run=(self.r_umu.get_text().strip() or None) if umu else None,
            proton=(self.f["proton"] or None) if umu else None,
            wine_bin=(self.f["wine"] or None) if not umu else None,
            gameid=gid or None,
            env=self.collect_env() or None,
            wineprefix_env=bool(self.f["prefix"]),
            umu_use_steam=False,
            exec_raw=None, wmclass=(self.f.get("wmclass") or None),
            icon=self.f["icon"] or None,
            icon_name=None,
            slug=(self._editing or {}).get("slug"),
            workdir=self.f["workdir"] or None,
            categories="Game;", prefer_dgpu=False,
        )

    def refresh(self):
        umu = self.r_mode.get_selected() == 0
        self.r_gameid.set_sensitive(umu)
        self.row_proton.set_visible(umu)
        self.r_umu.set_visible(umu)
        self.row_wine.set_visible(not umu)
        self.update_env_count()

        umu_text = self.r_umu.get_text().strip()
        resolved = umubuild.resolve_umu(umu_text) if (umu and umu_text) else None

        try:
            ns = self.ns()
            wm, _why = core.derive_wmclass(ns)
            body = core.desktop_text(ns, wm)
        except Exception as e:
            self.buf.set_text(f"# 出错: {e}")
            return

        self.buf.set_text(body)
        slug = ns.slug or core.slugify(ns.name)
        self.lbl_path.set_text(str(APPS / f"{slug}.desktop"))

        self.lbl_name.set_text(ns.name if self.r_name.get_text().strip() else "未命名")
        self.lbl_sub.set_text(
            "通过 umu + Proton 启动" if umu else "直接用 Wine 启动")

        if not self.f["icon"]:
            self.pic.set_from_icon_name("application-x-executable-symbolic")
            self.pic.set_pixel_size(96)

        notes = []
        if umu and not resolved:
            notes.append("还缺 umu-run —— 到「高级」里点放大镜自动查找")
        if not umu and not self.f["wine"]:
            notes.append("还没选 wine 程序，系统里也没找到")
        if not self.f["exe"]:
            notes.append("先选一个 .exe")
        elif not self.f["icon"]:
            notes.append("这个程序没有可用图标")
        if self.f.get("wmclass") in ("steam_proton", "steam_app_0"):
            notes.append("这个标识是「所有 Proton 游戏共用」的，说明启动时 GAMEID 没生效，"
                         "用它会让任务栏把所有游戏合并")
        gid = self.r_gameid.get_text().strip()
        if umu and gid and not core.is_umu_gameid(gid):
            notes.append(f"GAMEID 必须以 umu- 开头（现在的 {gid} 会让 umu 把 "
                         "SteamAppId 设成 0，任务栏会把所有游戏并成一个）")
        if wm == "steam_proton":
            notes.append("窗口标识不唯一，任务栏可能把多个游戏合并")
        self.lbl_warn.set_text("\n".join(notes))
        self.schedule_save()

    def generate(self, install):
        ns = self.ns()
        wm, _ = core.derive_wmclass(ns)
        body = core.desktop_text(ns, wm)
        was_edit = self._editing is not None
        slug = ns.slug or core.slugify(ns.name)
        apps = APPS
        apps.mkdir(parents=True, exist_ok=True)
        target = apps / f"{slug}.desktop"
        icons_root = ICONS
        icon_note = ""
        # 先装图标再写桌面项：反过来的话，桌面环境可能先看到一条没有图标的记录并缓存下来
        if install and self.f["icon"] and Path(self.f["icon"]).exists():
            core.install_icon(Path(self.f["icon"]), slug, icons_root)
            icon_note = " + 图标（7 档）"

        target.write_text(body, encoding="utf-8")
        target.chmod(0o644)

        msg = f"{'已更新' if was_edit else '已写入'} {target.name}{icon_note}"
        if install and self.r_install_icons.get_active():
            core.refresh(icons_root, apps)
            msg += "，缓存已刷新"
        self.toast.add_toast(Adw.Toast(title=msg, timeout=4))
        self._editing = None
        if was_edit:
            self.reload_manager()


class App(Adw.Application):
    def __init__(self):
        super().__init__(application_id="io.github.carboy.MkDesktop",
                         flags=Gio.ApplicationFlags.DEFAULT_FLAGS)
        # XWayland 下 WM_CLASS 取自 prgname，不设的话是 "mkdesktop-gui.py"，
        # 跟 app-id 对不上，Shell 就会退化成伪应用（齿轮图标 + 光秃秃的 id）
        GLib.set_prgname(selfinstall.APP_ID)
        Gtk.Window.set_default_icon_name(selfinstall.APP_ID)

    def do_activate(self):
        win = self.props.active_window or Window(self)
        win.present()


if __name__ == "__main__":
    App().run(sys.argv)
