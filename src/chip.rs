use crate::ring::usage_color;
use cosmic::applet::Context;
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::{Background, Border, Color, Length};
use cosmic::widget::{self, button, column, container, row, space, text};
use cosmic::{Element, theme};
use std::collections::VecDeque;

pub const IDENTITY_MAX_CHARS: usize = 28;

pub fn truncate_identity(raw: &str) -> String {
    truncate_identity_to(raw, IDENTITY_MAX_CHARS)
}

fn truncate_identity_to(raw: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if raw.chars().count() <= max {
        return raw.to_string();
    }
    if let Some((local, domain)) = raw.rsplit_once('@')
        && !domain.is_empty()
    {
        let suffix = format!("…@{domain}");
        let suffix_len = suffix.chars().count();
        if suffix_len < max {
            let keep = max - suffix_len;
            let prefix: String = local.chars().take(keep).collect();
            return format!("{prefix}{suffix}");
        }
    }
    let prefix: String = raw.chars().take(max.saturating_sub(1)).collect();
    format!("{prefix}…")
}

pub fn ring_icon<'a, Message: 'a>(applet: &Context, svg: String) -> Element<'a, Message> {
    let size = applet.suggested_size(true).0.saturating_add(10).max(24);
    widget::icon::from_svg_bytes(svg.into_bytes())
        .symbolic(false)
        .icon()
        .size(size)
        .into()
}

pub fn usage_bar<'a, Message: 'a>(fill_percent: f32, used_percent: f32) -> Element<'a, Message> {
    column::with_capacity(2)
        .push(fill_track(fill_percent, usage_color(used_percent)))
        .push(
            row::with_capacity(5)
                .push(text::caption("0"))
                .push(space::horizontal())
                .push(text::caption("50"))
                .push(space::horizontal())
                .push(text::caption("100")),
        )
        .spacing(4)
        .into()
}

/// Label column shared by the popup's usage bar and time bar.
///
/// `100% remaining` is the longest label: about 107px in the 14px body
/// font. Time labels (`3h 12m`, `2d 4h`, `—`) are shorter. A fixed column
/// keeps a short label from lengthening its track. Both tracks are
/// `Length::Fill` inside a row of the same width, so they match.
const METER_LABEL_WIDTH: f32 = 136.0;

fn meter_label_length() -> Length {
    Length::Fixed(METER_LABEL_WIDTH)
}

/// A meter with its label on the right. `fill_percent` is clamped to
/// 0..=100. A missing fraction is passed as `0.0` (empty track).
///
/// The label uses the shared fixed column and the track fills the rest, so
/// every meter from this function has the same track width.
pub fn labeled_bar<'a, Message: 'a>(
    fill_percent: f32,
    color: Color,
    label: impl Into<String>,
) -> Element<'a, Message> {
    row::with_capacity(2)
        .width(Length::Fill)
        .push(fill_track(fill_percent, color))
        .push(meter_label(label.into()))
        .align_y(Vertical::Center)
        .spacing(8)
        .into()
}

fn meter_label<'a, Message: 'a>(label: String) -> Element<'a, Message> {
    container(text::body(label))
        .width(meter_label_length())
        .align_x(Horizontal::Left)
        .align_y(Vertical::Center)
        .into()
}

fn fill_track<'a, Message: 'a>(fill_percent: f32, color: Color) -> Element<'a, Message> {
    let mut track: Color = theme::active().cosmic().bg_component_color().into();
    track.a = 1.0;
    let mut fill = color;
    fill.a = 1.0;
    let filled = (fill_percent.clamp(0.0, 100.0) * 10.0).round() as u16;
    let rest = 1000u16.saturating_sub(filled);
    let mut bar = row::with_capacity(2);
    if filled > 0 {
        bar = bar.push(bar_segment(filled, fill));
    }
    if rest > 0 {
        bar = bar.push(bar_segment(rest, track));
    }
    container(bar).width(Length::Fill).into()
}

