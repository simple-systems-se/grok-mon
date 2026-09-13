use crate::billing::{
    BuildAccountFetch, FetchError, UsageSnapshot, fetch_all_usage, format_percent,
    format_remaining, period_label,
};
use crate::chip::{self, PanelChip, panel_chip, usage_bar};
use crate::config::{APP_ID, Config, USAGE_URL};
use crate::ring::{RingIcon, usage_color, usage_ring};
use crate::sessions::{LiveSessions, live_sessions};
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
use std::collections::VecDeque;
use std::sync::LazyLock;

static PANEL_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("grok-monitor-panel"));

const HISTORY_LEN: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Settings,
}

#[derive(Default)]
struct AccountChip {
    id: String,
    identity: Option<String>,
    snapshot: Option<UsageSnapshot>,
    error: Option<FetchError>,
    history: VecDeque<f32>,
}

pub struct GrokMonitor {
    core: Core,
    popup: Option<Id>,
    config: Config,
    config_handler: Option<cosmic_config::Config>,
    accounts: Vec<AccountChip>,
    selected: Option<String>,
    live: LiveSessions,
    page: Page,
    size: Size,
    open_error: Option<String>,
    fetching: bool,
    fetch_started: Option<std::time::Instant>,
}

impl Default for GrokMonitor {
    fn default() -> Self {
        Self {
            core: Core::default(),
            popup: None,
            config: Config::default(),
            config_handler: None,
            accounts: vec![AccountChip::default()],
            selected: None,
            live: LiveSessions::default(),
            page: Page::Overview,
            size: Size::new(10.0, 10.0),
            open_error: None,
            fetching: false,
            fetch_started: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    Clock,
    TogglePopup(String),
    PopupClosed(Id),
    Size(Size),
    UsageFetched(Result<Vec<BuildAccountFetch>, FetchError>),
    OpenUsage,
    CopyPercent,
    ShowSettings,
    Back,
    SetPoll(u64),
    ToggleSparkline(bool),
    TogglePercent(bool),
    SetRemaining(bool),
    SetLabel(String),
    ConfigChanged(Config),
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
        let config = config_handler
            .as_ref()
            .map(|ctx| match Config::get_entry(ctx) {
                Ok(config) => config,
                Err((_errors, config)) => config,
            })
            .unwrap_or_default();

        let app = Self {
            core,
            config,
            config_handler,
            ..Self::default()
        };
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
                .map(|update| Message::ConfigChanged(update.config)),
        ];
        if self.popup.is_some() {
            subs.push(
                cosmic::iced::time::every(std::time::Duration::from_secs(1))
                    .map(|_| Message::Clock),
            );
        }
        Subscription::batch(subs)
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Tick => {
                self.live = live_sessions();
                if self.fetching {
                    if self
                        .fetch_started
                        .is_some_and(|t| t.elapsed() > std::time::Duration::from_secs(30))
                    {
                        tracing::warn!("usage fetch still running after 30s; retrying");
                        self.fetching = false;
                    } else {
                        return Task::none();
                    }
                }
                self.fetching = true;
                self.fetch_started = Some(std::time::Instant::now());
                return Task::perform(fetch_all_usage(), |result| {
                    cosmic::action::Action::App(Message::UsageFetched(result))
                });
            }
            Message::Clock => {}
            Message::UsageFetched(result) => {
                self.fetching = false;
                self.fetch_started = None;
                match result {
                    Ok(fetches) => self.merge_fetches(fetches),
                    Err(err) => {
                        self.accounts = vec![AccountChip {
                            error: Some(err),
                            ..AccountChip::default()
                        }];
                        self.selected = None;
                    }
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
                    self.live = live_sessions();
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
                    tracing::error!("{err}");
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
            Message::SetRemaining(value) => {
                self.config.show_remaining = value;
                self.save_config();
            }
            Message::SetLabel(value) => {
                let id = self.label_account_id();
                if !id.is_empty() {
                    self.config.set_account_label(id, value);
                    self.save_config();
                }
            }
            Message::ConfigChanged(config) => {
                self.config = config;
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let chips: Vec<Element<'_, Message>> = self
            .accounts
            .iter()
            .map(|account| self.account_chip(account))
            .collect();
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
        let content = match self.page {
            Page::Overview => self.overview(),
            Page::Settings => self.settings_page(),
        };

        self.core
            .applet
            .popup_container(container(content).width(Length::Fixed(360.0)))
            .into()
    }
}

impl GrokMonitor {
    fn save_config(&mut self) {
        if let Some(handler) = &self.config_handler
            && let Err(e) = self.config.write_entry(handler)
        {
            tracing::error!("failed to save config: {e}");
        }
    }

    fn selected_chip(&self) -> Option<&AccountChip> {
        self.selected
            .as_ref()
            .and_then(|id| self.accounts.iter().find(|c| &c.id == id))
            .or(self.accounts.first())
    }

    fn merge_fetches(&mut self, fetches: Vec<BuildAccountFetch>) {
        if fetches.is_empty() {
            self.accounts = vec![AccountChip {
                error: Some(FetchError::Auth(crate::auth::AuthError::Missing)),
                ..AccountChip::default()
            }];
            self.selected = None;
            return;
        }
        let mut previous: std::collections::HashMap<String, AccountChip> =
            std::mem::take(&mut self.accounts)
                .into_iter()
                .map(|chip| (chip.id.clone(), chip))
                .collect();
        self.accounts = fetches
            .into_iter()
            .map(|fetch| {
                let mut chip = previous.remove(&fetch.id).unwrap_or_default();
                chip.id = fetch.id;
                if fetch.identity.is_some() {
                    chip.identity = fetch.identity;
                }
                match fetch.result {
                    Ok(snapshot) => {
                        chip.error = None;
                        if chip.history.len() == HISTORY_LEN {
                            chip.history.pop_front();
                        }
                        chip.history.push_back(snapshot.percent);
                        if chip.identity.is_none() {
                            chip.identity = snapshot.email.clone();
                        }
                        chip.snapshot = Some(snapshot);
                    }
                    Err(err) => {
                        if matches!(err, FetchError::Auth(_)) {
                            chip.snapshot = None;
                            chip.history.clear();
                        }
                        chip.error = Some(err);
                    }
                }
                chip
            })
            .collect();
        if self
            .selected
            .as_ref()
            .is_none_or(|id| !self.accounts.iter().any(|c| &c.id == id))
        {
            self.selected = self.accounts.first().map(|c| c.id.clone());
        }
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
            .max_width(380.0)
            .min_height(200.0)
            .max_height(560.0);
        get_popup(popup_settings)
    }

    fn account_chip(&self, account: &AccountChip) -> Element<'_, Message> {
        let (label, color) = chip_label(account, &self.config);
        let theme = theme::active();
        let track: Color = theme.cosmic().on_bg_color().into();
        let percent = account
            .snapshot
            .as_ref()
            .map(|s| self.config.display_percent(s.percent))
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
                identity: self
                    .config
                    .chip_identity(&account.id, account.identity.as_deref()),
                sparkline,
                on_press: Message::TogglePopup(account.id.clone()),
            },
        )
    }

