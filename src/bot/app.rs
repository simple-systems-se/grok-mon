//! Retired panel slot.
//!
//! Grok Build and Grok Bot share one weekly pool, shown on Grok Monitor.
//! This process does not poll usage, read Grok Bot profile directories, or
//! decrypt secrets. It only tells the user to remove the applet.

use crate::chip::{self, PanelChip, panel_chip};
use crate::config::BOT_APP_ID;
use crate::ring::{RingIcon, usage_ring};
use cosmic::app::Core;
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::event::listen_with;
use cosmic::iced::platform_specific::shell::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::window::Id;
use cosmic::iced::{Color, Length, Limits, Size, Subscription};
use cosmic::widget::{self, column, container, row, text};
use cosmic::{Element, Task, theme};
use std::sync::LazyLock;

static PANEL_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("grok-bot-monitor-panel"));

pub struct GrokBotMonitor {
    core: Core,
    popup: Option<Id>,
    size: Size,
}

impl Default for GrokBotMonitor {
    fn default() -> Self {
        Self {
            core: Core::default(),
            popup: None,
            size: Size::new(10.0, 10.0),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    Size(Size),
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
        (
            Self {
                core,
                ..Self::default()
            },
            Task::none(),
        )
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
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
        })
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::TogglePopup => {
                if let Some(id) = self.popup.take() {
                    return destroy_popup(id);
                }
                return self.open_popup();
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
            Message::Size(size) => {
                self.size = size;
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let data = if self.core.applet.is_horizontal() {
            Element::from(
                row::with_children(vec![self.account_chip()])
                    .align_y(Vertical::Center)
                    .spacing(8),
            )
        } else {
            Element::from(
                column::with_children(vec![self.account_chip()])
                    .align_x(Horizontal::Center)
                    .spacing(8),
            )
        };
        widget::autosize::autosize(data, PANEL_ID.clone()).into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        self.core
            .applet
            .popup_container(chip::solid_popup(
                container(retired_overview()).width(Length::Fixed(360.0)),
            ))
            .into()
    }
}

impl GrokBotMonitor {
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

    fn account_chip(&self) -> Element<'_, Message> {
        let theme = theme::active();
        let cosmic = theme.cosmic();
        let color: Color = cosmic.on_bg_color().into();
        let track: Color = cosmic.on_bg_color().into();
        let center: Color = cosmic.bg_color().into();
        let svg = usage_ring(0.0, color, track, center, RingIcon::Bot);
        panel_chip(
            &self.core.applet,
            svg,
            PanelChip {
                usage_label: "—".into(),
                color,
                show_usage: true,
                identity: Some("remove".into()),
                sparkline: None,
                on_press: Message::TogglePopup,
            },
        )
    }
}

fn retired_overview() -> Element<'static, Message> {
    column::with_capacity(6)
        .padding([12, 0])
        .spacing(8)
        .push(padded(text::title4("Grok Bot Monitor")))
        .push(padded(text::body(
            "This applet no longer reads usage. Grok Build and Grok Bot share one weekly pool, shown once per account on Grok Monitor.",
        )))
        .push(padded(text::body(
            "The work account is labeled sim sys. The personal account is labeled pers.",
        )))
        .push(padded(text::caption(
            "Remove this applet: COSMIC Settings → Desktop → Panel → Applets. Select Grok Bot Monitor and remove it. The package does not change your panel layout.",
        )))
        .push(padded(text::caption("Grok API Monitor is unchanged.")))
        .into()
}

fn padded(content: impl Into<Element<'static, Message>>) -> Element<'static, Message> {
    cosmic::applet::padded_control(content).into()
}
