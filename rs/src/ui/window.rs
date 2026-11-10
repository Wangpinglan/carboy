//! 主窗口：左边填表、右边预览，两个页签（生成 / 已生成）。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::engine::desktop::{self, Form, Preset, Settings};
use crate::engine::{icons, system};
use crate::ui::dialogs;

#[derive(Debug, Default, Clone)]
pub struct Files {
    pub exe: String,
    pub prefix: String,
    pub proton: String,
    pub wine: String,
    pub workdir: String,
    pub icon: String,
}

pub struct EnvRow {
    pub row: adw::ActionRow,
    pub key: gtk::Entry,
    pub value: gtk::Entry,
}

pub struct Ui {
    pub window: adw::ApplicationWindow,
    pub stack: gtk::Stack,
    pub toast: adw::ToastOverlay,

    pub r_name: adw::EntryRow,
    pub r_args: adw::EntryRow,
    pub r_umu: adw::EntryRow,
    pub r_gameid: adw::EntryRow,
    pub r_wmclass: adw::EntryRow,
    pub r_mode: adw::ComboRow,
    pub r_install_icons: adw::SwitchRow,
    pub row_exe: adw::ActionRow,
    pub row_prefix: adw::ActionRow,
    pub row_proton: adw::ActionRow,
    pub row_wine: adw::ActionRow,
    pub row_workdir: adw::ActionRow,
    pub row_icon: adw::ActionRow,
    pub row_env: adw::ExpanderRow,
    b_preset: gtk::MenuButton,

    pub pic: gtk::Image,
    pub lbl_name: gtk::Label,
    pub lbl_sub: gtk::Label,
    pub lbl_warn: gtk::Label,
    pub lbl_path: gtk::Label,
    pub buf: gtk::TextBuffer,

    pub mgroup: adw::PreferencesGroup,
    mrows: RefCell<Vec<adw::PreferencesRow>>,

    pub files: RefCell<Files>,
    pub env_rows: RefCell<Vec<EnvRow>>,
    pub presets: RefCell<Vec<Preset>>,
    pub editing: RefCell<Option<String>>,
    pub gid_manual: RefCell<bool>,
    pub setting_gid: RefCell<bool>,
}

const ICON: &str = "io.github.carboy.MkDesktop";

