//! 对话框：构建（umu / wine）、窗口标识抓取、自定义预设。

use std::cell::RefCell;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::engine::system;
use crate::ui::window::Ui;

enum Msg {
    Line(String),
    Done(PathBuf),
    Fail(String),
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn open_window_dialog(ui: &Rc<Ui>, title: &str, width: i32, height: i32) -> (adw::Window, adw::HeaderBar) {
    let win = adw::Window::builder()
        .transient_for(&ui.window)
        .modal(true)
        .title(title)
        .default_width(width)
        .default_height(height)
        .build();
    let header = adw::HeaderBar::new();
    (win, header)
}

// ---------------------------------------------------------------- 构建

pub fn open_build(ui: &Rc<Ui>, kind: system::Kind) {
    let (title, subtitle) = match kind {
        system::Kind::Umu => ("构建 umu-launcher", "按官方 README：configure.sh → make → make install"),
        system::Kind::Wine => ("编译 Wine", "在源码目录的 build/ 里 configure + make"),
    };
    let (win, header) = open_window_dialog(ui, title, 780, 640);

    let spinner = gtk::Spinner::new();
    header.pack_start(&spinner);

    let stack = gtk::Stack::new();

    // ---- 依赖页
    let deps = system::dep_status(kind);
    let missing: Vec<String> = deps
        .iter()
        .filter(|d| d.required && !d.found)
        .map(|d| d.name.clone())
        .collect();

    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("依赖检查")
        .description(if missing.is_empty() {
            subtitle
        } else {
            "缺的项必须补齐才能构建"
        })
        .build();

    for d in &deps {
        let mark = if d.found { "✓" } else if d.required { "⚠" } else { "·" };
        let row = adw::ActionRow::builder()
            .title(format!("{mark}  {}", d.name))
            .subtitle(&d.note)
            .build();
        if !d.found && !d.required {
            row.add_css_class("dim-label");
        }
        group.add(&row);
    }
    page.add(&group);

    let hint = gtk::Label::builder()
        .label(if missing.is_empty() {
            String::new()
        } else {
            system::install_hint(&missing)
        })
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .build();
    hint.add_css_class("monospace");
    hint.set_margin_top(10);
    hint.set_margin_bottom(10);
    hint.set_margin_start(16);
    hint.set_margin_end(16);
    let g_hint = adw::PreferencesGroup::new();
    g_hint.add(&hint);
    page.add(&g_hint);

    // 32 位开关（只有 wine 用得上）
    let r_i386 = adw::SwitchRow::builder()
        .title("同时编译 32 位（i386）")
        .subtitle("只跑 64 位游戏可以关掉，能省一半依赖")
        .active(false)
        .build();
    if kind == system::Kind::Wine {
        let g2 = adw::PreferencesGroup::new();
        g2.add(&r_i386);
        page.add(&g2);
    }

    let deps_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&page)
        .build();

    // ---- 日志页
    let buf = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
    let tv = gtk::TextView::builder()
        .buffer(&buf)
        .monospace(true)
        .editable(false)
        .top_margin(8)
        .left_margin(10)
        .build();
    let sc = gtk::ScrolledWindow::builder().vexpand(true).child(&tv).build();
    let log_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    log_page.append(&sc);

    stack.add_named(&deps_scroll, Some("deps"));
    stack.add_named(&log_page, Some("log"));

    // ---- 底部按钮
    let b_cancel = gtk::Button::with_label("关闭");
    let b_start = gtk::Button::with_label("选择源码目录并构建");
    b_start.add_css_class("suggested-action");
    b_start.set_sensitive(missing.is_empty());

