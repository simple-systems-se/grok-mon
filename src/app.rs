use crate::accounts::{self, ResolvedIcon};
use crate::auth::{self, AuthError};
use crate::billing::{
    FetchError, IconTarget, IconUsage, UsageSnapshot, fetch_configured_usage, format_percent,
    format_remaining, period_label,
};
use crate::chip::{self, PanelChip, panel_chip, usage_bar};
use crate::config::{self, ACCOUNTS_CONFIG_PATH, APP_ID, Config, USAGE_URL};
use crate::login::{self, LoginPhase, LoginUiState, LoginUpdate};
use crate::pace::maybe_weekly_pace;
use crate::ring::{RingIcon, usage_color, usage_ring};
use crate::sessions::{LiveSessions, live_sessions_at};
use chrono::Utc;
use cosmic::app::Core;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::event::listen_with;
use cosmic::iced::platform_specific::shell::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::window::Id;
use cosmic::iced::{Color, Length, Limits, Size, Subscription};
use cosmic::widget::{self, button, column, container, divider, row, settings, text};
use cosmic::{Element, Task, theme};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::LazyLock;

static PANEL_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("grok-monitor-panel"));

const HISTORY_LEN: usize = 24;
const CONFIG_ERROR_ID: &str = "config-error";
const UNREADABLE_ACCOUNTS: &str =
    "Could not read the accounts list. Fix the accounts file. Icons are not being guessed.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Settings,
}

struct AccountChip {
    index: usize,
    id: String,
    label: String,
    cli_home: PathBuf,
    cli_home_display: String,
    home_error: Option<String>,
    snapshot: Option<UsageSnapshot>,
    error: Option<FetchError>,
    history: VecDeque<f32>,
    login: LoginUiState,
}

impl Default for AccountChip {
    fn default() -> Self {
        Self {
            index: 0,
            id: String::new(),
            label: String::new(),
            cli_home: PathBuf::new(),
            cli_home_display: String::new(),
            home_error: None,
            snapshot: None,
            error: None,
            history: VecDeque::new(),
            login: LoginUiState::default(),
        }
    }
}

pub struct GrokMonitor {
    core: Core,
    popup: Option<Id>,
    config: Config,
    config_handler: Option<cosmic_config::Config>,
    config_error: Option<String>,
    accounts: Vec<AccountChip>,
    jobs: HashMap<String, login::LoginJob>,
    selected: Option<String>,
    live: LiveSessions,
    page: Page,
    size: Size,
    open_error: Option<String>,
    fetching: bool,
    fetch_generation: u64,
    fetch_started: Option<std::time::Instant>,
}

