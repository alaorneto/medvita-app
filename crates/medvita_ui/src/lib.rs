//! The GPUI Kit interface of Medvita Desktop. Only this crate and the binary depend on GPUI
//! (AGENTS.md). Every visible text comes from `locales/pt-BR.yml` through `t!`.

rust_i18n::i18n!("locales", fallback = "pt-BR");

pub mod assets;
pub mod bridge;
pub mod messages;
pub mod modules;
pub mod root;
pub mod session;
pub mod theme;
pub mod workspace;

use gpui_kit::component::TitleBar;
use gpui_kit::*;
use medvita_platform::build_info;

pub use root::{Boot, MedvitaRoot};

/// The locale of the interface (and of GPUI Kit's own strings).
pub const LOCALE: &str = "pt-BR";

/// Opens the main window and runs the app until it quits. Blocks the calling (main) thread.
pub fn run(boot: Boot, runtime: tokio::runtime::Runtime) {
    rust_i18n::set_locale(LOCALE);
    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::component::set_locale(LOCALE);
            bridge::init(cx, runtime);

            // Closing the last window ends the app.
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1200.), px(780.)), cx)),
                window_min_size: Some(size(px(900.), px(600.))),
                app_id: Some(build_info::APP_ID.to_owned()),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                window.set_window_title(build_info::APP_NAME);
                cx.new(|cx| MedvitaRoot::new(boot, window, cx))
            })
            .expect("could not open the main window");
            cx.activate(true);
        });
}
