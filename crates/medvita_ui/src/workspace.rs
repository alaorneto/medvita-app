//! The signed-in workspace: sidebar with the modules and the active module's view.

use std::sync::Arc;

use gpui_kit::component::sidebar::{Sidebar, SidebarGroup, SidebarMenu, SidebarMenuItem};
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::*;
use medvita_api::{ActiveApp, ApiClient, Me, Organization};
use rust_i18n::t;

use crate::bridge;
use crate::modules::{self, Module, ModuleContext};

struct Entry {
    module: Box<dyn Module>,
    view: Option<AnyView>,
}

pub struct Workspace {
    context: ModuleContext,
    entries: Vec<Entry>,
    active_apps: Option<Vec<ActiveApp>>,
    active: usize,
    _load_apps: Task<()>,
}

impl Workspace {
    pub fn new(
        api: Arc<ApiClient>,
        me: Me,
        organization: Organization,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let handle = bridge::handle(cx);
        let organization_id = organization.organization_id;
        let load_api = api.clone();
        let load_apps = cx.spawn_in(window, async move |this, cx| {
            let result = bridge::bridged(&handle, async move {
                load_api.active_apps(organization_id).await
            })
            .await;
            match result {
                Ok(Ok(apps)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.active_apps = apps;
                        cx.notify();
                    });
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "active apps unavailable; showing every module")
                }
                Err(error) => tracing::warn!(%error, "active apps task failed"),
            }
        });
        let mut workspace = Self {
            context: ModuleContext {
                api,
                me,
                organization,
            },
            entries: modules::registry()
                .into_iter()
                .map(|module| Entry { module, view: None })
                .collect(),
            active_apps: None,
            active: 0,
            _load_apps: load_apps,
        };
        workspace.ensure_view(0, window, cx);
        workspace
    }

    pub fn organization(&self) -> &Organization {
        &self.context.organization
    }

    fn visible(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                modules::is_available(entry.module.app_label(), self.active_apps.as_deref())
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn ensure_view(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let entry = &mut self.entries[index];
        if entry.view.is_none() {
            entry.view = Some(entry.module.build_view(&self.context, window, cx));
        }
    }

    fn select(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.active = index;
        self.ensure_view(index, window, cx);
        cx.notify();
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let visible = self.visible();
        if !visible.contains(&self.active) {
            let first = visible.first().copied().unwrap_or(0);
            self.select(first, window, cx);
        }
        let items: Vec<SidebarMenuItem> = visible
            .iter()
            .map(|&index| {
                let module = &self.entries[index].module;
                SidebarMenuItem::new(module.title())
                    .icon(module.icon())
                    .active(index == self.active)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.select(index, window, cx)),
                    )
            })
            .collect();
        let content = self.entries[self.active].view.clone();

        h_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(
                Sidebar::new("medvita-sidebar").w(px(232.)).child(
                    SidebarGroup::new(t!("nav.group").to_string())
                        .child(SidebarMenu::new().children(items)),
                ),
            )
            .child(div().flex_1().min_w_0().h_full().children(content))
    }
}