    let b_prebuilt = gtk::Button::with_label("装预编译版");
    b_prebuilt.set_visible(kind == system::Kind::Umu);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.set_margin_top(10);
    footer.set_margin_bottom(10);
    footer.set_margin_start(12);
    footer.set_margin_end(12);
    footer.append(&b_prebuilt);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    footer.append(&spacer);
    footer.append(&b_cancel);
    footer.append(&b_start);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&header);
    body.append(&stack);
    body.append(&footer);
    win.set_content(Some(&body));

    b_cancel.connect_clicked({
        let win = win.clone();
        move |_| win.close()
    });

    // 预编译安装：从官方 GitHub Releases 下载 zipapp，免 root 装到 ~/.local/bin。
    // 注意 umu-launcher 并不在 PyPI 上，uv/pipx 那条路是走不通的。
    b_prebuilt.connect_clicked({
        let buf = buf.clone();
        let sc = sc.clone();
        let ui = Rc::clone(ui);
        let stack = stack.clone();
        let spinner = spinner.clone();
        let win2 = win.clone();
        move |_| {
            let cmd = r#"exec 2>&1
set -e
VER=$(curl -sIL -o /dev/null -w '%{url_effective}' https://github.com/Open-Wine-Components/umu-launcher/releases/latest | grep -oE '[0-9]+\.[0-9]+\.[0-9]+$' || true)
if [ -z "$VER" ]; then
    echo "✗ 拿不到最新版本号，检查网络（github.com 可达性）"
    exit 1
fi
DEST="$HOME/.local/share/umu-launcher/zipapp-$VER"
mkdir -p "$DEST"
echo "==> 下载 umu-launcher $VER zipapp"
curl -fL --retry 2 -o "$DEST/zipapp.tar" \
    "https://github.com/Open-Wine-Components/umu-launcher/releases/download/$VER/umu-launcher-$VER-zipapp.tar"
tar xf "$DEST/zipapp.tar" -C "$DEST"
if [ ! -f "$DEST/umu/umu-run" ]; then
    echo "✗ 包结构变了（没有 umu/umu-run），看看 release 里有什么"
    ls -R "$DEST"
    exit 1
fi
install -Dm755 "$DEST/umu/umu-run" "$HOME/.local/bin/umu-run"
echo "==> 已装到 $HOME/.local/bin/umu-run（版本 $VER）"
"#
            .to_string();
            start_log(&stack, &spinner);
            let (tx, rx) = channel();
            run_shell(cmd, PathBuf::from("/"), tx);
            pump(rx, buf.clone(), sc.clone(), spinner.clone(), stack.clone(), {
                let ui = Rc::clone(&ui);
                let win = win2.clone();
                move |path: PathBuf, ok: bool| {
                    if ok {
                        // zipapp 装在固定位置，装完再查一次实际路径
                        let resolved = if path.as_os_str().is_empty() {
                            system::find_umu().map(PathBuf::from).unwrap_or_default()
                        } else {
                            path
                        };
                        if resolved.as_os_str().is_empty() {
                            ui.toast_msg("装完了但没找到 umu-run，手动选一下");
                            return;
                        }
                        ui.apply_build_result(system::Kind::Umu, &resolved);
                        ui.toast_msg("umu-launcher 已安装");
                        win.close();
                    } else {
                        ui.toast_msg("安装失败，看日志");
                    }
                }
            });
        }
    });

    b_start.connect_clicked({
        let ui = Rc::clone(ui);
        let stack = stack.clone();
        let spinner = spinner.clone();
        let win2 = win.clone();
        let r_i386 = r_i386.clone();
        let buf = buf.clone();
        let sc = sc.clone();
        move |_| {
            let ui2 = Rc::clone(&ui);
            let win3 = win2.clone();
            let dlg = gtk::FileDialog::new();
            dlg.set_title("选择源码目录");
            let kind = kind;
            let stack = stack.clone();
            let spinner = spinner.clone();
            let r_i386 = r_i386.clone();
            let buf = buf.clone();
            let sc = sc.clone();
            let parent = win3.clone();
            dlg.select_folder(Some(&parent), None::<&gio::Cancellable>, move |res| {
                let Ok(f) = res else { return };
                let Some(src) = f.path() else { return };
                let ok = match kind {
                    system::Kind::Umu => system::is_umu_source(&src),
                    system::Kind::Wine => system::is_wine_source(&src),
                };
                if !ok {
                    ui2.toast_msg("这个目录不像源码树（缺 configure / dlls 之类）");
                    return;
                }
                let steps = match kind {
                    system::Kind::Umu => system::plan_umu(&src),
                    system::Kind::Wine => system::plan_wine(&src, r_i386.is_active()),
                };
                // wine 要先建 build 目录
                if kind == system::Kind::Wine {
                    let _ = std::fs::create_dir_all(src.join("build"));
                }

                start_log(&stack, &spinner);
                let (tx, rx) = channel();
                let src2 = src.clone();
                std::thread::spawn(move || {
                    for step in steps {
                        if let Err(e) = run_step(&step, &tx) {
                            let _ = tx.send(Msg::Fail(e));
                            return;
                        }
                    }
                    let _ = tx.send(Msg::Done(system::plan_result(kind, &src2)));
                });

                let ui3 = Rc::clone(&ui2);
                let win4 = win3.clone();
                pump(rx, buf, sc, spinner, stack, move |path: PathBuf, ok: bool| {
                    if ok {
                        ui3.apply_build_result(kind, &path);
                        ui3.toast_msg(&format!("构建完成：{}", path.display()));
                        win4.close();
                    } else {
                        ui3.toast_msg("构建失败，看日志");
                    }
                });
            });
        }
    });

    win.present();
}