impl Default for GrokMonitor {
    fn default() -> Self {
        Self {
            core: Core::default(),
            popup: None,
            config: Config::default(),
            config_handler: None,
            config_error: None,
            accounts: Vec::new(),
            jobs: HashMap::new(),
            selected: None,
            live: LiveSessions::default(),
            page: Page::Overview,
            size: Size::new(10.0, 10.0),
            open_error: None,
            fetching: false,
            fetch_generation: 0,
            fetch_started: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    Clock,
    LoginPoll,
    TogglePopup(String),
    PopupClosed(Id),
    Size(Size),
    UsageFetched(u64, Vec<IconUsage>),
    OpenUsage,
    CopyPercent,
    CopyText(String),
    SignIn,
    CancelSignIn,
    SignOut,
    LogoutFinished(String, Result<(), String>),
    ShowSettings,
    Back,
    SetPoll(u64),
    ToggleSparkline(bool),
    TogglePercent(bool),
    TogglePace(bool),
    SetRemaining(bool),
    SetLabel(String),
    ConfigChanged,
}

impl cosmic::Application for GrokMonitor {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let config_handler = cosmic_config::Config::new(Self::APP_ID, Config::VERSION).ok();
        let loaded = config_handler.as_ref().map(config::load_from);
        let (config, config_error) = match loaded {
            Some(loaded) => {
                let error = loaded
                    .accounts_unreadable
                    .then(|| UNREADABLE_ACCOUNTS.to_string());
                (loaded.config, error)
            }
            None => (Config::default(), None),
        };
        let mut app = Self {
            core,
            config,
            config_handler,
            config_error,
            ..Self::default()
        };
        app.sync_accounts();
        (app, cosmic::task::message(Message::Tick))
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            listen_with(|event, _status, id| {
                if let cosmic::iced::Event::Window(
                    cosmic::iced::window::Event::Resized(size)
                    | cosmic::iced::window::Event::Opened { position: _, size },
                ) = event
                    && id == cosmic::iced::window::Id::RESERVED
                {
                    Some(Message::Size(size))
                } else {
                    None
                }
            }),
            cosmic::iced::time::every(self.config.poll_duration()).map(|_| Message::Tick),
            self.core
                .watch_config::<Config>(Self::APP_ID)
                .map(|_| Message::ConfigChanged),
        ];
        if self.popup.is_some() {
            subs.push(
                cosmic::iced::time::every(std::time::Duration::from_secs(1))
                    .map(|_| Message::Clock),
            );
        }
        if !self.jobs.is_empty() {
            subs.push(
                cosmic::iced::time::every(std::time::Duration::from_millis(250))
                    .map(|_| Message::LoginPoll),
            );
        }
        Subscription::batch(subs)
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Tick => {
                self.refresh_sessions();
                return self.start_fetch(false);
            }
            Message::Clock => {
                self.refresh_sessions();
            }
            Message::LoginPoll => return self.poll_logins(),
            Message::UsageFetched(generation, results) => {
                if generation == self.fetch_generation {
                    self.fetching = false;
                    self.fetch_started = None;
                    self.apply_usage(results);
                }
            }
            Message::TogglePopup(account_id) => {
                if self.popup.is_some() && self.selected.as_ref() == Some(&account_id) {
                    if let Some(id) = self.popup.take() {
                        self.page = Page::Overview;
                        return destroy_popup(id);
                    }
                } else {
                    self.selected = Some(account_id);
                    self.refresh_sessions();
                    if self.popup.is_none() {
                        return self.open_popup();
                    }
                }
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                    self.page = Page::Overview;
                    self.open_error = None;
                }
            }
            Message::Size(size) => {
                self.size = size;
            }
            Message::OpenUsage => match crate::spawn::spawn_detached("xdg-open", &[USAGE_URL]) {
                Ok(()) => self.open_error = None,
                Err(err) => {
                    tracing::error!("could not open the usage page");
                    self.open_error = Some(err);
                }
            },
            Message::CopyPercent => {
                if let Some(snapshot) = self.selected_chip().and_then(|c| c.snapshot.as_ref()) {
                    return cosmic::iced::clipboard::write(format_percent(
                        self.config.display_percent(snapshot.percent),
                    ));
                }
            }
            Message::CopyText(value) => {
                if !value.is_empty() {
                    return cosmic::iced::clipboard::write(value);
                }
            }
            Message::SignIn => return self.start_sign_in(),
            Message::CancelSignIn => self.cancel_sign_in(),
            Message::SignOut => return self.start_sign_out(),
            Message::LogoutFinished(id, result) => self.finish_sign_out(&id, result),
            Message::ShowSettings => {
                self.page = Page::Settings;
            }
            Message::Back => {
                self.page = Page::Overview;
            }
            Message::SetPoll(secs) => {
                self.config.poll_secs = secs;
                self.save_config();
            }
            Message::ToggleSparkline(value) => {
                self.config.show_sparkline = value;
                self.save_config();
            }
            Message::TogglePercent(value) => {
                self.config.show_percent = value;
                self.save_config();
            }
            Message::TogglePace(value) => {
                self.config.show_pace = value;
                self.save_config();
            }
            Message::SetRemaining(value) => {
                self.config.show_remaining = value;
                self.save_config();
            }
            Message::SetLabel(value) => self.set_selected_label(value),
            Message::ConfigChanged => self.reload_config(),
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let chips: Vec<Element<'_, Message>> = if self.accounts.is_empty() {
            if self.config_error.is_some() {
                vec![self.problem_chip()]
            } else {
                Vec::new()
            }
        } else {
            self.accounts
                .iter()
                .map(|account| self.account_chip(account))
                .collect()
        };
        let data = if self.core.applet.is_horizontal() {
            Element::from(
                row::with_children(chips)
                    .align_y(Vertical::Center)
                    .spacing(8),
            )
        } else {
            Element::from(
                column::with_children(chips)
                    .align_x(Horizontal::Center)
                    .spacing(8),
            )
        };
        widget::autosize::autosize(data, PANEL_ID.clone()).into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let content = if self.accounts.is_empty() {
            self.empty_overview()
        } else {
            match self.page {
                Page::Overview => self.overview(),
                Page::Settings => self.settings_page(),
            }
        };

        self.core
            .applet
            .popup_container(container(content).width(Length::Fixed(360.0)))
            .into()
    }
}