pub fn build(app: &adw::Application) -> Rc<Ui> {
    let saved = desktop::load_settings();

    // ------------------------------------------------------------ 表单
    let r_name = adw::EntryRow::builder().title("显示名称").build();

    let row_exe = adw::ActionRow::builder()
        .title("游戏程序")
        .subtitle("未选择")
        .build();
    let b_exe = pick_button();
    row_exe.add_suffix(&b_exe);

    let r_args = adw::EntryRow::builder()
        .title("启动参数（一般留空）")
        .build();

    let g_app = adw::PreferencesGroup::builder().title("应用").build();
    g_app.add(&r_name);
    g_app.add(&row_exe);
    g_app.add(&r_args);

    let r_mode = adw::ComboRow::builder()
        .title("启动方式")
        .model(&gtk::StringList::new(&["umu + Proton", "直接 Wine"]))
        .selected(0)
        .build();

    let row_prefix = adw::ActionRow::builder()
        .title("Wine 前缀")
        .subtitle("未选择")
        .build();
    let b_prefix = pick_button();
    row_prefix.add_suffix(&b_prefix);

    let row_proton = adw::ActionRow::builder()
        .title("Proton 版本")
        .subtitle("未选择")
        .build();
    let b_proton = pick_button();
    row_proton.add_suffix(&b_proton);

    let row_wine = adw::ActionRow::builder()
        .title("Wine 程序")
        .subtitle("未选择")
        .build();
    let b_wine = pick_button();
    row_wine.add_suffix(&b_wine);
    row_wine.set_visible(false);

    let g_run = adw::PreferencesGroup::builder().title("运行环境").build();
    g_run.add(&r_mode);
    g_run.add(&row_prefix);
    g_run.add(&row_proton);
    g_run.add(&row_wine);

    // ---- 环境变量
    let row_env = adw::ExpanderRow::builder()
        .title("额外变量")
        .subtitle("0 个")
        .build();
    let b_add = gtk::Button::from_icon_name("list-add-symbolic");
    b_add.add_css_class("flat");
    b_add.set_valign(gtk::Align::Center);
    b_add.set_tooltip_text(Some("加一条"));
    let b_preset = gtk::MenuButton::builder().label("预设").build();
    b_preset.add_css_class("flat");
    b_preset.set_valign(gtk::Align::Center);
    b_preset.set_always_show_arrow(false);
    b_preset.set_tooltip_text(Some("从预设添加"));
    row_env.add_suffix(&b_preset);
    row_env.add_suffix(&b_add);

    let g_env = adw::PreferencesGroup::builder().title("环境变量").build();
    g_env.add(&row_env);

    // ---- 图标
    let row_icon = adw::ActionRow::builder()
        .title("图标")
        .subtitle("不选就用 exe 里的图标")
        .build();
    let b_icon = pick_button();
    row_icon.add_suffix(&b_icon);
    let r_install_icons = adw::SwitchRow::builder()
        .title("安装到图标主题并刷新缓存")
        .active(true)
        .build();

    let g_icon = adw::PreferencesGroup::builder().title("图标").build();
    g_icon.add(&row_icon);
    g_icon.add(&r_install_icons);

    // ---- 高级
    let r_umu = adw::EntryRow::builder().title("umu-run 路径").build();
    let b_umu = pick_button();
    b_umu.set_tooltip_text(Some("选一个目录，自动在里面找 umu-run"));
    r_umu.add_suffix(&b_umu);
    let r_gameid = adw::EntryRow::builder()
        .title("GAMEID（决定窗口标识，必须 umu- 开头，留空自动推导）")
        .build();
    let b_gid_reset = gtk::Button::from_icon_name("view-refresh-symbolic");
    b_gid_reset.add_css_class("flat");
    b_gid_reset.set_valign(gtk::Align::Center);
    b_gid_reset.set_tooltip_text(Some("按名称重新生成"));
    r_gameid.add_suffix(&b_gid_reset);
    let r_wmclass = adw::EntryRow::builder().title("窗口标识 StartupWMClass（留空自动推导）").build();
    let b_grab = gtk::Button::from_icon_name("system-search-symbolic");
    b_grab.add_css_class("flat");
    b_grab.set_valign(gtk::Align::Center);
    b_grab.set_tooltip_text(Some("从正在运行的窗口抓取"));
    r_wmclass.add_suffix(&b_grab);
    let row_workdir = adw::ActionRow::builder()
        .title("工作目录")
        .subtitle("默认 exe 所在目录")
        .build();
    let b_workdir = pick_button();
    row_workdir.add_suffix(&b_workdir);

    let (row_umu_build, b_umu_build) = tool_row("构建 umu-launcher", "从源码编译，或装预编译包");
    let (row_wine_build, b_wine_build) = tool_row("编译 Wine", "从源码构建到 build/wine");
    let (row_refresh, b_refresh) = tool_row("刷新图标与桌面索引", "改完图标不显示时点这里");

    let adv = adw::ExpanderRow::builder().title("高级").build();
    adv.add_row(&r_umu);
    adv.add_row(&r_gameid);
    adv.add_row(&r_wmclass);
    adv.add_row(&row_workdir);
    adv.add_row(&gtk::Separator::new(gtk::Orientation::Horizontal));
    adv.add_row(&row_umu_build);
    adv.add_row(&row_wine_build);
    adv.add_row(&row_refresh);

    let g_adv = adw::PreferencesGroup::new();
    g_adv.add(&adv);

    let page = adw::PreferencesPage::new();
    page.add(&g_app);
    page.add(&g_run);
    page.add(&g_env);
    page.add(&g_icon);
    page.add(&g_adv);

    // ------------------------------------------------------------ 预览
    let pic = gtk::Image::from_icon_name("application-x-executable");
    pic.set_pixel_size(84);
    pic.set_halign(gtk::Align::Center);

    let lbl_name = gtk::Label::builder()
        .label("未命名")
        .halign(gtk::Align::Center)
        .wrap(true)
        .build();
    lbl_name.add_css_class("title-2");

    let lbl_sub = gtk::Label::builder()
        .label("通过 umu + Proton 启动")
        .halign(gtk::Align::Center)
        .build();
    lbl_sub.add_css_class("dim-label");

    let lbl_warn = gtk::Label::builder()
        .label("")
        .xalign(0.0)
        .wrap(true)
        .visible(false)
        .build();
    lbl_warn.add_css_class("caption");
    lbl_warn.add_css_class("warning");

    let btn_install = gtk::Button::with_label("生成并安装");
    btn_install.add_css_class("suggested-action");
    btn_install.add_css_class("pill");
    let btn_save = gtk::Button::with_label("仅保存");
    btn_save.add_css_class("pill");

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::Center);
    actions.append(&btn_save);
    actions.append(&btn_install);

    let buf = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
    let tv = gtk::TextView::builder()
        .buffer(&buf)
        .monospace(true)
        .editable(false)
        .top_margin(8)
        .left_margin(10)
        .build();
    let tv_scroll = gtk::ScrolledWindow::builder()
        .min_content_height(220)
        .child(&tv)
        .build();
    let lbl_path = gtk::Label::builder()
        .label("")
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .build();
    lbl_path.add_css_class("caption");
    lbl_path.add_css_class("dim-label");

    let text_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    text_box.set_margin_top(8);
    text_box.set_margin_bottom(8);
    text_box.set_margin_start(10);
    text_box.set_margin_end(10);
    text_box.append(&lbl_path);
    text_box.append(&tv_scroll);
    let expander = adw::ExpanderRow::builder().title("查看 .desktop 内容").build();
    expander.add_row(&text_box);
    let g_preview = adw::PreferencesGroup::new();
    g_preview.add(&expander);

    let right_page = adw::PreferencesPage::new();
    right_page.add(&g_preview);
    let right_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&right_page)
        .build();

    let right = gtk::Box::new(gtk::Orientation::Vertical, 10);
    right.set_width_request(380);
    right.set_margin_top(18);
    right.set_margin_bottom(18);
    right.set_margin_start(6);
    right.set_margin_end(6);
    right.append(&pic);
    right.append(&lbl_name);
    right.append(&lbl_sub);
    right.append(&lbl_warn);
    right.append(&actions);
    right.append(&right_scroll);

    let left_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&page)
        .build();

    let paned = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    paned.append(&left_scroll);
    paned.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    paned.append(&right);
    paned.set_hexpand(true);
    paned.set_vexpand(true);

    // ------------------------------------------------------------ 已生成
    let mgroup = adw::PreferencesGroup::builder().title("已生成").build();
    let mpage = adw::PreferencesPage::new();
    mpage.add(&mgroup);
    let m_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&mpage)
        .build();

    // ------------------------------------------------------------ 组装
    let stack = gtk::Stack::new();
    stack.add_titled(&paned, Some("gen"), "生成");
    stack.add_titled(&m_scroll, Some("manage"), "已生成");

    let header = adw::HeaderBar::new();
    let switcher = gtk::StackSwitcher::builder().stack(&stack).build();
    header.set_title_widget(Some(&switcher));

    // 右上角主菜单：关于
    let b_menu = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("菜单")
        .build();
    let about_pop = gtk::Popover::new();
    let b_about = gtk::Button::builder()
        .label("关于 Carboy")
        .css_classes(["flat"])
        .build();
    about_pop.set_child(Some(&b_about));
    b_menu.set_popover(Some(&about_pop));

    let b_new = gtk::Button::with_label("新建");
    b_new.add_css_class("flat");
    b_new.set_tooltip_text(Some("清空表单，从头做一条新的"));
    // pack_end 先包的靠最右：菜单占右上角，新建在它左边
    header.pack_end(&b_menu);
    header.pack_end(&b_new);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&header);
    root.append(&stack);

    let toast = adw::ToastOverlay::new();
    toast.set_child(Some(&root));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Carboy · 桌面项生成器")
        .default_width(1120)
        .default_height(780)
        .content(&toast)
        .build();
    gtk::Window::set_default_icon_name(ICON);

    // 关于对话框：图标 + 名称 + 版本徽章 + 许可证 + 主页
    b_about.connect_clicked({
        let window = window.clone();
        let pop = about_pop.clone();
        move |_| {
            pop.popdown();
            let dlg = adw::AboutDialog::builder()
                .application_name("Carboy")
                .application_icon(ICON)
                .version(env!("CARGO_PKG_VERSION"))
                .comments("为 Wine / Proton 应用生成能被桌面环境正确识别的 .desktop")
                .developers(["Carboy contributors"])
                .license_type(gtk::License::MitX11)
                .website("https://github.com/Wangpinglan/carboy")
                .build();
            dlg.present(Some(&window));
        }
    });

    let ui = Rc::new(Ui {
        window,
        stack,
        toast,
        r_name,
        r_args,
        r_umu,
        r_gameid,
        r_wmclass,
        r_mode,
        r_install_icons,
        row_exe,
        row_prefix,
        row_proton,
        row_wine,
        row_workdir,
        row_icon,
        row_env,
        b_preset: b_preset.clone(),
        pic,
        lbl_name,
        lbl_sub,
        lbl_warn,
        lbl_path,
        buf,
        mgroup,
        mrows: RefCell::new(Vec::new()),
        files: RefCell::new(Files::default()),
        env_rows: RefCell::new(Vec::new()),
        presets: RefCell::new(saved.env_presets.clone()),
        editing: RefCell::new(None),
        gid_manual: RefCell::new(false),
        setting_gid: RefCell::new(false),
    });

    ui.load_prefs(&saved);
    ui.wire(&b_exe, &b_prefix, &b_proton, &b_wine, &b_icon, &b_workdir,
            &b_add, &b_grab, &b_gid_reset, &b_new, &b_preset, &b_umu_build, &b_wine_build,
            &b_refresh, &btn_install, &btn_save, &b_umu);
    ui.reload_manager();
    ui.refresh();
    if std::env::var_os("CARBOY_PAGE").map(|v| v == "manage").unwrap_or(false) {
        ui.stack.set_visible_child_name("manage");
    }
    // 仅在 debug 构建里存在的自检钩子：直接走「编辑」按钮调用的那条路径，
    // 用于在无头环境验证回填是否正常
    #[cfg(debug_assertions)]
    if std::env::var_os("CARBOY_SELFTEST").is_some() {
        if let Some(e) = desktop::list_generated().first().cloned() {
            ui.load_for_edit(&e);
            println!(
                "SELFTEST name={:?} gameid={:?} slug={:?} wm={:?}",
                ui.r_name.text(),
                ui.r_gameid.text(),
                ui.editing.borrow().clone(),
                ui.r_wmclass.text()
            );
            glib::timeout_add_seconds_local_once(1, || {
                std::process::exit(0);
            });
        }
    }
    ui
}