// ---------------------------------------------------------------- 前缀初始化

/// 选中的前缀目录是空的（缺 system.reg / drive_c）时自动初始化：
/// umu 模式跑 `umu-run createprefix`（上游 umu_run.py 官方支持的建前缀用法），
/// Wine 模式跑 `wine wineboot -i`。
pub fn open_prefix_init(ui: &Rc<Ui>, dir: PathBuf) {
    let (win, header) = open_window_dialog(ui, "初始化 Wine 前缀", 640, 420);
    let spinner = gtk::Spinner::new();
    header.pack_start(&spinner);

    let buf = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
    let tv = gtk::TextView::builder()
        .buffer(&buf)
        .monospace(true)
        .editable(false)
        .top_margin(8)
        .left_margin(10)
        .build();
    let sc = gtk::ScrolledWindow::builder().vexpand(true).child(&tv).build();

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&header);
    body.append(&sc);
    win.set_content(Some(&body));

    let dq = dir.to_string_lossy().to_string();
    let umu_without_proton = ui.umu_mode() && ui.files.borrow().proton.is_empty();
    let cmd = if ui.umu_mode() {
        let runner_raw = ui.r_umu.text().trim().to_string();
        let runner = if runner_raw.is_empty() { "umu-run".to_string() } else { runner_raw };
        let mut gid = ui.r_gameid.text().trim().to_string();
        if !gid.starts_with("umu-") {
            gid = "umu-0".to_string();
        }
        let mut env = format!("WINEPREFIX={} GAMEID={}", shell_quote(&dq), shell_quote(&gid));
        let proton = ui.files.borrow().proton.clone();
        if !proton.is_empty() {
            env.push_str(&format!(" PROTONPATH={}", shell_quote(&proton)));
        }
        format!("env {env} {} createprefix 2>&1", shell_quote(&runner))
    } else {
        let wine = {
            let f = ui.files.borrow();
            let w = f.wine.clone();
            if w.is_empty() { system::find_wine() } else { Some(w) }
        };
        let Some(wine) = wine else {
            let mut end = buf.end_iter();
            buf.insert(&mut end, "没找到 wine 可执行文件，先选一个 wine 再重新选前缀目录。\n");
            win.present();
            return;
        };
        format!(
            "env WINEPREFIX={} {} wineboot -i 2>&1",
            shell_quote(&dq),
            shell_quote(&wine)
        )
    };

    let start = {
        let ui = Rc::clone(ui);
        let win = win.clone();
        let buf = buf.clone();
        let sc = sc.clone();
        let spinner = spinner.clone();
        move |cmd: String, dir: PathBuf| {
            run_prefix_init(&ui, &win, &buf, &sc, &spinner, cmd, dir);
        }
    };

    // umu 模式且没选 Proton 时，umu 会自己下载默认 UMU-Proton（几百 MB），
    // 先问一句，避免"点一下选目录就静默开始下载"。
    if umu_without_proton {
        let dlg = adw::MessageDialog::new(
            Some(&win),
            Some("没选 Proton 版本，umu 会自动下载"),
            Some(
                "没有指定 PROTONPATH，umu 会按默认行为下载 UMU-Proton（几百 MB，之前下载过就直接复用）。\n\
                 也可以取消，先在「Proton 版本」里选一个已有的。",
            ),
        );
        dlg.add_response("cancel", "取消");
        dlg.add_response("continue", "继续下载并初始化");
        dlg.set_response_appearance("continue", adw::ResponseAppearance::Suggested);
        dlg.connect_response(None, {
            let win = win.clone();
            move |_, resp| {
                if resp == "continue" {
                    start(cmd.clone(), dir.clone());
                } else {
                    win.close();
                }
            }
        });
        dlg.present();
        win.present();
        return;
    }

    start(cmd, dir);
    win.present();
}