impl GrokMonitor {
    fn save_config(&mut self) {
        if self.config_error.is_some() {
            return;
        }
        if let Some(handler) = &self.config_handler
            && let Err(err) = self.config.write_entry(handler)
        {
            tracing::error!("failed to save config: {err}");
        }
    }

    fn reload_config(&mut self) {
        let Some(handler) = &self.config_handler else {
            return;
        };
        let loaded = config::load_from(handler);
        self.config_error = loaded
            .accounts_unreadable
            .then(|| UNREADABLE_ACCOUNTS.to_string());
        self.config = loaded.config;
        self.sync_accounts();
    }

    fn sync_accounts(&mut self) {
        let user_home = accounts::user_home_dir();
        let resolved = accounts::resolve_icons(&self.config.accounts, user_home.as_deref());
        let previous = std::mem::take(&mut self.accounts);
        let mut previous: HashMap<String, AccountChip> = previous
            .into_iter()
            .map(|chip| (chip.id.clone(), chip))
            .collect();
        self.accounts = resolved
            .into_iter()
            .map(|icon| {
                let mut chip = previous.remove(&icon.id).unwrap_or_default();
                apply_icon(&mut chip, icon);
                chip
            })
            .collect();
        let live: Vec<String> = self.accounts.iter().map(|chip| chip.id.clone()).collect();
        self.jobs
            .retain(|id, _| live.iter().any(|live_id| live_id == id));
        if self
            .selected
            .as_ref()
            .is_none_or(|id| id == CONFIG_ERROR_ID || !live.iter().any(|live_id| live_id == id))
        {
            self.selected = self.accounts.first().map(|chip| chip.id.clone());
        }
    }

    fn selected_chip(&self) -> Option<&AccountChip> {
        self.selected
            .as_ref()
            .and_then(|id| self.accounts.iter().find(|chip| &chip.id == id))
    }

    fn chip_mut(&mut self, id: &str) -> Option<&mut AccountChip> {
        self.accounts.iter_mut().find(|chip| chip.id == id)
    }

    fn refresh_sessions(&mut self) {
        self.live = self
            .selected_chip()
            .filter(|chip| chip.home_error.is_none())
            .map(|chip| live_sessions_at(&chip.cli_home))
            .unwrap_or_default();
    }

    fn start_fetch(&mut self, force: bool) -> Task<cosmic::Action<Message>> {
        if self.fetching && !force {
            if self
                .fetch_started
                .is_some_and(|started| started.elapsed() > std::time::Duration::from_secs(30))
            {
                tracing::warn!("usage fetch still running after 30s; retrying");
                self.fetching = false;
            } else {
                return Task::none();
            }
        }
        let targets: Vec<IconTarget> = self
            .accounts
            .iter()
            .filter(|chip| chip.home_error.is_none())
            .map(|chip| IconTarget {
                id: chip.id.clone(),
                label: chip.label.clone(),
                cli_home: chip.cli_home.clone(),
            })
            .collect();
        if targets.is_empty() {
            self.fetching = false;
            self.fetch_started = None;
            return Task::none();
        }
        self.fetching = true;
        self.fetch_generation = self.fetch_generation.wrapping_add(1);
        self.fetch_started = Some(std::time::Instant::now());
        let generation = self.fetch_generation;
        Task::perform(fetch_configured_usage(targets), move |results| {
            cosmic::action::Action::App(Message::UsageFetched(generation, results))
        })
    }