fn pick_button() -> gtk::Button {
    let b = gtk::Button::with_label("选择");
    b.add_css_class("flat");
    b.set_valign(gtk::Align::Center);
    b
}

fn tool_row(title: &str, subtitle: &str) -> (adw::ActionRow, gtk::Button) {
    let row = adw::ActionRow::builder().title(title).subtitle(subtitle).build();
    let b = gtk::Button::with_label("打开");
    b.add_css_class("flat");
    b.set_valign(gtk::Align::Center);
    row.add_suffix(&b);
    (row, b)
}


impl Ui {    fn window(&self) -> &adw::ApplicationWindow {
        &self.window
    }

    pub fn umu_mode(&self) -> bool {
        self.r_mode.selected() == 0
    }

    fn load_prefs(&self, s: &Settings) {
        {
            let mut f = self.files.borrow_mut();
            f.prefix = s.prefix.clone();
            f.proton = s.proton.clone();
            f.workdir = s.workdir.clone();
            f.wine = s.wine.clone();
        }
        self.row_prefix.set_subtitle(if s.prefix.is_empty() { "未选择" } else { &s.prefix });
        self.row_proton.set_subtitle(if s.proton.is_empty() { "未选择" } else { &s.proton });
        self.row_workdir.set_subtitle(if s.workdir.is_empty() { "默认 exe 所在目录" } else { &s.workdir });
        self.row_wine.set_subtitle(if s.wine.is_empty() { "未选择" } else { &s.wine });

        let umu = if !s.umu.is_empty() {
            s.umu.clone()
        } else {
            system::find_umu().unwrap_or_default()
        };
        self.r_umu.set_text(&umu);
        if !s.wine.is_empty() || self.files.borrow().wine.is_empty() {
            if self.files.borrow().wine.is_empty() {
                if let Some(w) = system::find_wine() {
                    self.files.borrow_mut().wine = w.clone();
                    self.row_wine.set_subtitle(&w);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn wire(
        self: &Rc<Self>,
        b_exe: &gtk::Button,
        b_prefix: &gtk::Button,
        b_proton: &gtk::Button,
        b_wine: &gtk::Button,
        b_icon: &gtk::Button,
        b_workdir: &gtk::Button,
        b_add: &gtk::Button,
        b_grab: &gtk::Button,
        b_gid_reset: &gtk::Button,
        b_new: &gtk::Button,
        b_preset: &gtk::MenuButton,
        b_umu_build: &gtk::Button,
        b_wine_build: &gtk::Button,
        b_refresh: &gtk::Button,
        btn_install: &gtk::Button,
        btn_save: &gtk::Button,
        b_umu: &gtk::Button,
    ) {
        let ui = self;
        ui.r_name.connect_changed({
            let ui = Rc::clone(ui);
            move |_| {
                if !*ui.gid_manual.borrow() {
                    ui.sync_gameid();
                }
                ui.refresh();
            }
        });
        ui.r_args.connect_changed(clone_refresh(ui));
        ui.r_umu.connect_changed(clone_refresh(ui));
        ui.r_gameid.connect_changed({
            let ui = Rc::clone(ui);
            move |_| {
                if !*ui.setting_gid.borrow() {
                    *ui.gid_manual.borrow_mut() = true;
                }
                ui.refresh();
            }
        });
        ui.r_wmclass.connect_changed(clone_refresh(ui));
        ui.r_mode.connect_selected_notify(clone_refresh(ui));

        b_exe.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| ui.pick_exe()
        });
        b_prefix.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| ui.pick_dir("prefix", "Wine 前缀")
        });
        b_proton.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| ui.pick_dir("proton", "Proton 目录")
        });
        b_wine.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| ui.pick_wine()
        });
        b_umu.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| ui.pick_umu()
        });
        b_icon.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| ui.pick_icon()
        });
        b_workdir.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| ui.pick_dir("workdir", "工作目录")
        });

        b_add.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| {
                ui.add_env_row("", "");
                ui.refresh();
            }
        });
        b_grab.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| dialogs::open_window_pick(&ui)
        });
        b_gid_reset.connect_clicked({            let ui = Rc::clone(ui);
            move |_| {
                *ui.gid_manual.borrow_mut() = false;
                ui.sync_gameid();
                ui.refresh();
            }
        });
        b_new.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| ui.new_entry()
        });
        b_preset.set_popover(Some(&build_preset_popover(ui)));

        b_umu_build.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| dialogs::open_build(&ui, system::Kind::Umu)
        });
        b_wine_build.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| dialogs::open_build(&ui, system::Kind::Wine)
        });
        b_refresh.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| {
                let done = desktop::refresh();
                ui.toast_msg(&format!(
                    "已刷新：{}",
                    if done.is_empty() { "没有可用命令".into() } else { done.join("、") }
                ));
            }
        });

        btn_save.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| ui.generate(false)
        });
        btn_install.connect_clicked({
            let ui = Rc::clone(ui);
            move |_| ui.generate(true)
        });

        ui.stack.connect_visible_child_name_notify({
            let ui = Rc::clone(ui);
            move |_| {
                if ui.stack.visible_child_name().as_deref() == Some("manage") {
                    ui.reload_manager();
                }
            }
        });

        ui.window.connect_close_request({
            let ui = Rc::clone(ui);
            move |_| {
                ui.save_prefs();
                glib::Propagation::Proceed
            }
        });
    }

    // ------------------------------------------------------------ 文件选择

    fn pick_exe(self: &Rc<Self>) {
        let dlg = gtk::FileDialog::new();
        dlg.set_title("选择游戏程序");
        let filter = gtk::FileFilter::new();
        filter.add_pattern("*.exe");
        filter.add_pattern("*.EXE");
        filter.add_pattern("*");
        let list = gio::ListStore::new::<gtk::FileFilter>();
        list.append(&filter);
        dlg.set_filters(Some(&list));
        let ui = Rc::clone(self);
        dlg.open(Some(self.window()), None::<&gio::Cancellable>, move |res| {
            if let Ok(f) = res {
                if let Some(p) = f.path() {
                    ui.set_file("exe", &p);
                }
            }
        });
    }

    fn pick_dir(self: &Rc<Self>, key: &'static str, title: &'static str) {
        let dlg = gtk::FileDialog::new();
        dlg.set_title(title);
        let ui = Rc::clone(self);
        dlg.select_folder(Some(self.window()), None::<&gio::Cancellable>, move |res| {
            if let Ok(f) = res {
                if let Some(p) = f.path() {
                    ui.set_file(key, &p);
                }
            }
        });
    }

    fn pick_wine(self: &Rc<Self>) {
        let dlg = gtk::FileDialog::new();
        dlg.set_title("选择 wine 可执行文件");
        let ui = Rc::clone(self);
        dlg.open(Some(self.window()), None::<&gio::Cancellable>, move |res| {
            if let Ok(f) = res {
                if let Some(p) = f.path() {
                    ui.set_file("wine", &p);
                }
            }
        });
    }

    /// 选一个目录，自动在里面找 umu-run（resolve_umu 也会兼容文件路径，
    /// 但对话框本身只选目录；直接选到文件的需求走手动填路径）。
    fn pick_umu(self: &Rc<Self>) {
        let dlg = gtk::FileDialog::new();
        dlg.set_title("选择 umu-launcher 目录");
        let ui = Rc::clone(self);
        dlg.select_folder(Some(self.window()), None::<&gio::Cancellable>, move |res| {
            let Ok(f) = res else { return };
            let Some(p) = f.path() else { return };
            let resolved = system::resolve_umu(&p.to_string_lossy());
            if resolved.is_empty() {
                ui.toast_msg("这个目录里没有 umu-run，手动把路径填进输入框");
                return;
            }
            ui.r_umu.set_text(&resolved);
            ui.refresh();
        });
    }

    fn pick_icon(self: &Rc<Self>) {
        let dlg = gtk::FileDialog::new();
        dlg.set_title("选择图标");
        let filter = gtk::FileFilter::new();
        filter.add_pattern("*.png");
        filter.add_pattern("*.svg");
        filter.add_pattern("*.ico");
        let list = gio::ListStore::new::<gtk::FileFilter>();
        list.append(&filter);
        dlg.set_filters(Some(&list));
        let ui = Rc::clone(self);
        dlg.open(Some(self.window()), None::<&gio::Cancellable>, move |res| {
            if let Ok(f) = res {
                if let Some(p) = f.path() {
                    ui.set_file("icon", &p);
                }
            }
        });
    }

    /// 选中的前缀目录缺 system.reg / drive_c 时视为空前缀，弹窗自动初始化：
    /// umu 模式跑 `umu-run createprefix`，Wine 模式跑 `wine wineboot -i`。
    fn maybe_init_prefix(self: &Rc<Self>, path: &str) {
        let p = Path::new(path);
        if !p.exists() {
            if let Err(e) = std::fs::create_dir_all(p) {
                self.toast_msg(&format!("无法创建目录 {path}：{e}"));
                return;
            }
        }
        let ready = p.join("system.reg").is_file() && p.join("drive_c").is_dir();
        if ready {
            return;
        }
        dialogs::open_prefix_init(self, PathBuf::from(path));
    }

    fn set_file(self: &Rc<Self>, key: &str, p: &std::path::Path) {
        let s = p.to_string_lossy().to_string();
        {
            let mut f = self.files.borrow_mut();
            match key {
                "exe" => f.exe = s.clone(),
                "prefix" => f.prefix = s.clone(),
                "proton" => f.proton = s.clone(),
                "wine" => f.wine = s.clone(),
                "workdir" => f.workdir = s.clone(),
                "icon" => f.icon = s.clone(),
                _ => {}
            }
        }
        match key {
            "exe" => {
                self.row_exe.set_subtitle(&s);
                self.auto_icon();
            }
            "prefix" => {
                self.row_prefix.set_subtitle(&s);
                self.maybe_init_prefix(&s);
            }
            "proton" => self.row_proton.set_subtitle(&s),
            "wine" => {
                let resolved = system::resolve_wine(&s);
                if resolved.is_empty() {
                    self.row_wine.set_subtitle(&format!("{s} —— 不是可执行的 wine"));
                } else {
                    self.files.borrow_mut().wine = resolved.clone();
                    self.row_wine.set_subtitle(&resolved);
                }
            }
            "workdir" => self.row_workdir.set_subtitle(&s),
            "icon" => self.row_icon.set_subtitle(&s),
            _ => {}
        }
        self.refresh();
    }

    fn auto_icon(&self) {
        let exe = self.files.borrow().exe.clone();
        if exe.is_empty() || !self.files.borrow().icon.is_empty() {
            return;
        }
        let name = self.r_name.text();
        let stem = std::path::Path::new(&exe)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "app".into());
        let slug = desktop::slugify(if name.trim().is_empty() { stem.as_str() } else { &name });
        match icons::extract_to_cache(std::path::Path::new(&exe), &slug) {
            Ok((path, (w, h), metas)) => {
                let s = path.to_string_lossy().to_string();
                self.files.borrow_mut().icon = s.clone();
                let mut sizes: Vec<u32> = metas.iter().map(|m| m.w.max(m.h)).collect();
                sizes.sort_unstable();
                sizes.dedup();
                let bpp = metas.iter().max_by_key(|m| m.w).map(|m| m.bpp).unwrap_or(0);
                let sizes_str = sizes.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("/");
                self.row_icon.set_subtitle(&format!(
                    "{w}×{h} · {bpp} 位色 · 源档 {sizes_str}（来自 exe）"
                ));
            }
            Err(_) => {
                // 没有图标资源很常见，不打扰用户
            }
        }
    }

    // ------------------------------------------------------------ 环境变量

    pub fn add_env_row(self: &Rc<Self>, key: &str, value: &str) {
        let row = adw::ActionRow::new();
        let e_key = gtk::Entry::builder()
            .text(key)
            .placeholder_text("KEY")
            .width_chars(12)
            .valign(gtk::Align::Center)
            .build();
        let e_val = gtk::Entry::builder()
            .text(value)
            .placeholder_text("VALUE")
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        let b_del = gtk::Button::from_icon_name("user-trash-symbolic");
        b_del.add_css_class("flat");
        b_del.set_valign(gtk::Align::Center);

        let boxed = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        boxed.append(&e_key);
        boxed.append(&e_val);
        boxed.append(&b_del);
        row.set_child(Some(&boxed));

        self.row_env.add_row(&row);
        self.env_rows.borrow_mut().push(EnvRow {
            row: row.clone(),
            key: e_key.clone(),
            value: e_val.clone(),
        });

        {
            let ui = Rc::clone(self);
            let target = row.clone();
            b_del.connect_clicked(move |_| ui.remove_env_row(&target));
        }
        {
            let ui = Rc::clone(self);
            e_key.connect_changed(move |_| ui.refresh());
        }
        {
            let ui = Rc::clone(self);
            e_val.connect_changed(move |_| ui.refresh());
        }
        self.row_env.set_expanded(true);
    }

    /// 按控件本身定位要删的行 —— 用下标会在删过一次之后错位，删到别人身上。
    fn remove_env_row(self: &Rc<Self>, target: &adw::ActionRow) {
        let pos = self.env_rows.borrow().iter().position(|r| &r.row == target);
        let Some(pos) = pos else { return };
        let removed = self.env_rows.borrow_mut().remove(pos);
        self.row_env.remove(&removed.row);
        self.refresh();
    }

    pub fn collect_env(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for r in self.env_rows.borrow().iter() {
            let k = r.key.text().to_string();
            let k = k.trim();
            if k.is_empty() {
                continue;
            }
            if matches!(k, "WINEPREFIX" | "GAMEID" | "PROTONPATH") {
                continue;
            }
            out.push((k.to_string(), r.value.text().to_string()));
        }
        out
    }

    fn update_env_count(&self) {
        let n = self.collect_env().len();
        self.row_env.set_subtitle(&format!("{n} 个 · 会写进 Exec 的 env 段"));
    }

    // ------------------------------------------------------------ GAMEID

    fn sync_gameid(&self) {
        let name = self.r_name.text();
        let name = if name.trim().is_empty() { "未命名" } else { name.trim() };
        *self.setting_gid.borrow_mut() = true;
        self.r_gameid.set_text(&desktop::auto_gameid(name));
        *self.setting_gid.borrow_mut() = false;
    }

    // ------------------------------------------------------------ 表单 → 模型

    pub fn form(&self) -> Form {
        let umu = self.umu_mode();
        let name = self.r_name.text().to_string();
        let mut gid = self.r_gameid.text().to_string();
        if umu && gid.trim().is_empty() {
            gid = desktop::auto_gameid(if name.trim().is_empty() { "未命名" } else { &name });
        }
        let f = self.files.borrow();
        Form {
            name,
            exe: f.exe.clone(),
            args: desktop::split_quoted(&self.r_args.text()),
            prefix: f.prefix.clone(),
            proton: f.proton.clone(),
            wine: f.wine.clone(),
            umu_run: if umu { self.r_umu.text().to_string() } else { String::new() },
            gameid: if umu { gid } else { String::new() },
            wmclass: self.r_wmclass.text().trim().to_string(),
            workdir: f.workdir.clone(),
            icon: f.icon.clone(),
            env: self.collect_env(),
            umu_mode: umu,
            slug: self.editing.borrow().clone(),
        }
    }

    // ------------------------------------------------------------ 刷新

    pub fn refresh(self: &Rc<Self>) {
        let umu = self.umu_mode();
        self.row_proton.set_visible(umu);
        self.r_umu.set_visible(umu);
        self.row_wine.set_visible(!umu);
        self.r_gameid.set_sensitive(umu);
        self.update_env_count();

        let form = self.form();
        let (wm, _why) = desktop::derive_wmclass(&form);
        self.buf.set_text(&desktop::desktop_text(&form, &wm));

        let slug = form.slug();
        self.lbl_path.set_text(&format!(
            "{}/{slug}.desktop",
            desktop::apps_dir().display()
        ));
        self.lbl_name.set_text(if form.name.trim().is_empty() { "未命名" } else { form.name.trim() });
        self.lbl_sub
            .set_text(if umu { "通过 umu + Proton 启动" } else { "直接用 Wine 启动" });

        self.update_picture(&form.icon);

        let mut notes: Vec<String> = Vec::new();
        if umu && self.r_umu.text().trim().is_empty() {
            notes.push("还没填 umu-run 路径，高级里可以自动查找".into());
        }
        if form.exe.is_empty() {
            notes.push("先选一个 .exe".into());
        } else if form.icon.is_empty() {
            notes.push("这个程序没有可用图标".into());
        }
        if !form.wmclass.is_empty() && (form.wmclass == "steam_proton" || form.wmclass == "steam_app_0") {
            notes.push("这个窗口标识是所有 Proton 应用共用的，任务栏会把它们并成一个".into());
        }
        if umu {
            let gid = self.r_gameid.text().to_string();
            if !gid.trim().is_empty() && !desktop::is_umu_gameid(gid.trim()) {
                notes.push(format!(
                    "GAMEID 必须以 umu- 开头（{gid} 会让 umu 把 SteamAppId 设成 0）"
                ));
            }
        }
        self.lbl_warn.set_text(&notes.join("\n"));
        self.lbl_warn.set_visible(!notes.is_empty());

        self.save_prefs_deferred();
    }

    fn update_picture(&self, path: &str) {
        if !path.is_empty() && std::path::Path::new(path).exists() {
            self.pic.set_from_file(Some(path));
            self.pic.set_pixel_size(96);
        } else {
            self.pic.set_icon_name(Some("application-x-executable"));
            self.pic.set_pixel_size(84);
        }
    }

    // ------------------------------------------------------------ 生成

    fn generate(self: &Rc<Self>, install: bool) {
        let form = self.form();
        let (wm, _) = desktop::derive_wmclass(&form);
        let slug = form.slug();
        let was_edit = self.editing.borrow().is_some();

        let mut note = String::new();
        if install && !form.icon.is_empty() {
            let src = std::path::Path::new(&form.icon);
            if src.exists() {
                desktop::install_icon(src, &slug);
                note = " + 图标（7 档）".into();
            }
        }

        match desktop::write_entry(&form, &wm) {
            Ok(_) => {
                if install && self.r_install_icons.is_active() {
                    desktop::refresh();
                    note.push_str("，缓存已刷新");
                }
                self.toast_msg(&format!(
                    "{} {}.desktop{note}",
                    if was_edit { "已更新" } else { "已写入" },
                    slug
                ));
                self.reload_manager();
            }
            Err(e) => self.toast_msg(&format!("写入失败：{e}")),
        }
    }

    // ------------------------------------------------------------ 已生成

    pub fn reload_manager(self: &Rc<Self>) {
        // PreferencesGroup 的直接子节点是内部 ListBox，靠 first_child 遍历删不掉我们加的行；
        // 自己记着加过哪些，重载时逐条摘掉，否则每刷新一次就多一份。
        let old: Vec<adw::PreferencesRow> = self.mrows.borrow_mut().drain(..).collect();
        for row in old {
            self.mgroup.remove(&row);
        }

        let entries = desktop::list_generated();
        if entries.is_empty() {
            let row = adw::ActionRow::builder()
                .title("还没有生成任何桌面项")
                .subtitle("左边填完点「生成并安装」")
                .build();
            self.mgroup.add(&row);
            self.mrows.borrow_mut().push(row.upcast());
            return;
        }

        for e in entries {
            let row = adw::ExpanderRow::builder()
                .title(e.name())
                .subtitle(&format!(
                    "{} · {}.desktop",
                    if e.get("Exec").contains("umu") { "umu" } else { "wine" },
                    e.slug
                ))
                .build();

            let thumb_path = desktop::icons_root()
                .join("256x256/apps")
                .join(format!("{}.png", e.slug));
            let thumb = if thumb_path.exists() {
                let img = gtk::Image::from_file(&thumb_path);
                img.set_pixel_size(20);
                img.set_valign(gtk::Align::Center);
                img
            } else {
                let img = gtk::Image::from_icon_name("application-x-executable-symbolic");
                img.set_pixel_size(20);
                img.set_valign(gtk::Align::Center);
                img
            };
            row.add_prefix(&thumb);

            let b_edit = gtk::Button::from_icon_name("document-edit-symbolic");
            b_edit.add_css_class("flat");
            b_edit.set_valign(gtk::Align::Center);
            b_edit.set_tooltip_text(Some("编辑"));
            let ui_edit = Rc::clone(self);
            let entry_edit = e.clone();
            b_edit.connect_clicked(move |_| ui_edit.load_for_edit(&entry_edit));
            let b_del = gtk::Button::from_icon_name("user-trash-symbolic");
            b_del.add_css_class("flat");
            b_del.set_valign(gtk::Align::Center);
            b_del.set_tooltip_text(Some("删除"));
            let ui_del = Rc::clone(self);
            let entry_del = e.clone();
            b_del.connect_clicked(move |_| ui_del.ask_delete(&entry_del));
            row.add_suffix(&b_edit);
            row.add_suffix(&b_del);

            for (label, value) in [
                ("Exec", e.get("Exec").to_string()),
                ("Icon", e.get("Icon").to_string()),
                ("窗口标识", e.get("StartupWMClass").to_string()),
                ("文件", e.path().to_string_lossy().to_string()),
            ] {
                let r = adw::ActionRow::builder().title(label).subtitle(&value).build();
                r.set_subtitle_lines(2);
                row.add_row(&r);
            }

            self.mgroup.add(&row);
            self.mrows.borrow_mut().push(row.upcast());
        }
    }

    fn ask_delete(self: &Rc<Self>, entry: &desktop::Entry) {
        let dlg = adw::AlertDialog::builder()
            .heading("删除这个桌面项？")
            .body(format!("{}\n{}", entry.name(), entry.path().display()))
            .build();
        dlg.add_response("cancel", "取消");
        dlg.add_response("delete", "删除");
        dlg.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dlg.set_default_response(Some("cancel"));
        dlg.set_close_response("cancel");

        let ui = Rc::clone(self);
        let entry = entry.clone();
        dlg.connect_response(None, move |_, resp| {
            if resp != "delete" {
                return;
            }
            let gone = desktop::remove_generated(&entry);
            ui.toast_msg(&format!("已删除 {} 个文件", gone.len()));
            ui.reload_manager();
        });
        dlg.present(Some(&self.window));
    }

    fn new_entry(self: &Rc<Self>) {
        *self.editing.borrow_mut() = None;
        *self.gid_manual.borrow_mut() = false;
        self.r_name.set_text("");
        self.r_args.set_text("");
        self.r_wmclass.set_text("");
        self.r_gameid.set_text("");
        {
            let mut f = self.files.borrow_mut();
            f.exe.clear();
            f.icon.clear();
            f.workdir.clear();
        }
        self.row_exe.set_subtitle("未选择");
        self.row_icon.set_subtitle("不选就用 exe 里的图标");
        self.row_workdir.set_subtitle("默认 exe 所在目录");
        {
            let mut rows = self.env_rows.borrow_mut();
            for r in rows.drain(..) {
                self.row_env.remove(&r.row);
            }
        }
        self.sync_gameid();
        self.stack.set_visible_child_name("gen");
        self.refresh();
        self.toast_msg("已清空表单，这是一条新条目");
    }

    fn refresh_preset_popover(self: &Rc<Self>) {
        self.b_preset.set_popover(Some(&build_preset_popover(self)));
    }

    fn load_for_edit(self: &Rc<Self>, entry: &desktop::Entry) {
        let form = entry.to_form();
        *self.editing.borrow_mut() = Some(entry.slug.clone());

        *self.setting_gid.borrow_mut() = true;
        self.r_name.set_text(&form.name);
        self.r_gameid.set_text(&form.gameid);
        *self.setting_gid.borrow_mut() = false;
        *self.gid_manual.borrow_mut() = true;

        self.r_mode.set_selected(if form.umu_mode { 0 } else { 1 });
        if form.umu_mode {
            self.r_umu.set_text(&form.umu_run);
        }
        self.r_wmclass.set_text(&form.wmclass);
        self.r_args.set_text(&form.args.join(" "));

        {
            let mut f = self.files.borrow_mut();
            f.exe = form.exe.clone();
            f.prefix = form.prefix.clone();
            f.proton = form.proton.clone();
            f.workdir = form.workdir.clone();
            f.icon = form.icon.clone();
            if !form.umu_mode {
                f.wine = form.wine.clone();
            }
        }
        self.row_exe.set_subtitle(if form.exe.is_empty() { "未选择" } else { &form.exe });
        self.row_prefix
            .set_subtitle(if form.prefix.is_empty() { "未选择" } else { &form.prefix });
        self.row_proton
            .set_subtitle(if form.proton.is_empty() { "未选择" } else { &form.proton });
        self.row_workdir
            .set_subtitle(if form.workdir.is_empty() { "默认 exe 所在目录" } else { &form.workdir });
        self.row_icon
            .set_subtitle(if form.icon.is_empty() { "不选就用 exe 里的图标" } else { &form.icon });
        if !form.umu_mode && !form.wine.is_empty() {
            self.row_wine.set_subtitle(&form.wine);
        }

        {
            let mut rows = self.env_rows.borrow_mut();
            for r in rows.drain(..) {
                self.row_env.remove(&r.row);
            }
        }
        for (k, v) in &form.env {
            self.add_env_row(k, v);
        }

        self.stack.set_visible_child_name("gen");
        self.refresh();
        self.toast_msg(&format!("已载入「{}」，改完点「生成并安装」覆盖", form.name));
    }

    // ------------------------------------------------------------ 杂项

    pub fn apply_build_result(self: &Rc<Self>, kind: system::Kind, path: &Path) {
        match kind {
            system::Kind::Umu => {
                self.r_umu.set_text(&path.to_string_lossy());
            }
            system::Kind::Wine => {
                self.r_mode.set_selected(1);
                self.set_file("wine", path);
            }
        }
        self.refresh();
    }

    pub fn push_preset(self: &Rc<Self>, key: &str, value: &str) {
        {
            let mut presets = self.presets.borrow_mut();
            if let Some(p) = presets.iter_mut().find(|p| p.key == key) {
                p.value = value.to_string();
            } else {
                presets.push(Preset {
                    key: key.to_string(),
                    value: value.to_string(),
                    hint: String::new(),
                });
            }
        }
        self.save_prefs();
        self.refresh_preset_popover();
        self.toast_msg(&format!("预设已保存：{key}={value}"));
    }

    pub fn set_wmclass(self: &Rc<Self>, cls: &str) {
        self.r_wmclass.set_text(cls);
        self.refresh();
        self.toast_msg(&format!("窗口标识已实测：{cls}"));
    }

    fn save_prefs(&self) {
        let mut s = desktop::load_settings();
        let f = self.files.borrow();
        if !f.prefix.is_empty() {
            s.prefix = f.prefix.clone();
        }
        if !f.proton.is_empty() {
            s.proton = f.proton.clone();
        }
        if !f.workdir.is_empty() {
            s.workdir = f.workdir.clone();
        }
        if !f.wine.is_empty() {
            s.wine = f.wine.clone();
        }
        let umu = self.r_umu.text().to_string();
        if !umu.trim().is_empty() {
            s.umu = umu;
        }
        s.env_presets = self.presets.borrow().clone();
        desktop::save_settings(&s);
    }

    fn save_prefs_deferred(self: &Rc<Self>) {
        let ui = Rc::clone(self);
        glib::timeout_add_seconds_local_once(1, move || ui.save_prefs());
    }

    pub fn toast_msg(&self, msg: &str) {
        self.toast.add_toast(adw::Toast::new(msg));
    }

    fn add_preset(self: &Rc<Self>, key: &str, value: &str) {
        let mut found = false;
        {
            let rows = self.env_rows.borrow_mut();
            for r in rows.iter() {
                if r.key.text().trim() == key {
                    r.value.set_text(value);
                    found = true;
                }
            }
        }
        if !found {
            self.add_env_row(key, value);
        }
        self.refresh();
    }
}

