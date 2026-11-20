//! Theme: "Sistema" (default) follows Windows light/dark; "Claro"/"Escuro" are fixed.
//! GPUI Kit only knows Light and Dark and starts in Light, so "Sistema" is ours: read the window
//! appearance when the window opens and again whenever it changes (`observe_window_appearance`).

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Window};
use medvita_platform::ThemePreference;

pub fn apply(preference: ThemePreference, window: &mut Window, cx: &mut App) {
    match preference {
        ThemePreference::System => Theme::sync_system_appearance(Some(window), cx),
        ThemePreference::Light => Theme::change(ThemeMode::Light, Some(window), cx),
        ThemePreference::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
    }
    window.refresh();
}