    fn apply_usage(&mut self, results: Vec<IconUsage>) {
        for item in results {
            let Some(chip) = self.accounts.iter_mut().find(|chip| chip.id == item.id) else {
                continue;
            };
            match item.result {
                Ok(snapshot) => {
                    chip.error = None;
                    if chip.history.len() == HISTORY_LEN {
                        chip.history.pop_front();
                    }
                    chip.history.push_back(snapshot.percent);
                    chip.snapshot = Some(snapshot);
                    if chip.login.phase == LoginPhase::Succeeded {
                        chip.login.phase = LoginPhase::Idle;
                        chip.login.message.clear();
                    }
                }
                Err(err) => {
                    if chip.login.is_in_progress() && matches!(err, FetchError::Auth(_)) {
                        continue;
                    }
                    if matches!(err, FetchError::Auth(_)) {
                        chip.snapshot = None;
                        chip.history.clear();
                        if chip.login.phase == LoginPhase::Succeeded {
                            chip.login.apply(LoginUpdate::Failed(err.to_string()));
                            continue;
                        }
                    }
                    chip.error = Some(err);
                }
            }
        }
    }

    fn poll_logins(&mut self) -> Task<cosmic::Action<Message>> {
        let ids: Vec<String> = self.jobs.keys().cloned().collect();
        let mut fetch = false;
        for id in ids {
            let update = {
                let Some(job) = self.jobs.get_mut(&id) else {
                    continue;
                };
                job.poll()
            };
            if let Some(prompt) = update.prompt
                && let Some(chip) = self.chip_mut(&id)
            {
                chip.login.apply(LoginUpdate::Prompt(prompt));
            }
            let session_ready = self
                .accounts
                .iter()
                .find(|chip| chip.id == id)
                .filter(|chip| chip.home_error.is_none())
                .is_some_and(|chip| auth::load_bearer_from(&chip.cli_home).is_ok());
            if let Some(finish) = update.finished {
                self.jobs.remove(&id);
                if let Some(chip) = self.chip_mut(&id) {
                    if chip.login.phase == LoginPhase::Succeeded || session_ready {
                        if chip.login.phase != LoginPhase::Succeeded && session_ready {
                            chip.login.apply(LoginUpdate::SessionReady);
                            fetch = true;
                        }
                    } else {
                        if !finish.success {
                            tracing::error!("device-code sign-in failed");
                        }
                        chip.login.apply(LoginUpdate::Exited {
                            success: finish.success,
                            message: finish.message,
                        });
                    }
                }
            } else if session_ready
                && let Some(chip) = self.chip_mut(&id)
                && matches!(chip.login.phase, LoginPhase::Starting | LoginPhase::Waiting)
            {
                chip.login.apply(LoginUpdate::SessionReady);
                fetch = true;
            }
        }
        if fetch {
            self.start_fetch(true)
        } else {
            Task::none()
        }
    }

    fn start_sign_in(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(id) = self.selected.clone() else {
            return Task::none();
        };
        let Some(chip) = self.accounts.iter().find(|chip| chip.id == id) else {
            return Task::none();
        };
        if chip.home_error.is_some() || chip.login.is_in_progress() {
            return Task::none();
        }
        let home = chip.cli_home.clone();
        match login::spawn_device_login(&home) {
            Ok(job) => {
                self.jobs.insert(id.clone(), job);
                if let Some(chip) = self.chip_mut(&id) {
                    chip.login.apply(LoginUpdate::Started);
                }
            }
            Err(err) => {
                tracing::error!("could not start device-code sign-in");
                if let Some(chip) = self.chip_mut(&id) {
                    chip.login.apply(LoginUpdate::Failed(err));
                }
            }
        }
        Task::none()
    }