fn clone_refresh<W: IsA<gtk::Widget>>(ui: &Rc<Ui>) -> impl Fn(&W) + 'static {
    let ui = Rc::clone(ui);
    move |_| ui.refresh()
}


fn build_preset_popover(ui: &Rc<Ui>) -> gtk::Popover {
    let pop = gtk::Popover::new();
    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    list.set_margin_top(6);
    list.set_margin_bottom(6);
    list.set_margin_start(6);
    list.set_margin_end(6);

    for p in ui.presets.borrow().iter() {
        let b = gtk::Button::with_label(&format!("{}={}", p.key, p.value));
        b.add_css_class("flat");
        b.set_halign(gtk::Align::Start);
        if !p.hint.is_empty() {
            b.set_tooltip_text(Some(&p.hint));
        }
        let ui2 = Rc::clone(ui);
        let key = p.key.clone();
        let value = p.value.clone();
        let pop2 = pop.clone();
        b.connect_clicked(move |_| {
            pop2.popdown();
            ui2.add_preset(&key, &value);
        });
        list.append(&b);
    }

    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    list.append(&sep);
    let b_custom = gtk::Button::with_label("添加预设…");
    b_custom.add_css_class("flat");
    b_custom.set_halign(gtk::Align::Start);
    let ui3 = Rc::clone(ui);
    let pop3 = pop.clone();
    b_custom.connect_clicked(move |_| {
        pop3.popdown();
        dialogs::open_preset_dialog(&ui3);
    });
    list.append(&b_custom);

    pop.set_child(Some(&list));
    pop
}
