use crate::ring::usage_color;
use cosmic::applet::Context;
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::{Background, Border, Color, Length};
use cosmic::widget::{self, button, column, container, row, space, text};
use cosmic::{Element, theme};
use std::collections::VecDeque;

pub const IDENTITY_MAX_CHARS: usize = 28;

pub fn display_identity(email: Option<&str>, name: Option<&str>) -> Option<String> {
    email
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            name.map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
}

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
    let fill = usage_color(used_percent);
    let mut track: Color = theme::active().cosmic().on_bg_color().into();
    track.a *= 0.22;
    let filled = (fill_percent.clamp(0.0, 100.0) * 10.0).round() as u16;
    let rest = 1000u16.saturating_sub(filled);
    let mut bar = row::with_capacity(2);
    if filled > 0 {
        bar = bar.push(bar_segment(filled, fill));
    }
    if rest > 0 {
        bar = bar.push(bar_segment(rest, track));
    }
    column::with_capacity(2)
        .push(container(bar).width(Length::Fill))
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
    fn display_identity_prefers_email() {
        assert_eq!(
            display_identity(Some("a@x.ai"), Some("Ada")).as_deref(),
            Some("a@x.ai")
        );
        assert_eq!(display_identity(None, Some("Ada")).as_deref(), Some("Ada"));
        assert_eq!(
            display_identity(Some("  "), Some("Ada")).as_deref(),
            Some("Ada")
        );
        assert_eq!(display_identity(None, None), None);
        assert_eq!(display_identity(Some(""), Some("")), None);
    }

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
}