    fn cancel_sign_in(&mut self) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.jobs.remove(&id);
        if let Some(chip) = self.chip_mut(&id) {
            chip.login.apply(LoginUpdate::Canceled);
        }
    }

    fn start_sign_out(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(id) = self.selected.clone() else {
            return Task::none();
        };
        let Some(chip) = self.accounts.iter().find(|chip| chip.id == id) else {
            return Task::none();
        };
        if chip.login.phase == LoginPhase::SigningOut || chip.home_error.is_some() {
            return Task::none();
        }
        let home = chip.cli_home.clone();
        self.jobs.remove(&id);
        if let Some(chip) = self.chip_mut(&id) {
            chip.login.apply(LoginUpdate::SigningOut);
        }
        Task::perform(login::logout_async(home), move |result| {
            cosmic::action::Action::App(Message::LogoutFinished(id, result))
        })
    }

    fn finish_sign_out(&mut self, id: &str, result: Result<(), String>) {
        let still_signed_in = self
            .accounts
            .iter()
            .find(|chip| chip.id == id)
            .filter(|chip| chip.home_error.is_none())
            .is_some_and(|chip| auth::load_bearer_from(&chip.cli_home).is_ok());
        let Some(chip) = self.chip_mut(id) else {
            return;
        };
        if result.is_ok() || !still_signed_in {
            chip.snapshot = None;
            chip.history.clear();
            chip.error = Some(FetchError::Auth(AuthError::Missing));
            chip.login.apply(LoginUpdate::SignedOut);
        } else {
            tracing::error!("sign-out failed");
            chip.login.phase = LoginPhase::Failed;
            chip.login.message = result.err().unwrap_or_else(|| "Sign-out failed.".into());
        }
    }

    fn set_selected_label(&mut self, value: String) {
        let Some(index) = self.selected_chip().map(|chip| chip.index) else {
            return;
        };
        let Some(account) = self.config.accounts.get_mut(index) else {
            return;
        };
        account.label = value;
        self.save_config();
        self.sync_accounts();
    }

    fn open_popup(&mut self) -> Task<cosmic::Action<Message>> {
        let Some(parent) = self.core.main_window_id() else {
            tracing::warn!("popup requested with no main window");
            return Task::none();
        };
        let new_id = Id::unique();
        self.popup.replace(new_id);
        let mut popup_settings = self
            .core
            .applet
            .get_popup_settings(parent, new_id, None, None, None);
        popup_settings.positioner.anchor_rect = cosmic::iced::Rectangle {
            x: 0,
            y: 0,
            width: self.size.width as i32,
            height: self.size.height as i32,
        };
        popup_settings.positioner.size_limits = Limits::NONE
            .min_width(320.0)
            .max_width(420.0)
            .min_height(200.0)
            .max_height(720.0);
        get_popup(popup_settings)
    }

    fn account_chip(&self, account: &AccountChip) -> Element<'_, Message> {
        let (label, color) = chip_label(account, &self.config);
        let theme = theme::active();
        let track: Color = theme.cosmic().on_bg_color().into();
        let percent = account
            .snapshot
            .as_ref()
            .map(|snapshot| self.config.display_percent(snapshot.percent))
            .unwrap_or(0.0);
        let svg = usage_ring(percent, color, track, RingIcon::Hammer);
        let sparkline = (self.config.show_sparkline && !account.history.is_empty())
            .then(|| chip::sparkline(&account.history));
        panel_chip(
            &self.core.applet,
            svg,
            PanelChip {
                usage_label: label,
                color,
                show_usage: self.config.show_percent || account.snapshot.is_none(),
                identity: Some(account.label.clone()),
                sparkline,
                on_press: Message::TogglePopup(account.id.clone()),
            },
        )
    }

    fn problem_chip(&self) -> Element<'_, Message> {
        let theme = theme::active();
        let track: Color = theme.cosmic().on_bg_color().into();
        let color: Color = theme.cosmic().warning_color().into();
        let svg = usage_ring(0.0, color, track, RingIcon::Hammer);
        panel_chip(
            &self.core.applet,
            svg,
            PanelChip {
                usage_label: "?".into(),
                color,
                show_usage: true,
                identity: Some("Config".into()),
                sparkline: None,
                on_press: Message::TogglePopup(CONFIG_ERROR_ID.into()),
            },
        )
    }

    fn empty_overview(&self) -> Element<'_, Message> {
        let mut col = column::with_capacity(6).padding([12, 0]).spacing(8);
        col = col.push(padded(text::title4("Grok Monitor")));
        if let Some(error) = &self.config_error {
            col = col.push(padded(text::body(error).class(destructive_text())));
        } else {
            col = col.push(padded(text::body(
                "No accounts configured. The panel is empty until the accounts list has an entry.",
            )));
        }
        col = col.push(padded(text::caption(format!(
            "Accounts file: {ACCOUNTS_CONFIG_PATH}"
        ))));
        col = col.push(padded(text::caption(
            "Each entry has a label and a cli_home. The list order is the panel order.",
        )));
        col.into()
    }

    fn overview(&self) -> Element<'_, Message> {
        let mut col = column::with_capacity(16).padding([12, 0]).spacing(8);
        let Some(chip) = self.selected_chip() else {
            return self.empty_overview();
        };
        let snapshot = chip.snapshot.clone();
        let error = chip.error.clone();
        let label = chip.label.clone();
        let home_display = chip.cli_home_display.clone();
        let login = chip.login.clone();
        let home_error = chip.home_error.clone();

        let title = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.plan.clone())
            .unwrap_or_else(|| label.clone());
        let email = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.email.clone())
            .unwrap_or_default();
        let mut heading = column::with_capacity(4).push(text::title4(title));
        if !email.is_empty() {
            heading = heading.push(text::caption(email));
        }
        if !home_display.is_empty() {
            heading = heading.push(text::caption(format!("CLI home: {home_display}")));
        }
        if snapshot.as_ref().is_some_and(|snapshot| snapshot.unified) {
            heading = heading.push(text::caption("Shared weekly pool"));
        }
        col = col.push(padded(heading));
        col = col.push(padded(divider::horizontal::default()));

        if let Some(snapshot) = snapshot.as_ref() {
            let shown = self.config.display_percent(snapshot.percent);
            col = col.push(padded(usage_bar(shown, snapshot.percent)));
            let mut detail = format!(
                "{} · {}",
                period_label(snapshot.period_type.as_deref()),
                format_percent(shown)
            );
            if self.config.show_remaining {
                detail.push_str(" remaining");
            }
            if let (Some(used), Some(limit)) = (snapshot.used, snapshot.limit) {
                detail = format!("{detail}  ({used:.0} / {limit:.0})");
            }
            col = col.push(padded(text::body(detail)));
            if let Some(end) = snapshot.resets_at {
                col = col.push(padded(text::caption(format_remaining(end))));
            }
            if self.config.show_pace
                && let Some(pace) = maybe_weekly_pace(
                    snapshot.percent,
                    Utc::now(),
                    snapshot.starts_at,
                    snapshot.resets_at,
                    snapshot.period_type.as_deref(),
                )
            {
                col = col.push(padded(text::caption(pace.popup_line())));
            }
            let age = (Utc::now() - snapshot.fetched_at).num_seconds().max(0);
            col = col.push(padded(text::caption(format!("updated {age}s ago"))));
        }

        if home_error.is_none()
            && let Some(error) = error.as_ref().filter(|_| {
                !matches!(
                    login.phase,
                    LoginPhase::Starting | LoginPhase::Waiting | LoginPhase::Succeeded
                )
            })
        {
            col = col.push(padded(
                text::caption(error.to_string()).class(destructive_text()),
            ));
        }
        if let Some(error) = &self.open_error {
            col = col.push(padded(text::caption(error).class(destructive_text())));
        }

        col = col.push(padded(self.login_controls(
            &login,
            home_error.as_deref(),
            snapshot.is_some(),
        )));

        let session_line = if self.live.count == 0 {
            "no live sessions".to_string()
        } else {
            format!(
                "{} live · {}",
                self.live.count,
                self.live
                    .names
                    .iter()
                    .take(3)
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        col = col.push(padded(text::body(session_line)));
        col = col.push(padded(divider::horizontal::default()));
        col = col.push(padded(
            row::with_capacity(2)
                .push(
                    button::standard("Open usage")
                        .on_press(Message::OpenUsage)
                        .width(Length::Fill),
                )
                .push(
                    button::standard("Copy")
                        .on_press(Message::CopyPercent)
                        .width(Length::Shrink),
                )
                .spacing(8),
        ));
        if self.config_error.is_none() {
            col = col.push(padded(
                button::link("Settings…").on_press(Message::ShowSettings),
            ));
        }
        col.into()
    }

    fn login_controls(
        &self,
        login_state: &LoginUiState,
        home_error: Option<&str>,
        signed_in: bool,
    ) -> Element<'_, Message> {
        let mut col = column::with_capacity(6).spacing(8);
        if let Some(error) = home_error {
            col = col.push(text::caption(error.to_string()).class(destructive_text()));
            return col.into();
        }
        match login_state.phase {
            LoginPhase::Starting | LoginPhase::Waiting => {
                col = col.push(text::caption(
                    "Open this URL in an Incognito window and enter the code. A normal browser window can attach the account already signed in there.",
                ));
                if let Some(url) = &login_state.prompt.verification_url {
                    col = col.push(text::caption(url.clone()));
                } else {
                    col = col.push(text::caption("Starting device-code sign-in…"));
                }
                if let Some(code) = &login_state.prompt.user_code {
                    col = col.push(text::title4(code.clone()));
                }
                let mut copies = row::with_capacity(2).spacing(8);
                if let Some(url) = &login_state.prompt.verification_url {
                    copies = copies.push(
                        button::standard("Copy URL")
                            .on_press(Message::CopyText(url.clone()))
                            .width(Length::Fill),
                    );
                }
                if let Some(code) = &login_state.prompt.user_code {
                    copies = copies.push(
                        button::standard("Copy code")
                            .on_press(Message::CopyText(code.clone()))
                            .width(Length::Fill),
                    );
                }
                if login_state.prompt.verification_url.is_some()
                    || login_state.prompt.user_code.is_some()
                {
                    col = col.push(copies);
                }
                if login_state.phase == LoginPhase::Waiting {
                    col = col.push(text::caption("Waiting for sign-in…"));
                }
                col = col.push(
                    button::standard("Cancel")
                        .on_press(Message::CancelSignIn)
                        .width(Length::Fill),
                );
            }
            LoginPhase::SigningOut | LoginPhase::Succeeded => {
                if !login_state.message.is_empty() {
                    col = col.push(text::caption(login_state.message.clone()));
                }
            }
            LoginPhase::Failed => {
                if !login_state.message.is_empty() {
                    col = col
                        .push(text::caption(login_state.message.clone()).class(destructive_text()));
                }
                if signed_in {
                    col = col.push(sign_out_button());
                } else {
                    col = col.push(sign_in_button());
                }
            }
            LoginPhase::Idle => {
                if !login_state.message.is_empty() {
                    col = col.push(text::caption(login_state.message.clone()));
                }
                if signed_in {
                    col = col.push(sign_out_button());
                } else {
                    col = col.push(sign_in_button());
                }
            }
        }
        col.into()
    }

    fn settings_page(&self) -> Element<'_, Message> {
        let mut col = column::with_capacity(16).padding([12, 0]).spacing(8);
        col = col.push(padded(button::standard("← Back").on_press(Message::Back)));
        col = col.push(padded(text::title4("Settings")));
        col = col.push(padded(text::body("Poll interval")));
        col = col.push(padded(
            row::with_capacity(3)
                .push(poll_button(30, self.config.poll_secs))
                .push(poll_button(60, self.config.poll_secs))
                .push(poll_button(300, self.config.poll_secs))
                .spacing(8),
        ));
        col = col.push(padded(settings::item(
            "Sparkline on panel",
            widget::toggler(self.config.show_sparkline).on_toggle(Message::ToggleSparkline),
        )));
        col = col.push(padded(settings::item(
            "Percent on panel",
            widget::toggler(self.config.show_percent).on_toggle(Message::TogglePercent),
        )));
        col = col.push(padded(settings::item(
            "Pace in popup",
            widget::toggler(self.config.show_pace).on_toggle(Message::TogglePace),
        )));
        col = col.push(padded(self.label_setting()));
        col = col.push(padded(text::body("Panel number")));
        col = col.push(padded(
            row::with_capacity(2)
                .push(mode_button("Used", false, self.config.show_remaining))
                .push(mode_button("Remaining", true, self.config.show_remaining))
                .spacing(8),
        ));
        col = col.push(padded(text::caption(
            "Color by % used: green 0–50 · yellow 50–80 · orange 80–90 · red 90+",
        )));
        col = col.push(padded(text::caption(
            "Icons come from the accounts list, in that order. Each icon uses its own CLI home.",
        )));
        col = col.push(padded(text::caption(format!(
            "Accounts file: {ACCOUNTS_CONFIG_PATH}"
        ))));
        col = col.push(padded(text::caption(format!(
            "Grok Monitor {}",
            env!("CARGO_PKG_VERSION")
        ))));
        col.into()
    }

    fn label_setting(&self) -> Element<'_, Message> {
        let Some(chip) = self.selected_chip() else {
            return text::caption("No account selected.").into();
        };
        let index = chip.index;
        let home = chip.cli_home_display.clone();
        let value = self
            .config
            .accounts
            .get(index)
            .map(|account| account.label.clone())
            .unwrap_or_default();
        column::with_capacity(3)
            .push(text::body("Panel label"))
            .push(
                widget::text_input("Grok", value)
                    .on_input(Message::SetLabel)
                    .width(Length::Fill),
            )
            .push(text::caption(format!(
                "Shown on this icon. CLI home: {home}"
            )))
            .spacing(8)
            .into()
    }
}