fn run_prefix_init(
    ui: &Rc<Ui>,
    win: &adw::Window,
    buf: &gtk::TextBuffer,
    sc: &gtk::ScrolledWindow,
    spinner: &gtk::Spinner,
    cmd: String,
    dir: PathBuf,
) {
    {
        let mut end = buf.end_iter();
        buf.insert(&mut end, &format!("$ {cmd}\n"));
    }
    spinner.start();

    let (tx, rx) = channel();
    run_shell(cmd, dir.clone(), tx);

    pump(rx, buf.clone(), sc.clone(), spinner.clone(), gtk::Stack::new(), {
        let ui = Rc::clone(ui);
        let win = win.clone();
        let buf = buf.clone();
        let dir = dir.clone();
        move |_: PathBuf, ok: bool| {
            if ok {
                ui.toast_msg("前缀初始化完成");
                win.close();
                return;
            }
            // dwproton 等定制 Proton 在 createprefix 收尾时会以非零状态退出，
            // 但前缀实际已经建好；以 system.reg / drive_c 是否存在为准。
            let p = PathBuf::from(&dir);
            if p.join("system.reg").is_file() && p.join("drive_c").is_dir() {
                let mut end = buf.end_iter();
                buf.insert(
                    &mut end,
                    "\n命令以非零状态退出，但 system.reg / drive_c 已就位，前缀创建成功。\n",
                );
                ui.toast_msg("前缀已创建（命令退出码非零，可忽略）");
                win.close();
            } else {
                ui.toast_msg("前缀初始化失败，看日志");
            }
        }
    });
}

fn start_log(stack: &gtk::Stack, spinner: &gtk::Spinner) {
    stack.set_visible_child_name("log");
    spinner.start();
}

fn run_shell(cmd: String, cwd: PathBuf, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        let res = Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(&cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        match res {
            Ok(mut child) => {
                if let Some(out) = child.stdout.take() {
                    for line in BufReader::new(out).lines() {
                        let Ok(l) = line else { break };
                        let _ = tx.send(Msg::Line(system::strip_ansi(&l)));
                    }
                }
                match child.wait() {
                    Ok(s) if s.success() => {
                        let _ = tx.send(Msg::Done(PathBuf::new()));
                    }
                    Ok(_) => {
                        let _ = tx.send(Msg::Fail("命令失败".into()));
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Fail(e.to_string()));
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(Msg::Fail(e.to_string()));
            }
        }
    });
}

fn run_step(step: &system::Step, tx: &Sender<Msg>) -> Result<(), String> {
    let mut cmd = format!("{}", step.program);
    for a in &step.args {
        cmd.push(' ');
        cmd.push_str(&shell_quote(a));
    }
    cmd.push_str(" 2>&1");
    let _ = tx.send(Msg::Line(format!("$ {cmd}")));

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .current_dir(&step.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;

    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines() {
            let Ok(l) = line else { break };
            let _ = tx.send(Msg::Line(system::strip_ansi(&l)));
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} 失败（退出码 {}）", step.label, status.code().unwrap_or(-1)))
    }
}