    fn overview(&self) -> Element<'_, Message> {
        let mut col = column::with_capacity(12).padding([12, 0]).spacing(8);
        let chip = self.selected_chip();
        let snapshot = chip.and_then(|c| c.snapshot.as_ref());
        let error = chip.and_then(|c| c.error.as_ref());

        let title = snapshot
            .and_then(|s| s.plan.as_deref())
            .unwrap_or("Grok Build");
        let email = snapshot
            .and_then(|s| s.email.as_deref())
            .or_else(|| chip.and_then(|c| c.identity.as_deref()))
            .unwrap_or("");

        col = col.push(padded(
            column::with_capacity(2)
                .push(text::title4(title))
                .push(text::caption(email)),
        ));

        col = col.push(padded(divider::horizontal::default()));

        if let Some(snapshot) = snapshot {
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

            let age = (Utc::now() - snapshot.fetched_at).num_seconds().max(0);
            col = col.push(padded(text::caption(format!("updated {age}s ago"))));
        }

        if let Some(error) = error {
            col = col.push(padded(text::caption(error.to_string()).class(
                theme::Text::Color({
                    let c = theme::active().cosmic().destructive_color();
                    c.into()
                }),
            )));
        }

        if let Some(error) = &self.open_error {
            col = col.push(padded(text::caption(error).class(theme::Text::Color({
                let c = theme::active().cosmic().destructive_color();
                c.into()
            }))));
        }

        let session_line = if self.live.count == 0 {
            "no live sessions".into()
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
            row::with_capacity(3)
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

        col = col.push(padded(
            button::link("Settings…").on_press(Message::ShowSettings),
        ));

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

        col = col.push(padded(text::caption(format!(
            "Grok Monitor {}",
            env!("CARGO_PKG_VERSION")
        ))));

        col.into()
    }

    fn label_account_id(&self) -> String {
        self.selected
            .clone()
            .filter(|id| !id.is_empty())
            .or_else(|| {
                self.accounts
                    .iter()
                    .map(|c| c.id.clone())
                    .find(|id| !id.is_empty())
            })
            .unwrap_or_default()
    }

    fn label_setting(&self) -> Element<'_, Message> {
        let id = self.label_account_id();
        let auto = self
            .accounts
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.identity.as_deref())
            .filter(|s| !s.is_empty())
            .unwrap_or("account email");
        let value = self
            .config
            .account_labels
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let mut input = widget::text_input(auto, value).width(Length::Fill);
        if !id.is_empty() {
            input = input.on_input(Message::SetLabel);
        }
        column::with_capacity(3)
            .push(text::body("Panel label"))
            .push(input)
            .push(text::caption(format!(
                "Shown on the panel chip. Empty uses {auto}."
            )))
            .spacing(8)
            .into()
    }
}

fn chip_label(account: &AccountChip, config: &Config) -> (String, Color) {
    let theme = theme::active();
    let cosmic = theme.cosmic();
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