fn apply_icon(chip: &mut AccountChip, icon: ResolvedIcon) {
    let was_config = matches!(chip.error, Some(FetchError::Config(_)));
    chip.index = icon.index;
    chip.id = icon.id;
    chip.label = icon.panel_label;
    chip.cli_home = icon.cli_home;
    chip.cli_home_display = icon.cli_home_display;
    chip.home_error = icon.home_error.clone();
    if let Some(message) = icon.home_error {
        chip.error = Some(FetchError::Config(message));
        chip.snapshot = None;
        chip.history.clear();
    } else if was_config {
        chip.error = None;
    }
}

fn chip_label(account: &AccountChip, config: &Config) -> (String, Color) {
    let theme = theme::active();
    let cosmic = theme.cosmic();
    if account.snapshot.is_none()
        && (account.login.is_in_progress() || account.login.phase == LoginPhase::Succeeded)
    {
        return ("…".into(), cosmic.on_bg_color().into());
    }
    match (&account.snapshot, &account.error) {
        (Some(snapshot), err) => {
            let mut color = usage_color(snapshot.percent);
            if err.is_some() {
                color.a *= 0.7;
            }
            (
                format_percent(config.display_percent(snapshot.percent)),
                color,
            )
        }
        (None, Some(FetchError::Auth(_))) => ("—".into(), cosmic.on_bg_color().into()),
        (None, Some(_)) => ("?".into(), cosmic.warning_color().into()),
        (None, None) => ("…".into(), cosmic.on_bg_color().into()),
    }
}

