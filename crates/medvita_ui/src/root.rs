//! The root view: title bar, the screen of the current phase, status bar, and the overlays
//! (settings, mandatory update). It owns the connection to the server, the session and the updater.

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{ActiveTheme, IconName, Sizable, TitleBar, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use medvita_api::{AccessTokenProvider, ApiClient, ApiError, Me, Organization};
use medvita_auth::{AuthError, LOGIN_TIMEOUT, OidcClient, OidcEndpoints, OidcSession};
use medvita_platform::build_info::{self, CLIENT_ID};
use medvita_platform::{
    AppPaths, SecretStore, ServerUrlError, Settings, SettingsStore, ThemePreference, UpdateChannel,
    parse_server_url,
};
use medvita_updater::service::{CHECK_INTERVAL, FIRST_CHECK_DELAY, PendingUpdate};
use medvita_updater::{UpdateService, UpdaterConfig, UpdaterStatus};
use rust_i18n::t;
use url::Url;
use uuid::Uuid;

use crate::assets::LOGO;
use crate::bridge;
use crate::messages;
use crate::session::{Event, Notice, Phase, SessionController};
use crate::theme;
use crate::workspace::Workspace;

/// What the binary hands to the UI.
pub struct Boot {
    pub paths: AppPaths,
    pub secrets: Arc<dyn SecretStore>,
}

enum Connection {
    Oidc {
        session: Arc<OidcSession>,
        api: Arc<ApiClient>,
    },
    #[cfg(debug_assertions)]
    Personal { api: Arc<ApiClient> },
}

impl Connection {
    fn api(&self) -> Arc<ApiClient> {
        match self {
            Self::Oidc { api, .. } => api.clone(),
            #[cfg(debug_assertions)]
            Self::Personal { api } => api.clone(),
        }
    }
}

struct OidcContext {
    client: OidcClient,
    endpoints: OidcEndpoints,
}

struct ConnectOutcome {
    client: OidcClient,
    discovered: Result<OidcEndpoints, AuthError>,
    session: Option<OidcSession>,
    #[cfg(debug_assertions)]
    personal: Option<medvita_api::StaticTokenProvider>,
}

fn http_client() -> reqwest::Client {
    ApiClient::http_client_builder()
        .build()
        .expect("HTTP client with the default settings")
}

fn api_for(server: &Url, provider: Arc<dyn AccessTokenProvider>) -> Option<Arc<ApiClient>> {
    match ApiClient::new(server.clone(), provider) {
        Ok(api) => Some(Arc::new(api)),
        Err(error) => {
            tracing::error!(%error, "API client unavailable");
            None
        }
    }
}

async fn discover_and_restore(
    http: reqwest::Client,
    server: Url,
    secrets: Arc<dyn SecretStore>,
) -> ConnectOutcome {
    let client = OidcClient::new(http, server.clone(), CLIENT_ID, secrets.clone());
    let discovered = client.discover().await;
    let session = match &discovered {
        Ok(endpoints) => client.restore(endpoints).unwrap_or_else(|error| {
            tracing::warn!(%error, "stored session unreadable");
            None
        }),
        Err(error) => {
            tracing::warn!(%error, "OIDC discovery failed");
            None
        }
    };
    #[cfg(debug_assertions)]
    let personal = medvita_auth::personal::restore(&secrets, &server)
        .ok()
        .flatten();
    #[cfg(not(debug_assertions))]
    let _ = (&secrets, &server);
    ConnectOutcome {
        client,
        discovered,
        session,
        #[cfg(debug_assertions)]
        personal,
    }
}

pub struct MedvitaRoot {
    paths: AppPaths,
    settings_store: SettingsStore,
    settings: Settings,
    secrets: Arc<dyn SecretStore>,
    server: Option<Url>,
    controller: SessionController,
    oidc: Option<Arc<OidcContext>>,
    discovery_error: Option<String>,
    connection: Option<Connection>,
    me: Option<Me>,
    workspace: Option<Entity<Workspace>>,
    authorize_url: Option<String>,
    server_input: Entity<InputState>,
    server_error: Option<String>,
    #[cfg(debug_assertions)]
    token_input: Entity<InputState>,
    #[cfg(debug_assertions)]
    token_error: Option<String>,
    show_settings: bool,
    updater: Option<Arc<UpdateService>>,
    update_status: UpdaterStatus,
    pending_update: Option<Arc<PendingUpdate>>,
    update_required: bool,
    checking: bool,
    /// The connect / sign-in / profile task; replacing or dropping it cancels the previous one.
    task: Option<Task<()>>,
    update_loop: Option<Task<()>>,
    manual_check: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl MedvitaRoot {
    pub fn new(boot: Boot, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings_store = SettingsStore::new(boot.paths.settings_file());
        let settings = settings_store.load();
        let server = settings.server_url.clone().or_else(|| {
            build_info::default_server_url().and_then(|url| parse_server_url(url).ok())
        });

        let initial = server.as_ref().map(Url::to_string).unwrap_or_default();
        let server_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("server.placeholder").to_string())
                .default_value(initial)
        });
        #[cfg(debug_assertions)]
        let token_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("login.dev_placeholder").to_string())
                .masked(true)
        });

        let subscriptions = vec![
            cx.subscribe_in(
                &server_input,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        this.submit_server(window, cx);
                    }
                },
            ),
            cx.observe_window_appearance(window, |this, window, cx| {
                if this.settings.theme == ThemePreference::System {
                    theme::apply(ThemePreference::System, window, cx);
                }
            }),
        ];

        let mut root = Self {
            paths: boot.paths,
            settings_store,
            settings,
            secrets: boot.secrets,
            server: None,
            controller: SessionController::new(),
            oidc: None,
            discovery_error: None,
            connection: None,
            me: None,
            workspace: None,
            authorize_url: None,
            server_input,
            server_error: None,
            #[cfg(debug_assertions)]
            token_input,
            #[cfg(debug_assertions)]
            token_error: None,
            show_settings: false,
            updater: None,
            update_status: UpdaterStatus::Idle,
            pending_update: None,
            update_required: false,
            checking: false,
            task: None,
            update_loop: None,
            manual_check: None,
            _subscriptions: subscriptions,
        };
        theme::apply(root.settings.theme, window, cx);
        match server {
            Some(url) => root.connect(url, window, cx),
            None => root.controller.apply(Event::ServerMissing),
        }
        root.start_updater(false, window, cx);
        root
    }

    fn save_settings(&self) {
        if let Err(error) = self.settings_store.save(&self.settings) {
            tracing::error!(%error, "could not save settings");
        }
    }

    fn server_host(&self) -> Option<String> {
        self.server
            .as_ref()
            .map(|url| url.host_str().unwrap_or_default().to_owned())
    }

    // ----- server and session -------------------------------------------------------------------

    fn connect(&mut self, server: Url, window: &mut Window, cx: &mut Context<Self>) {
        self.server = Some(server.clone());
        self.controller.apply(Event::Connecting);
        self.oidc = None;
        self.discovery_error = None;
        self.connection = None;
        self.me = None;
        self.workspace = None;
        let handle = bridge::handle(cx);
        let secrets = self.secrets.clone();
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let outcome = bridge::bridged(
                &handle,
                discover_and_restore(http_client(), server, secrets),
            )
            .await;
            let _ = this.update_in(cx, |this, window, cx| this.connected(outcome, window, cx));
        }));
        cx.notify();
    }

    fn connected(
        &mut self,
        outcome: anyhow::Result<ConnectOutcome>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(server) = self.server.clone() else {
            return;
        };
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                tracing::error!(%error, "connection task failed");
                self.controller.apply(Event::NoSession);
                cx.notify();
                return;
            }
        };
        match outcome.discovered {
            Ok(endpoints) => {
                self.oidc = Some(Arc::new(OidcContext {
                    client: outcome.client,
                    endpoints,
                }))
            }
            Err(error) => self.discovery_error = Some(messages::auth_error(&error)),
        }
        if let Some(session) = outcome.session {
            let session = Arc::new(session);
            if let Some(api) = api_for(&server, session.clone()) {
                self.connection = Some(Connection::Oidc { session, api });
            }
        }
        #[cfg(debug_assertions)]
        if self.connection.is_none()
            && let Some(provider) = outcome.personal
            && let Some(api) = api_for(&server, Arc::new(provider))
        {
            self.connection = Some(Connection::Personal { api });
        }
        if self.connection.is_some() {
            self.controller.apply(Event::SessionFound);
            self.load_profile(window, cx);
        } else {
            self.controller.apply(Event::NoSession);
        }
        cx.notify();
    }

    fn submit_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.server_input.read(cx).value().to_string();
        match parse_server_url(&value) {
            Ok(url) => {
                self.server_error = None;
                self.show_settings = false;
                if self.server.as_ref() != Some(&url)
                    || matches!(self.controller.phase(), Phase::NeedsServer)
                {
                    self.end_session(cx);
                    self.settings.server_url = Some(url.clone());
                    self.save_settings();
                    self.connect(url, window, cx);
                    self.start_updater(false, window, cx);
                }
            }
            Err(ServerUrlError::InsecureScheme) => {
                self.server_error = Some(t!("server.insecure").to_string())
            }
            Err(_) => self.server_error = Some(t!("server.invalid").to_string()),
        }
        cx.notify();
    }

    fn sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(context) = self.oidc.clone() else {
            let message = self
                .discovery_error
                .clone()
                .unwrap_or_else(|| t!("errors.no_sign_in").to_string());
            self.controller.apply(Event::SignInFailed(message));
            cx.notify();
            return;
        };
        self.controller.apply(Event::SignInStarted);
        self.authorize_url = None;
        let handle = bridge::handle(cx);
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let begin_context = context.clone();
            let begun = bridge::bridged(&handle, async move {
                begin_context
                    .client
                    .begin_login(&begin_context.endpoints)
                    .await
            })
            .await;
            let attempt = match begun {
                Ok(Ok(attempt)) => attempt,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.sign_in_failed(messages::auth_error(&error), cx)
                    });
                    return;
                }
                Err(error) => {
                    let _ = this.update(cx, |this, cx| this.sign_in_failed(error.to_string(), cx));
                    return;
                }
            };
            let url = attempt.authorize_url().to_string();
            let opened = this.update(cx, |this, cx| {
                this.authorize_url = Some(url.clone());
                cx.open_url(&url);
                cx.notify();
            });
            if opened.is_err() {
                return;
            }
            let completed = bridge::bridged(&handle, attempt.complete(LOGIN_TIMEOUT)).await;
            let _ = this.update_in(cx, |this, window, cx| match completed {
                Ok(Ok(session)) => this.signed_in(Arc::new(session), window, cx),
                Ok(Err(error)) => this.sign_in_failed(messages::auth_error(&error), cx),
                Err(error) => this.sign_in_failed(error.to_string(), cx),
            });
        }));
        cx.notify();
    }

    fn sign_in_failed(&mut self, message: String, cx: &mut Context<Self>) {
        tracing::warn!("sign-in failed");
        self.authorize_url = None;
        self.controller.apply(Event::SignInFailed(message));
        cx.notify();
    }

    fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        // Dropping the task aborts the tokio task and closes the loopback listener.
        self.task = None;
        self.authorize_url = None;
        self.controller.apply(Event::SignInCancelled);
        cx.notify();
    }

    fn signed_in(
        &mut self,
        session: Arc<OidcSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(server) = self.server.clone() else {
            return;
        };
        self.authorize_url = None;
        let Some(api) = api_for(&server, session.clone()) else {
            return;
        };
        self.connection = Some(Connection::Oidc { session, api });
        self.controller.apply(Event::SignedIn);
        self.load_profile(window, cx);
        cx.notify();
    }

    #[cfg(debug_assertions)]
    fn sign_in_with_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use secrecy::SecretString;
        let Some(server) = self.server.clone() else {
            return;
        };
        let value = self.token_input.read(cx).value().to_string();
        if !medvita_auth::personal::looks_like_personal_token(&value) {
            self.token_error = Some(t!("login.dev_invalid").to_string());
            cx.notify();
            return;
        }
        self.token_error = None;
        self.token_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        match medvita_auth::personal::save(&self.secrets, &server, &SecretString::from(value)) {
            Ok(provider) => {
                if let Some(api) = api_for(&server, Arc::new(provider)) {
                    self.connection = Some(Connection::Personal { api });
                    self.controller.apply(Event::SignedIn);
                    self.load_profile(window, cx);
                }
            }
            Err(error) => self.token_error = Some(messages::auth_error(&error)),
        }
        cx.notify();
    }

    fn load_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(api) = self.connection.as_ref().map(Connection::api) else {
            return;
        };
        let handle = bridge::handle(cx);
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = bridge::bridged(&handle, async move {
                let me = api.me().await?;
                let organizations = api.organizations().await?;
                Ok::<_, ApiError>((me.value, organizations))
            })
            .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.profile_loaded(result, window, cx)
            });
        }));
    }

    fn profile_loaded(
        &mut self,
        result: anyhow::Result<Result<(Me, Vec<Organization>), ApiError>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(Ok((me, organizations))) => {
                self.me = Some(me);
                let last = self
                    .server_host()
                    .and_then(|host| self.settings.last_organization.get(&host).copied());
                self.controller.apply(Event::ProfileLoaded {
                    organizations,
                    last,
                });
                self.phase_changed(window, cx);
            }
            Ok(Err(error)) if error.needs_sign_in() => {
                self.end_session(cx);
                self.controller.apply(Event::SessionExpired);
            }
            Ok(Err(error)) => self
                .controller
                .apply(Event::ProfileFailed(messages::api_error(&error))),
            Err(error) => self
                .controller
                .apply(Event::ProfileFailed(error.to_string())),
        }
        cx.notify();
    }

    /// Opens the workspace of the chosen organization and remembers the choice.
    fn phase_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Phase::Ready { organization } = self.controller.phase().clone() else {
            return;
        };
        let current = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.read(cx).organization().organization_id);
        if current == Some(organization.organization_id) {
            return;
        }
        if let Some(host) = self.server_host() {
            self.settings
                .last_organization
                .insert(host, organization.organization_id);
            self.save_settings();
        }
        let (Some(api), Some(me)) = (
            self.connection.as_ref().map(Connection::api),
            self.me.clone(),
        ) else {
            return;
        };
        self.workspace = Some(cx.new(|cx| Workspace::new(api, me, organization, window, cx)));
    }

    fn choose_organization(
        &mut self,
        organization: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.controller
            .apply(Event::OrganizationChosen(organization));
        self.phase_changed(window, cx);
        cx.notify();
    }

    /// Forgets the session of this run; revokes and clears the vault in the background.
    fn end_session(&mut self, cx: &mut Context<Self>) {
        self.task = None;
        self.workspace = None;
        self.me = None;
        match self.connection.take() {
            Some(Connection::Oidc { session, .. }) => {
                bridge::handle(cx).spawn(async move { session.logout().await });
            }
            #[cfg(debug_assertions)]
            Some(Connection::Personal { .. }) => {
                if let Some(server) = &self.server {
                    let _ = medvita_auth::personal::forget(&self.secrets, server);
                }
            }
            None => {}
        }
    }

    fn sign_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_session(cx);
        self.show_settings = false;
        self.controller.apply(Event::SignedOut);
        window.push_notification(t!("user.signed_out").to_string(), cx);
        cx.notify();
    }

    // ----- settings ------------------------------------------------------------------------------

    fn set_theme(
        &mut self,
        preference: ThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.theme = preference;
        self.save_settings();
        theme::apply(preference, window, cx);
        cx.notify();
    }

    fn set_channel(&mut self, channel: UpdateChannel, window: &mut Window, cx: &mut Context<Self>) {
        let downgrade =
            self.settings.update_channel == UpdateChannel::Beta && channel == UpdateChannel::Stable;
        self.settings.update_channel = channel;
        self.save_settings();
        self.start_updater(downgrade, window, cx);
        cx.notify();
    }

    // ----- updates -------------------------------------------------------------------------------

    fn start_updater(
        &mut self,
        allow_downgrade: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_loop = None;
        self.manual_check = None;
        self.pending_update = None;
        self.update_required = false;
        self.checking = false;
        let Some(server) = self.server.clone() else {
            self.updater = None;
            self.update_status = UpdaterStatus::Disabled(t!("status.no_server").to_string());
            return;
        };
        let service = Arc::new(UpdateService::new(UpdaterConfig {
            server,
            channel: self.settings.update_channel,
            state_dir: self.paths.state_dir(),
            runtime: bridge::handle(cx),
            allow_downgrade,
        }));
        self.update_status = service.initial_status();
        let enabled = service.is_enabled();
        self.updater = Some(service);
        if !enabled {
            return;
        }
        self.update_loop = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(FIRST_CHECK_DELAY).await;
            loop {
                let Ok(check) =
                    this.update_in(cx, |this, window, cx| this.check_updates(window, cx))
                else {
                    return;
                };
                if let Some(check) = check {
                    check.await;
                }
                cx.background_executor().timer(CHECK_INTERVAL).await;
            }
        }));
    }

    fn check_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Task<()>> {
        let service = self.updater.clone()?;
        if !service.is_enabled() || self.checking || self.pending_update.is_some() {
            return None;
        }
        self.checking = true;
        self.update_status = UpdaterStatus::Checking;
        cx.notify();
        let handle = bridge::handle(cx);
        let (progress_sender, mut progress) =
            tokio::sync::watch::channel::<Option<(String, u8)>>(None);
        Some(cx.spawn_in(window, async move |this, cx| {
            let policy_service = service.clone();
            let policy =
                bridge::bridged(&handle, async move { policy_service.policy().await }).await;
            let current = service.current_version();
            if let Ok(Ok(Some(policy))) = &policy
                && policy.requires_update(&current)
            {
                let _ = this.update(cx, |this, cx| {
                    this.update_required = true;
                    cx.notify();
                });
            }

            let progress_view = this.clone();
            let _progress = cx.spawn(async move |cx| {
                while progress.changed().await.is_ok() {
                    let value = progress.borrow().clone();
                    if let Some((version, percent)) = value {
                        let _ = progress_view.update(cx, |this, cx| {
                            this.update_status = UpdaterStatus::Downloading { version, percent };
                            cx.notify();
                        });
                    }
                }
            });

            let download_service = service.clone();
            let result = bridge::bridged_blocking(&handle, move || {
                download_service.check_and_download(|version, percent| {
                    progress_sender.send_replace(Some((version.to_owned(), percent)));
                })
            })
            .await;

            let _ = this.update_in(cx, |this, window, cx| {
                this.checking = false;
                this.update_status = match result {
                    Ok(Ok(Some(pending))) => {
                        let version = pending.version.clone();
                        this.pending_update = Some(Arc::new(pending));
                        window.push_notification(
                            Notification::success(
                                t!("update.ready_notification", version = version.as_str())
                                    .to_string(),
                            ),
                            cx,
                        );
                        UpdaterStatus::Ready { version }
                    }
                    Ok(Ok(None)) => UpdaterStatus::UpToDate,
                    Ok(Err(error)) => {
                        tracing::warn!(%error, "update check failed");
                        UpdaterStatus::Failed(error.to_string())
                    }
                    Err(error) => UpdaterStatus::Failed(error.to_string()),
                };
                cx.notify();
            });
        }))
    }

    fn check_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.manual_check = self.check_updates(window, cx);
    }

    fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        let (Some(service), Some(pending)) = (self.updater.clone(), self.pending_update.clone())
        else {
            return;
        };
        bridge::handle(cx).spawn_blocking(move || {
            if let Err(error) = service.apply_and_restart(&pending) {
                tracing::error!(%error, "could not apply the update");
            }
        });
    }

    fn update_status_text(&self) -> String {
        match &self.update_status {
            UpdaterStatus::Disabled(reason) => {
                t!("update.disabled", reason = reason.as_str()).to_string()
            }
            UpdaterStatus::Idle => t!("update.idle").to_string(),
            UpdaterStatus::Checking => t!("update.checking").to_string(),
            UpdaterStatus::UpToDate => t!("update.up_to_date").to_string(),
            UpdaterStatus::Downloading { version, percent } => t!(
                "update.downloading",
                version = version.as_str(),
                percent = percent.to_string()
            )
            .to_string(),
            UpdaterStatus::Ready { version } => {
                t!("update.ready", version = version.as_str()).to_string()
            }
            UpdaterStatus::Failed(reason) => {
                t!("update.failed", reason = reason.as_str()).to_string()
            }
        }
    }
}

