use super::launch;
use super::roster::{BotRoster, live_roster};
use super::usage::{BotSnapshot, FetchError, fetch_bot_usage};
use crate::billing::{format_percent, format_remaining};
use crate::config::{BOT_APP_ID, Config};
use crate::ring::{RingIcon, usage_color, usage_ring};
use chrono::Utc;
use cosmic::app::Core;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::event::listen_with;
use cosmic::iced::platform_specific::shell::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::window::Id;
use cosmic::iced::{Color, Length, Limits, Size, Subscription};
use cosmic::widget::{self, button, column, container, divider, row, settings, space, text};
use cosmic::{Element, Task, theme};
use std::collections::VecDeque;
use std::sync::LazyLock;

static PANEL_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("grok-bot-monitor-panel"));

const HISTORY_LEN: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Settings,
}

pub struct GrokBotMonitor {
    core: Core,
    popup: Option<Id>,
    config: Config,
    config_handler: Option<cosmic_config::Config>,
    snapshot: Option<BotSnapshot>,
    error: Option<FetchError>,
    history: VecDeque<f32>,
    live: BotRoster,
    page: Page,
    size: Size,
    open_error: Option<String>,
    fetching: bool,
}

impl Default for GrokBotMonitor {
    fn default() -> Self {
        Self {
            core: Core::default(),
            popup: None,
            config: Config::default(),
            config_handler: None,
            snapshot: None,
            error: None,
            history: VecDeque::new(),
            live: BotRoster::default(),
            page: Page::Overview,
            size: Size::new(10.0, 10.0),
            open_error: None,
            fetching: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    TogglePopup,
    PopupClosed(Id),
    Size(Size),
    UsageFetched(Result<BotSnapshot, FetchError>),
    OpenApp,
    CopyPercent,
    ShowSettings,
    Back,
    SetPoll(u64),
    ToggleSparkline(bool),
    TogglePercent(bool),
    SetRemaining(bool),
    ConfigChanged(Config),
}

impl cosmic::Application for GrokBotMonitor {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = BOT_APP_ID;

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
        Subscription::batch([
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
        ])
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Tick => {
                self.live = live_roster();
                if self.fetching {
                    return Task::none();
                }
                self.fetching = true;
                return Task::perform(fetch_bot_usage(), |result| {
                    cosmic::action::Action::App(Message::UsageFetched(result))
                });
            }
            Message::UsageFetched(result) => {
                self.fetching = false;
                match result {
                    Ok(snapshot) => {
                        self.error = None;
                        if self.history.len() == HISTORY_LEN {
                            self.history.pop_front();
                        }
                        self.history.push_back(snapshot.percent);
                        self.snapshot = Some(snapshot);
                    }
                    Err(err) => {
                        if matches!(err, FetchError::Auth(_)) {
                            self.snapshot = None;
                            self.history.clear();
                        }
                        self.error = Some(err);
                    }
                }
            }
            Message::TogglePopup => {
                return if let Some(id) = self.popup.take() {
                    self.page = Page::Overview;
                    destroy_popup(id)
                } else {
                    self.live = live_roster();
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
                        .max_height(520.0);
                    get_popup(popup_settings)
                };
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
            Message::OpenApp => match launch::open_grok_bot() {
                Ok(()) => self.open_error = None,
                Err(err) => {
                    tracing::error!("failed to open Grok Bot: {err}");
                    self.open_error = Some(err);
                }
            },
            Message::CopyPercent => {
                if let Some(snapshot) = &self.snapshot {
                    if snapshot.enterprise {
                        return cosmic::iced::clipboard::write("n/a".into());
                    }
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
            Message::ConfigChanged(config) => {
                self.config = config;
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let (label, color) = self.chip_label();
        let ring = self.usage_badge(color);
        let percent = self
            .core
            .applet
            .text(label)
            .class(theme::Text::Color(color));

        let mut children: Vec<Element<'_, Message>> = vec![ring];
        if self.config.show_percent || self.snapshot.is_none() {
            children.push(percent.into());
        }
        if self.config.show_sparkline && !self.history.is_empty() {
            children.push(self.sparkline());
        }

        let data = if self.core.applet.is_horizontal() {
            Element::from(
                row::with_children(children)
                    .align_y(Vertical::Center)
                    .spacing(4),
            )
        } else {
            Element::from(
                column::with_children(children)
                    .align_x(Horizontal::Center)
                    .spacing(4),
            )
        };

        let button = button::custom(data)
            .class(theme::Button::AppletIcon)
            .on_press_down(Message::TogglePopup);

        widget::autosize::autosize(button, PANEL_ID.clone()).into()
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

impl GrokBotMonitor {
    fn save_config(&mut self) {
        if let Some(handler) = &self.config_handler
            && let Err(e) = self.config.write_entry(handler)
        {
            tracing::error!("failed to save config: {e}");
        }
    }

    fn usage_badge(&self, color: Color) -> Element<'_, Message> {
        let theme = theme::active();
        let track: Color = theme.cosmic().on_bg_color().into();
        let percent = self
            .snapshot
            .as_ref()
            .map(|s| {
                if s.enterprise {
                    0.0
                } else {
                    self.config.display_percent(s.percent)
                }
            })
            .unwrap_or(0.0);
        let svg = usage_ring(percent, color, track, RingIcon::Bot);
        let size = self
            .core
            .applet
            .suggested_size(true)
            .0
            .saturating_add(10)
            .max(24);
        widget::icon::from_svg_bytes(svg.into_bytes())
            .symbolic(false)
            .icon()
            .size(size)
            .into()
    }

    fn chip_label(&self) -> (String, Color) {
        let theme = theme::active();
        let cosmic = theme.cosmic();
        match (&self.snapshot, &self.error) {
            (Some(snapshot), err) if snapshot.enterprise => {
                let mut color: Color = cosmic.on_bg_color().into();
                if err.is_some() {
                    color.a *= 0.7;
                }
                ("n/a".into(), color)
            }
            (Some(snapshot), err) => {
                let mut color = usage_color(snapshot.percent);
                if err.is_some() {
                    color.a *= 0.7;
                }
                (
                    format_percent(self.config.display_percent(snapshot.percent)),
                    color,
                )
            }
            (None, Some(FetchError::Auth(_))) => ("—".into(), cosmic.on_bg_color().into()),
            (None, Some(_)) => ("?".into(), cosmic.warning_color().into()),
            (None, None) => ("…".into(), cosmic.on_bg_color().into()),
        }
    }

    fn sparkline(&self) -> Element<'_, Message> {
        let bars: Vec<Element<'_, Message>> = self
            .history
            .iter()
            .map(|p| {
                let h = (p.clamp(0.0, 100.0) / 100.0 * 14.0).max(1.0);
                container(space::vertical().height(Length::Fixed(h)))
                    .width(Length::Fixed(2.0))
                    .class(theme::Container::Primary)
                    .into()
            })
            .collect();
        row::with_children(bars)
            .spacing(1)
            .align_y(Vertical::Bottom)
            .height(Length::Fixed(14.0))
            .into()
    }

    fn overview(&self) -> Element<'_, Message> {
        let mut col = column::with_capacity(14).padding([12, 0]).spacing(8);

        let title = self
            .snapshot
            .as_ref()
            .and_then(|s| s.plan.as_deref())
            .unwrap_or("Grok Bot");
        let email = self
            .snapshot
            .as_ref()
            .and_then(|s| s.email.as_deref())
            .unwrap_or("");

        col = col.push(padded(
            column::with_capacity(2)
                .push(text::title4(title))
                .push(text::caption(email)),
        ));

        col = col.push(padded(divider::horizontal::default()));

        if let Some(snapshot) = &self.snapshot {
            if snapshot.enterprise {
                col = col.push(padded(text::body("Team usage unavailable")));
            } else {
                let shown = self.config.display_percent(snapshot.percent);
                col = col.push(padded(self.gauge(shown)));
                let mut detail = format!("Weekly · {}", format_percent(shown));
                if self.config.show_remaining {
                    detail.push_str(" remaining");
                }
                if let (Some(used), Some(limit)) = (snapshot.used_cents, snapshot.limit_cents) {
                    detail = format!("{detail}  (${:.2} / ${:.2})", used / 100.0, limit / 100.0);
                }
                col = col.push(padded(text::body(detail)));
            }

            if let Some(end) = snapshot.resets_at {
                col = col.push(padded(text::caption(format_remaining(end))));
            }
            if let Some(trial) = snapshot.trial_expires_at {
                col = col.push(padded(text::caption(format!(
                    "trial {}",
                    format_remaining(trial)
                ))));
            }

            let age = (Utc::now() - snapshot.fetched_at).num_seconds().max(0);
            col = col.push(padded(text::caption(format!("updated {age}s ago"))));
        }

        if self.snapshot.is_none() {
            col = col.push(padded(text::caption(
                "Reads Grok Bot’s login (keyring + sand-secrets.json) read-only. Unlock the login keyring if prompted.",
            )));
        }

        if let Some(error) = &self.error {
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

        col = col.push(padded(text::body(roster_line(&self.live))));
        col = col.push(padded(text::caption(if self.live.running {
            "Grok Bot running"
        } else {
            "Grok Bot not running"
        })));

        col = col.push(padded(divider::horizontal::default()));

        col = col.push(padded(
            row::with_capacity(3)
                .push(
                    button::standard("Open Grok Bot")
                        .on_press(Message::OpenApp)
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
            "Grok Bot Monitor {}",
            env!("CARGO_PKG_VERSION")
        ))));

        col.into()
    }

    fn gauge(&self, percent: f32) -> Element<'_, Message> {
        let filled = (percent.clamp(0.0, 100.0) * 10.0).round() as u16;
        let rest = 1000u16.saturating_sub(filled).max(1);
        let filled = filled.max(1);
        container(
            row::with_capacity(2)
                .push(
                    container(space::horizontal())
                        .width(Length::FillPortion(filled))
                        .height(Length::Fixed(8.0))
                        .class(theme::Container::Primary),
                )
                .push(
                    container(space::horizontal())
                        .width(Length::FillPortion(rest))
                        .height(Length::Fixed(8.0))
                        .class(theme::Container::Background),
                ),
        )
        .width(Length::Fill)
        .into()
    }
}

fn roster_line(live: &BotRoster) -> String {
    if live.count == 0 {
        return "no bots".into();
    }
    let mut line = format!(
        "{} bots · {} unread · {}",
        live.count,
        live.unread,
        live.names.join(", ")
    );
    if live.needs_you {
        line.push_str(" · needs you");
    }
    line
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