fn bar_segment<'a, Message: 'a>(portion: u16, color: Color) -> Element<'a, Message> {
    container(space::horizontal())
        .width(Length::FillPortion(portion.max(1)))
        .height(Length::Fixed(8.0))
        .class(theme::Container::custom(move |_| {
            cosmic::iced::widget::container::Style {
                background: Some(Background::Color(color)),
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .into()
}

pub fn sparkline<'a, Message: 'a>(history: &VecDeque<f32>) -> Element<'a, Message> {
    let bars: Vec<Element<'a, Message>> = history
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

pub struct PanelChip<'a, Message> {
    pub usage_label: String,
    pub color: Color,
    pub show_usage: bool,
    pub identity: Option<String>,
    pub sparkline: Option<Element<'a, Message>>,
    pub on_press: Message,
}

/// Popup body on the theme's opaque background, so a frosted applet popup
/// does not show through the content.
pub fn solid_popup<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .class(theme::Container::custom(|ui| {
            let cosmic = ui.cosmic();
            let mut background: Color = cosmic.bg_color().into();
            background.a = 1.0;
            cosmic::iced::widget::container::Style {
                background: Some(Background::Color(background)),
                border: Border {
                    radius: cosmic.corner_radii.radius_m.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                ..Default::default()
            }
        }))
        .into()
}

pub fn panel_chip<'a, Message: Clone + 'static>(
    applet: &Context,
    ring_svg: String,
    chip: PanelChip<'a, Message>,
) -> Element<'a, Message> {
    let ring = ring_icon(applet, ring_svg);
    let mut text_col = column::with_capacity(2).spacing(0);
    if chip.show_usage {
        text_col = text_col.push(
            applet
                .text(chip.usage_label)
                .class(theme::Text::Color(chip.color)),
        );
    }
    let identity = chip.identity.filter(|s| !s.trim().is_empty());
    if let Some(ref identity) = identity {
        let caption = truncate_identity(identity.trim());
        let id_color: Color = theme::active().cosmic().on_bg_color().into();
        text_col = text_col.push(text::caption(caption).class(theme::Text::Color(id_color)));
    }

    let mut children: Vec<Element<'a, Message>> = vec![ring];
    if chip.show_usage || identity.is_some() {
        children.push(text_col.align_x(Horizontal::Left).into());
    }
    if let Some(sparkline) = chip.sparkline {
        children.push(sparkline);
    }

    let data = row::with_children(children)
        .align_y(Vertical::Center)
        .spacing(4);
    button::custom(data)
        .class(theme::Button::AppletIcon)
        .on_press_down(chip.on_press)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_identity_keeps_short() {
        assert_eq!(truncate_identity("user@example.com"), "user@example.com");
        assert_eq!(truncate_identity("ab"), "ab");
    }

    #[test]
    fn truncate_identity_shortens_local_part() {
        let long = "very-long-account-name@example.com";
        let out = truncate_identity(long);
        assert!(out.chars().count() <= IDENTITY_MAX_CHARS);
        assert!(out.ends_with("@example.com"));
        assert!(out.contains('…'));
        assert_eq!(out, "very-long-accou…@example.com");
    }

    #[test]
    fn truncate_identity_without_at() {
        let raw = "abcdefghijklmnopqrstuvwxyz0123456789";
        let out = truncate_identity(raw);
        assert_eq!(out.chars().count(), IDENTITY_MAX_CHARS);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn truncate_identity_to_zero() {
        assert_eq!(truncate_identity_to("a@b.com", 0), "");
    }

    #[test]
    fn popup_meter_labels_share_one_fixed_column() {
        assert_eq!(meter_label_length(), Length::Fixed(136.0));
        let longest = crate::billing::format_remaining_percent(0.0);
        assert_eq!(longest, "100% remaining");
        for sample in ["72% remaining", "3h 12m", "2d 4h", "—", "0m"] {
            assert!(sample.chars().count() < longest.chars().count());
        }
    }
}
