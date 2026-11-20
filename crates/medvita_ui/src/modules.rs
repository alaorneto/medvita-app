//! Modules of the workspace (sidebar entries). A business module (patients, schedule…) implements
//! [`Module`] and is added to [`registry`]; it shows up when the server reports its app as active
//! for the organization (`apps/active/`), or always while the server does not report it.

use std::sync::Arc;

use gpui_kit::component::{ActiveTheme, IconName, v_flex};
use gpui_kit::*;
use medvita_api::{ActiveApp, ApiClient, Me, Organization};
use rust_i18n::t;

/// What a module view gets when it is built.
#[derive(Clone)]
pub struct ModuleContext {
    pub api: Arc<ApiClient>,
    pub me: Me,
    pub organization: Organization,
}

pub trait Module: 'static {
    fn id(&self) -> &'static str;
    fn title(&self) -> SharedString;
    fn icon(&self) -> IconName;
    /// Label of the server app the module needs (`med_patients`…); `None`: always available.
    fn app_label(&self) -> Option<&'static str>;
    fn build_view(&self, context: &ModuleContext, window: &mut Window, cx: &mut App) -> AnyView;
}

/// Every module, in sidebar order.
pub fn registry() -> Vec<Box<dyn Module>> {
    vec![
        Box::new(HomeModule),
        Box::new(PlaceholderModule::new(
            "patients",
            "nav.patients",
            IconName::User,
            "med_patients",
        )),
        Box::new(PlaceholderModule::new(
            "schedule",
            "nav.schedule",
            IconName::Calendar,
            "med_scheduling",
        )),
        Box::new(PlaceholderModule::new(
            "records",
            "nav.records",
            IconName::FileText,
            "med_records",
        )),
        Box::new(PlaceholderModule::new(
            "finance",
            "nav.finance",
            IconName::ChartPie,
            "med_finance",
        )),
    ]
}

/// Whether a module needing `app_label` is shown, given what the server reports.
pub fn is_available(app_label: Option<&str>, active_apps: Option<&[ActiveApp]>) -> bool {
    match (app_label, active_apps) {
        (None, _) | (Some(_), None) => true,
        (Some(label), Some(apps)) => apps.iter().any(|app| app.label == label),
    }
}

pub struct HomeModule;

impl Module for HomeModule {
    fn id(&self) -> &'static str {
        "home"
    }
    fn title(&self) -> SharedString {
        t!("nav.home").to_string().into()
    }
    fn icon(&self) -> IconName {
        IconName::LayoutDashboard
    }
    fn app_label(&self) -> Option<&'static str> {
        None
    }
    fn build_view(&self, context: &ModuleContext, _: &mut Window, cx: &mut App) -> AnyView {
        let view = HomeView {
            name: context.me.display_name().into(),
            organization: context.organization.organization_name.clone().into(),
        };
        cx.new(|_| view).into()
    }
}

struct HomeView {
    name: SharedString,
    organization: SharedString,
}

impl Render for HomeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_8()
            .gap_3()
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("home.greeting", name = self.name.as_ref()).to_string()),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("home.organization", name = self.organization.as_ref()).to_string()),
            )
            .child(
                div()
                    .max_w(px(640.))
                    .child(t!("home.description").to_string()),
            )
    }
}

/// Sidebar entry of a module that is not built yet.
pub struct PlaceholderModule {
    id: &'static str,
    title_key: &'static str,
    icon: IconName,
    app_label: &'static str,
}

impl PlaceholderModule {
    pub fn new(
        id: &'static str,
        title_key: &'static str,
        icon: IconName,
        app_label: &'static str,
    ) -> Self {
        Self {
            id,
            title_key,
            icon,
            app_label,
        }
    }
}

impl Module for PlaceholderModule {
    fn id(&self) -> &'static str {
        self.id
    }
    fn title(&self) -> SharedString {
        t!(self.title_key).to_string().into()
    }
    fn icon(&self) -> IconName {
        self.icon.clone()
    }
    fn app_label(&self) -> Option<&'static str> {
        Some(self.app_label)
    }
    fn build_view(&self, _: &ModuleContext, _: &mut Window, cx: &mut App) -> AnyView {
        let title = self.title();
        cx.new(|_| PlaceholderView { title }).into()
    }
}

struct PlaceholderView {
    title: SharedString,
}

impl Render for PlaceholderView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("module.placeholder_title").to_string()),
            )
            .child(
                div()
                    .max_w(px(480.))
                    .text_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("module.placeholder", module = self.title.as_ref()).to_string()),
            )
    }
}

#[cfg(test)]
mod tests {
    // Explicit imports: `gpui_kit::*` (in the parent module) carries GPUI's `test` macro, which
    // would shadow the built-in `#[test]`.
    use super::{ActiveApp, is_available, registry};

    fn app(label: &str) -> ActiveApp {
        ActiveApp {
            app_id: format!("io.example.{label}"),
            label: label.into(),
            version: "0.1.0".into(),
            title: String::new(),
        }
    }

    #[test]
    fn modules_follow_the_active_apps_when_the_server_reports_them() {
        let apps = vec![app("med_patients")];
        assert!(is_available(None, Some(&apps)));
        assert!(is_available(Some("med_patients"), Some(&apps)));
        assert!(!is_available(Some("med_finance"), Some(&apps)));
        // Older servers do not report active apps: show everything.
        assert!(is_available(Some("med_finance"), None));
    }

    #[test]
    fn registry_ids_are_unique_and_home_comes_first() {
        let modules = registry();
        assert_eq!(modules[0].id(), "home");
        let mut ids: Vec<_> = modules.iter().map(|m| m.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), modules.len());
    }
}
