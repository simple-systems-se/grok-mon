use cosmic::iced::Color;

const HAMMER: &str =
    r#"<path d="M6.6 13.2 L10.4 6.2"/><path d="M8.2 4.2 L12.4 6.6 L11.3 8.6 L7.1 6.2 Z"/>"#;
const BOT: &str = r#"<rect x="3.3" y="5.5" width="9.4" height="7.2" rx="2.1"/><path d="M8 5.5V3.3"/><path d="M5.8 8.7h1.4"/><path d="M8.8 8.7h1.4"/>"#;
const KEY: &str = r#"<circle cx="6.2" cy="8" r="2.3"/><path d="M8.5 8h5.2"/><path d="M11.8 8v2.2"/><path d="M13.7 8v1.4"/>"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingIcon {
    Hammer,
    Bot,
    Key,
}

const GREEN: [f32; 3] = [0.18, 0.72, 0.32];
const YELLOW: [f32; 3] = [0.95, 0.82, 0.12];
const ORANGE: [f32; 3] = [0.96, 0.48, 0.08];
const RED: [f32; 3] = [0.90, 0.16, 0.14];
const DARK_RED: [f32; 3] = [0.72, 0.06, 0.06];

const USAGE_STOPS: [(u8, [f32; 3]); 5] = [
    (0, GREEN),
    (50, YELLOW),
    (80, ORANGE),
    (90, RED),
    (100, DARK_RED),
];

pub fn usage_color(used: f32) -> Color {
    let pct = used.clamp(0.0, 100.0).round() as u8;
    for pair in USAGE_STOPS.windows(2) {
        let (p0, c0) = pair[0];
        let (p1, c1) = pair[1];
        if pct <= p1 {
            let span = f32::from(p1.saturating_sub(p0)).max(1.0);
            let t = f32::from(pct.saturating_sub(p0)) / span;
            return Color::from_rgb(
                c0[0] + (c1[0] - c0[0]) * t,
                c0[1] + (c1[1] - c0[1]) * t,
                c0[2] + (c1[2] - c0[2]) * t,
            );
        }
    }
    Color::from_rgb(DARK_RED[0], DARK_RED[1], DARK_RED[2])
}

pub fn usage_ring(
    percent: f32,
    fill: Color,
    track: Color,
    center: Color,
    icon: RingIcon,
) -> String {
    let pct = percent.clamp(0.0, 100.0).round() as u8;
    let fill_hex = color_hex(fill);
    let track_hex = color_hex(track);
    let mut center = center;
    center.a = 1.0;
    let center_hex = color_hex(center);
    let glyph = match icon {
        RingIcon::Hammer => HAMMER,
        RingIcon::Bot => BOT,
        RingIcon::Key => KEY,
    };

    format!(
        r##"<svg viewBox="0 0 34 34" xmlns="http://www.w3.org/2000/svg">
  <path d="M17 1.0845 a 15.9155 15.9155 0 0 1 0 31.831 a 15.9155 15.9155 0 0 1 0 -31.831"
    fill="none" stroke="{track_hex}" stroke-width="2"/>
  <path d="M17 1.0845 a 15.9155 15.9155 0 0 1 0 31.831 a 15.9155 15.9155 0 0 1 0 -31.831"
    fill="none" stroke="{fill_hex}" stroke-width="2" stroke-linecap="round"
    stroke-dasharray="{pct}, 100"/>
  <circle cx="17" cy="17" r="12.9155" fill="{center_hex}"/>
  <g transform="translate(11,11) scale(0.75)" fill="none" stroke="{fill_hex}"
     stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    {glyph}
  </g>
</svg>"##
    )
}

fn color_hex(color: Color) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        (color.r * 255.0).round() as u8,
        (color.g * 255.0).round() as u8,
        (color.b * 255.0).round() as u8,
        (color.a * 255.0).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_includes_percent_and_icon() {
        let svg = usage_ring(
            73.7,
            Color::from_rgb(1.0, 0.8, 0.0),
            Color::from_rgb(0.5, 0.5, 0.5),
            Color::from_rgb(0.1, 0.1, 0.1),
            RingIcon::Hammer,
        );
        assert!(svg.contains("stroke-dasharray=\"74, 100\""));
        assert!(svg.contains("M6.6 13.2"));
        assert!(!svg.contains("M8 5.5V3.3"));
    }

    #[test]
    fn ring_clamps_and_picks_bot_glyph() {
        let svg = usage_ring(
            140.0,
            Color::WHITE,
            Color::BLACK,
            Color::from_rgb(0.1, 0.1, 0.1),
            RingIcon::Bot,
        );
        assert!(svg.contains("stroke-dasharray=\"100, 100\""));
        assert!(svg.contains("M8 5.5V3.3"));
    }

    #[test]
    fn ring_picks_key_glyph() {
        let svg = usage_ring(
            40.0,
            Color::WHITE,
            Color::BLACK,
            Color::from_rgb(0.1, 0.1, 0.1),
            RingIcon::Key,
        );
        assert!(svg.contains("M8.5 8h5.2"));
        assert!(!svg.contains("M6.6 13.2"));
        assert!(!svg.contains("M8 5.5V3.3"));
    }

    #[test]
    fn usage_color_hits_band_stops() {
        let green = usage_color(0.0);
        assert!(green.g > green.r && green.g > green.b);
        let yellow = usage_color(50.0);
        assert!(yellow.r > 0.8 && yellow.g > 0.7 && yellow.b < 0.3);
        let orange = usage_color(80.0);
        assert!(orange.r > 0.8 && orange.g > 0.3 && orange.g < 0.6);
        let red = usage_color(90.0);
        assert!(red.r > 0.7 && red.g < 0.3);
        let dark = usage_color(100.0);
        assert!(dark.r < red.r && dark.g <= red.g);
    }

    #[test]
    fn usage_color_steps_each_percent() {
        let a = usage_color(23.0);
        let b = usage_color(24.0);
        assert!((a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs() > 0.001);
        assert_eq!(usage_color(23.4).r, usage_color(23.0).r);
        assert_eq!(usage_color(23.4).g, usage_color(23.0).g);
        assert_eq!(usage_color(23.4).b, usage_color(23.0).b);
    }

    #[test]
    fn ring_center_is_solid_and_has_no_translucent_wash() {
        let fill = Color::from_rgb(0.90, 0.16, 0.14);
        let track = Color::from_rgb(0.20, 0.21, 0.22);
        let center = Color {
            a: 0.12,
            ..Color::from_rgb(0.11, 0.12, 0.13)
        };
        let svg = usage_ring(40.0, fill, track, center, RingIcon::Hammer);
        let solid_center = Color { a: 1.0, ..center };
        assert!(svg.contains(&format!("fill=\"{}\"", color_hex(solid_center))));
        assert!(svg.contains(&color_hex(fill)));
        assert!(svg.contains(&color_hex(track)));
        assert!(!svg.contains("clip-path"));
        assert!(!svg.contains("bottom-half"));
        let dimmed = Color { a: 0.7, ..fill };
        let dimmed_svg = usage_ring(10.0, dimmed, track, solid_center, RingIcon::Key);
        assert!(dimmed_svg.contains(&color_hex(dimmed)));
    }
}