// ----- rendering ---------------------------------------------------------------------------------

fn card() -> Div {
    v_flex().w(px(460.)).max_w_full().p_6().gap_4()
}

fn title(text: impl Into<SharedString>) -> Div {
    div()
        .text_xl()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

impl MedvitaRoot {
    fn render_title_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let organization = match self.controller.phase() {
            Phase::Ready { organization } => Some(organization.organization_name.clone()),
            _ => None,
        };
        let can_switch = self.controller.organizations().len() > 1;
        let user = self.me.as_ref().map(Me::display_name);
        let signed_in = self.controller.is_signed_in();

        TitleBar::new().child(
            h_flex()
                .flex_1()
                .justify_between()
                .items_center()
                .pr_2()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(img(LOGO).size_5())
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t!("app.name").to_string()),
                        )
                        .when_some(organization, |this, name| {
                            this.child(
                                Button::new("organization")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Building2)
                                    .label(name)
                                    .when(can_switch, |button| {
                                        button.tooltip(t!("organization.change").to_string())
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.controller.apply(Event::ChangeOrganization);
                                        cx.notify();
                                    })),
                            )
                        }),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .when_some(user, |this, name| {
                            this.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(name),
                            )
                        })
                        .child(
                            Button::new("settings")
                                .ghost()
                                .small()
                                .icon(IconName::Settings)
                                .tooltip(t!("nav.settings").to_string())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_settings = !this.show_settings;
                                    cx.notify();
                                })),
                        )
                        .when(signed_in, |this| {
                            this.child(
                                Button::new("sign-out")
                                    .ghost()
                                    .small()
                                    .label(t!("user.sign_out").to_string())
                                    .on_click(
                                        cx.listener(|this, _, window, cx| {
                                            this.sign_out(window, cx)
                                        }),
                                    ),
                            )
                        }),
                ),
        )
    }

    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let server = match self.server_host() {
            Some(host) => t!("status.server", host = host.as_str()).to_string(),
            None => t!("status.no_server").to_string(),
        };
        let ready = self.pending_update.is_some();
        StatusBar::new().left(div().text_xs().child(server)).right(
            h_flex()
                .gap_3()
                .items_center()
                .text_xs()
                .child(self.update_status_text())
                .when(ready, |this| {
                    this.child(
                        Button::new("restart-update")
                            .primary()
                            .xsmall()
                            .label(t!("update.restart").to_string())
                            .on_click(cx.listener(|this, _, _, cx| this.restart_to_update(cx))),
                    )
                })
                .child(t!("status.version", version = build_info::version()).to_string()),
        )
    }

    fn render_notice(&self, cx: &mut Context<Self>) -> Option<Div> {
        let text = match self.controller.notice()? {
            Notice::SessionExpired => t!("login.session_expired").to_string(),
            Notice::NoActiveOrganization => t!("organization.none").to_string(),
            Notice::Message(message) => message.clone(),
        };
        Some(div().text_sm().text_color(cx.theme().danger).child(text))
    }

    fn render_server(&mut self, cx: &mut Context<Self>) -> Div {
        card()
            .child(title(t!("server.title").to_string()))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("server.description").to_string()),
            )
            .child(Input::new(&self.server_input))
            .when_some(self.server_error.clone(), |this, error| {
                this.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
            .child(
                Button::new("server-continue")
                    .primary()
                    .label(t!("common.continue").to_string())
                    .on_click(cx.listener(|this, _, window, cx| this.submit_server(window, cx))),
            )
    }

    fn render_signed_out(&mut self, cx: &mut Context<Self>) -> Div {
        let host = self.server_host().unwrap_or_default();
        let mut body = card()
            .child(img(LOGO).size_12())
            .child(title(t!("login.title").to_string()))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("login.description").to_string()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("login.server", host = host.as_str()).to_string()),
            )
            .children(self.render_notice(cx))
            .child(
                Button::new("sign-in")
                    .primary()
                    .icon(IconName::ExternalLink)
                    .label(t!("login.button").to_string())
                    .on_click(cx.listener(|this, _, window, cx| this.sign_in(window, cx))),
            )
            .child(
                Button::new("change-server")
                    .link()
                    .label(t!("server.change").to_string())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.controller.apply(Event::ServerMissing);
                        cx.notify();
                    })),
            );
        #[cfg(debug_assertions)]
        {
            body = body.child(
                v_flex()
                    .gap_2()
                    .pt_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("login.dev_title").to_string()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("login.dev_description").to_string()),
                    )
                    .child(Input::new(&self.token_input))
                    .when_some(self.token_error.clone(), |this, error| {
                        this.child(div().text_sm().text_color(cx.theme().danger).child(error))
                    })
                    .child(
                        Button::new("sign-in-token")
                            .label(t!("login.dev_button").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.sign_in_with_token(window, cx)
                            })),
                    ),
            );
        }
        body
    }

    fn render_signing_in(&mut self, cx: &mut Context<Self>) -> Div {
        let url = self.authorize_url.clone();
        card()
            .child(title(t!("login.waiting_title").to_string()))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("login.waiting").to_string()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .when_some(url, |this, url| {
                        this.child(
                            Button::new("reopen-browser")
                                .icon(IconName::ExternalLink)
                                .label(t!("login.reopen").to_string())
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                    })
                    .child(
                        Button::new("cancel-sign-in")
                            .ghost()
                            .label(t!("common.cancel").to_string())
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_sign_in(cx))),
                    ),
            )
    }

    fn render_message(&mut self, message: String, retry: bool, cx: &mut Context<Self>) -> Div {
        card()
            .child(div().text_color(cx.theme().muted_foreground).child(message))
            .when(retry, |this| {
                this.child(
                    Button::new("retry")
                        .primary()
                        .label(t!("common.retry").to_string())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.controller.apply(Event::SessionFound);
                            this.load_profile(window, cx);
                            cx.notify();
                        })),
                )
            })
    }

    fn render_organizations(&mut self, cx: &mut Context<Self>) -> Div {
        let organizations = self.controller.organizations().to_vec();
        card()
            .child(title(t!("organization.title").to_string()))
            .when(!organizations.is_empty(), |this| {
                this.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("organization.description").to_string()),
                )
            })
            .children(self.render_notice(cx))
            .children(
                organizations
                    .into_iter()
                    .enumerate()
                    .map(|(index, organization)| {
                        let id = organization.organization_id;
                        Button::new(("organization", index))
                            .icon(IconName::Building2)
                            .label(organization.organization_name)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose_organization(id, window, cx)
                            }))
                    }),
            )
    }

    fn render_settings(&mut self, cx: &mut Context<Self>) -> Div {
        let theme = self.settings.theme;
        let channel = self.settings.update_channel;
        let theme_button =
            |id: &'static str, label: String, value: ThemePreference, cx: &mut Context<Self>| {
                let button = Button::new(id).small().label(label);
                let button = if theme == value {
                    button.primary()
                } else {
                    button
                };
                button.on_click(
                    cx.listener(move |this, _, window, cx| this.set_theme(value, window, cx)),
                )
            };
        let channel_button =
            |id: &'static str, label: String, value: UpdateChannel, cx: &mut Context<Self>| {
                let button = Button::new(id).small().label(label);
                let button = if channel == value {
                    button.primary()
                } else {
                    button
                };
                button.on_click(
                    cx.listener(move |this, _, window, cx| this.set_channel(value, window, cx)),
                )
            };
        let section = |text: String| div().pt_2().font_weight(FontWeight::SEMIBOLD).child(text);

        card()
            .w(px(560.))
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(title(t!("settings.title").to_string()))
                    .child(
                        Button::new("close-settings")
                            .ghost()
                            .small()
                            .label(t!("common.close").to_string())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_settings = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(section(t!("settings.theme").to_string()))
            .child(
                h_flex()
                    .gap_2()
                    .child(theme_button(
                        "theme-system",
                        t!("settings.theme_system").to_string(),
                        ThemePreference::System,
                        cx,
                    ))
                    .child(theme_button(
                        "theme-light",
                        t!("settings.theme_light").to_string(),
                        ThemePreference::Light,
                        cx,
                    ))
                    .child(theme_button(
                        "theme-dark",
                        t!("settings.theme_dark").to_string(),
                        ThemePreference::Dark,
                        cx,
                    )),
            )
            .child(section(t!("settings.channel").to_string()))
            .child(
                h_flex()
                    .gap_2()
                    .child(channel_button(
                        "channel-stable",
                        t!("settings.channel_stable").to_string(),
                        UpdateChannel::Stable,
                        cx,
                    ))
                    .child(channel_button(
                        "channel-beta",
                        t!("settings.channel_beta").to_string(),
                        UpdateChannel::Beta,
                        cx,
                    )),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("settings.channel_hint").to_string()),
            )
            .child(section(t!("settings.server").to_string()))
            .child(Input::new(&self.server_input))
            .when_some(self.server_error.clone(), |this, error| {
                this.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("settings-server")
                            .small()
                            .label(t!("server.change").to_string())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.submit_server(window, cx)),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("settings.server_hint").to_string()),
                    ),
            )
            .child(section(t!("settings.about").to_string()))
            .child(div().child(t!("settings.version", version = build_info::version()).to_string()))
            .when_some(build_info::commit(), |this, commit| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("settings.commit", commit = commit).to_string()),
                )
            })
            .child(div().text_sm().child(t!("settings.publisher").to_string()))
            .child(div().text_sm().child(self.update_status_text()))
            .child(
                Button::new("check-updates")
                    .small()
                    .icon(IconName::RefreshCw)
                    .label(t!("update.check_now").to_string())
                    .on_click(cx.listener(|this, _, window, cx| this.check_now(window, cx))),
            )
    }

    fn render_update_required(&mut self, cx: &mut Context<Self>) -> Div {
        let ready = self.pending_update.is_some();
        card()
            .child(title(t!("update.required_title").to_string()))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("update.required").to_string()),
            )
            .child(div().text_sm().child(self.update_status_text()))
            .when(ready, |this| {
                this.child(
                    Button::new("restart-required")
                        .primary()
                        .label(t!("update.restart").to_string())
                        .on_click(cx.listener(|this, _, _, cx| this.restart_to_update(cx))),
                )
            })
    }

    fn render_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.update_required {
            return centered(self.render_update_required(cx));
        }
        if self.show_settings {
            return centered(self.render_settings(cx));
        }
        match self.controller.phase().clone() {
            Phase::Booting | Phase::LoadingProfile => centered(
                card().child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("common.connecting").to_string()),
                ),
            ),
            Phase::NeedsServer => centered(self.render_server(cx)),
            Phase::SignedOut => centered(self.render_signed_out(cx)),
            Phase::SigningIn => centered(self.render_signing_in(cx)),
            Phase::Unavailable { message } => centered(self.render_message(message, true, cx)),
            Phase::SelectingOrganization => centered(self.render_organizations(cx)),
            Phase::Ready { .. } => match &self.workspace {
                Some(workspace) => workspace.clone().into_any_element(),
                None => centered(card().child(t!("common.loading").to_string())),
            },
        }
    }
}

fn centered(content: Div) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .p_6()
        .child(content)
        .into_any_element()
}

impl Render for MedvitaRoot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(div().flex_1().min_h_0().child(self.render_body(cx)))
            .child(self.render_status_bar(cx))
    }
}