fn pump<F>(
    rx: Receiver<Msg>,
    buf: gtk::TextBuffer,
    sc: gtk::ScrolledWindow,
    spinner: gtk::Spinner,
    _stack: gtk::Stack,
    done: F,
) where
    F: Fn(PathBuf, bool) + 'static,
{
    let done = Rc::new(RefCell::new(Some(done)));
    glib::timeout_add_local(Duration::from_millis(80), move || {
        let mut finished: Option<(PathBuf, bool)> = None;
        loop {
            match rx.try_recv() {
                Ok(Msg::Line(l)) => {
                    let mut end = buf.end_iter();
                    buf.insert(&mut end, &format!("{l}\n"));
                    let n = buf.line_count();
                    if n > 3000 {
                        let mut start = buf.start_iter();
                        let mut cut = buf
                            .iter_at_line(n - 3000)
                            .unwrap_or_else(|| buf.end_iter());
                        buf.delete(&mut start, &mut cut);
                    }
                    let adj = sc.vadjustment();
                    adj.set_value(adj.upper() - adj.page_size());
                }
                Ok(Msg::Done(p)) => {
                    finished = Some((p, true));
                    break;
                }
                Ok(Msg::Fail(e)) => {
                    let mut end = buf.end_iter();
                    buf.insert(&mut end, &format!("\n✗ {e}\n"));
                    finished = Some((PathBuf::new(), false));
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finished = Some((PathBuf::new(), false));
                    break;
                }
            }
        }
        if let Some((path, ok)) = finished {
            spinner.stop();
            if let Some(cb) = done.borrow_mut().take() {
                cb(path, ok);
            }
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
}

// ---------------------------------------------------------------- 预设

pub fn open_preset_dialog(ui: &Rc<Ui>) {
    let (win, header) = open_window_dialog(ui, "添加预设", 460, 320);
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("环境变量预设")
        .description("会出现在「从预设添加」下拉里")
        .build();
    let r_key = adw::EntryRow::builder().title("KEY").build();
    let r_val = adw::EntryRow::builder().title("VALUE").build();
    group.add(&r_key);
    group.add(&r_val);
    page.add(&group);

    let b_save = gtk::Button::with_label("保存");
    b_save.add_css_class("suggested-action");
    header.pack_end(&b_save);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&header);
    body.append(&page);
    win.set_content(Some(&body));

    b_save.connect_clicked({
        let ui = Rc::clone(ui);
        let win = win.clone();
        let r_key = r_key.clone();
        let r_val = r_val.clone();
        move |_| {
            let k = r_key.text().trim().to_string();
            let v = r_val.text().to_string();
            if k.is_empty() {
                ui.toast_msg("KEY 不能为空");
                return;
            }
            ui.push_preset(&k, &v);
            win.close();
        }
    });

    win.present();
}

// ---------------------------------------------------------------- 窗口标识

pub fn open_window_pick(ui: &Rc<Ui>) {
    let wins = system::list_x11_windows();
    let (win, header) = open_window_dialog(ui, "选择窗口", 640, 480);

    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("正在运行的窗口")
        .description("点「使用」把它的窗口标识写进这条启动项")
        .build();

    if wins.is_empty() {
        group.add(
            &adw::ActionRow::builder()
                .title("没抓到窗口")
                .subtitle("先把游戏跑起来再点这里；原生 Wayland 窗口读不到")
                .build(),
        );
    }
    for w in wins {
        let row = adw::ActionRow::builder()
            .title(if w.title.is_empty() { "(无标题)" } else { &w.title })
            .subtitle(if w.wm_class.is_empty() {
                format!("没有 WM_CLASS · {}", w.id)
            } else {
                format!("{} · {}", w.wm_class, w.id)
            })
            .build();
        let b = gtk::Button::with_label("使用");
        b.add_css_class("flat");
        b.set_valign(gtk::Align::Center);
        b.set_sensitive(!w.wm_class.is_empty());
        let ui2 = Rc::clone(ui);
        let win2 = win.clone();
        let cls = w.wm_class.clone();
        b.connect_clicked(move |_| {
            ui2.set_wmclass(&cls);
            win2.close();
        });
        row.add_suffix(&b);
        group.add(&row);
    }
    page.add(&group);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&header);
    body.append(&page);
    win.set_content(Some(&body));
    win.present();
}