fn sign_in_button<'a>() -> Element<'a, Message> {
    button::custom(text::body("Sign in"))
        .class(theme::Button::Suggested)
        .on_press(Message::SignIn)
        .width(Length::Fill)
        .into()
}

fn sign_out_button<'a>() -> Element<'a, Message> {
    button::standard("Sign out")
        .on_press(Message::SignOut)
        .width(Length::Fill)
        .into()
}

fn destructive_text() -> theme::Text {
    theme::Text::Color(theme::active().cosmic().destructive_color().into())
}

fn mode_button<'a>(label: &'a str, remaining: bool, current: bool) -> Element<'a, Message> {
    let class = if remaining == current {
        theme::Button::Suggested
    } else {
        theme::Button::Standard
    };
    button::custom(text::body(label))
        .class(class)
        .on_press(Message::SetRemaining(remaining))
        .into()
}

fn poll_button<'a>(secs: u64, current: u64) -> Element<'a, Message> {
    let label = match secs {
        30 => "30s",
        60 => "60s",
        300 => "5m",
        _ => "?",
    };
    let class = if secs == current {
        theme::Button::Suggested
    } else {
        theme::Button::Standard
    };
    button::custom(text::body(label))
        .class(class)
        .on_press(Message::SetPoll(secs))
        .into()
}

fn padded<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    cosmic::applet::padded_control(content).into()
}
