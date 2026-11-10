mod engine;
mod ui;

use adw::prelude::*;
use gtk::glib;

const APP_ID: &str = "io.github.carboy.MkDesktop";

fn main() -> glib::ExitCode {
    // XWayland 下 WM_CLASS 取自 prgname，不设就是二进制名 "carboy"，
    // 跟 desktop 文件名（app id）对不上，Shell 只能当成另一个应用
    glib::set_prgname(Some(APP_ID));

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        if let Some(win) = app.active_window() {
            win.present();
            return;
        }
        gtk::Window::set_default_icon_name(APP_ID);
        let ui = ui::build(app);
        ui.window.present();
    });

    app.run()
}
