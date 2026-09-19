//! Weekly usage-pace math for Build and Bot.
//!
//! Model: constant burn from the **period start** to now. If `U`% of quota is
//! used after fraction `E` of the week, the even-burn line is `E * 100` and the
//! projected used percent at reset is `U / E`.
//!
//! Classification (percentage-point band around the even-burn line):
//! - **On track** when `|used% − elapsed%| ≤ 10`
//! - **Ahead of pace** when used% is more than 10 points above elapsed%
//! - **Behind pace** when used% is more than 10 points below elapsed%
//!
//! Period start comes from the billing payload when present. Otherwise (Bot, or
//! Build without a start) it is `next reset − 7 days`. Pace is only shown for a
//! weekly window: `period_type` contains `WEEKLY`, or start→end is 6–8 days.
//! API prepaid has no weekly window, so it is skipped.

use chrono::{DateTime, Duration, Utc};

/// Percentage-point band around the even-burn line that still counts as on track.
pub const ON_TRACK_BAND: f32 = 10.0;

/// Elapsed fraction below this is the start of the week; projection is omitted.
pub const EARLY_ELAPSED: f32 = 0.01;

pub const WEEK: Duration = Duration::days(7);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaceKind {
    OnTrack,
    Ahead,
    Behind,
}

impl PaceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::OnTrack => "On track",
            Self::Ahead => "Ahead of pace",
            Self::Behind => "Behind pace",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pace {
    pub kind: PaceKind,
    pub elapsed_fraction: f32,
    pub used_percent: f32,
    pub projected_percent: f32,
}

impl Pace {
    pub fn elapsed_percent(&self) -> f32 {
        self.elapsed_fraction * 100.0
    }

    /// Popup caption, e.g. `On track · 52% of week elapsed · ~44% at reset`.
    pub fn popup_line(&self) -> String {
        if self.elapsed_fraction < EARLY_ELAPSED {
            format!("{} · week just started", self.kind.label())
        } else {
            format!(
                "{} · {:.0}% of week elapsed · ~{:.0}% at reset",
                self.kind.label(),
                self.elapsed_percent().round(),
                self.projected_percent.round()
            )
        }
    }
}

pub fn weekly_start(starts_at: Option<DateTime<Utc>>, resets_at: DateTime<Utc>) -> DateTime<Utc> {
    starts_at.unwrap_or(resets_at - WEEK)
}

pub fn elapsed_fraction(now: DateTime<Utc>, start: DateTime<Utc>, end: DateTime<Utc>) -> f32 {
    let total = (end - start).num_milliseconds() as f64;
    if total <= 0.0 {
        return 1.0;
    }
    let elapsed = (now - start).num_milliseconds() as f64;
    (elapsed / total).clamp(0.0, 1.0) as f32
}

/// Linear used% at reset, assuming constant burn from period start.
/// Unclamped above 100 so an overshoot stays visible. Early in the week,
/// a non-zero used% projects to 100 (rate is undefined at elapsed 0).
pub fn projected_use(used_percent: f32, elapsed: f32) -> f32 {
    let used = clamp_percent(used_percent);
    if elapsed <= 0.0 {
        return if used <= 0.0 { 0.0 } else { 100.0 };
    }
    (used / elapsed).min(999.0)
}

pub fn classify(used_percent: f32, elapsed: f32) -> PaceKind {
    let used = clamp_percent(used_percent);
    let elapsed_pct = elapsed.clamp(0.0, 1.0) * 100.0;
    let delta = used - elapsed_pct;
    if delta > ON_TRACK_BAND {
        PaceKind::Ahead
    } else if delta < -ON_TRACK_BAND {
        PaceKind::Behind
    } else {
        PaceKind::OnTrack
    }
}

pub fn is_weekly_period(
    period_type: Option<&str>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> bool {
    if let Some(t) = period_type {
        if t.contains("WEEKLY") {
            return true;
        }
        if t.contains("MONTHLY") || t.contains("DAILY") {
            return false;
        }
    }
    let days = (end - start).num_days();
    (6..=8).contains(&days)
}

pub fn weekly_pace(
    used_percent: f32,
    now: DateTime<Utc>,
    starts_at: Option<DateTime<Utc>>,
    resets_at: DateTime<Utc>,
) -> Pace {
    let start = weekly_start(starts_at, resets_at);
    let elapsed = elapsed_fraction(now, start, resets_at);
    let used = clamp_percent(used_percent);
    Pace {
        kind: classify(used, elapsed),
        elapsed_fraction: elapsed,
        used_percent: used,
        projected_percent: projected_use(used, elapsed),
    }
}

/// Pace when the window is weekly and a reset time exists.
pub fn maybe_weekly_pace(
    used_percent: f32,
    now: DateTime<Utc>,
    starts_at: Option<DateTime<Utc>>,
    resets_at: Option<DateTime<Utc>>,
    period_type: Option<&str>,
) -> Option<Pace> {
    let end = resets_at?;
    let start = weekly_start(starts_at, end);
    if !is_weekly_period(period_type, start, end) {
        return None;
    }
    Some(weekly_pace(used_percent, now, Some(start), end))
}

fn clamp_percent(p: f32) -> f32 {
    if !p.is_finite() {
        0.0
    } else {
        p.clamp(0.0, 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .unwrap()
    }

    fn week() -> (DateTime<Utc>, DateTime<Utc>) {
        (ts(2026, 8, 13, 19), ts(2026, 8, 20, 19))
    }

    #[test]
    fn elapsed_mid_week_is_half() {
        let (start, end) = week();
        let now = ts(2026, 8, 17, 7);
        let elapsed = elapsed_fraction(now, start, end);
        assert!((elapsed - 0.5).abs() < 0.001, "{elapsed}");
    }

    #[test]
    fn elapsed_clamps_before_start_and_after_end() {
        let (start, end) = week();
        assert_eq!(
            elapsed_fraction(start - Duration::hours(2), start, end),
            0.0
        );
        assert_eq!(elapsed_fraction(end + Duration::hours(2), start, end), 1.0);
        assert_eq!(elapsed_fraction(start, start, end), 0.0);
        assert_eq!(elapsed_fraction(end, start, end), 1.0);
    }

    #[test]
    fn elapsed_bad_window_is_complete() {
        let t = ts(2026, 8, 13, 19);
        assert_eq!(elapsed_fraction(t, t, t), 1.0);
        assert_eq!(elapsed_fraction(t, t + Duration::days(1), t), 1.0);
    }

    #[test]
    fn projected_use_from_period_start() {
        assert!((projected_use(50.0, 0.5) - 100.0).abs() < f32::EPSILON);
        assert!((projected_use(80.0, 0.5) - 160.0).abs() < f32::EPSILON);
        assert!((projected_use(20.0, 0.5) - 40.0).abs() < f32::EPSILON);
        assert_eq!(projected_use(0.0, 0.0), 0.0);
        assert_eq!(projected_use(5.0, 0.0), 100.0);
        assert_eq!(projected_use(2.0, 0.001), 999.0);
    }

    #[test]
    fn classify_band_is_ten_points() {
        assert_eq!(classify(50.0, 0.5), PaceKind::OnTrack);
        assert_eq!(classify(60.0, 0.5), PaceKind::OnTrack);
        assert_eq!(classify(40.0, 0.5), PaceKind::OnTrack);
        assert_eq!(classify(80.0, 0.5), PaceKind::Ahead);
        assert_eq!(classify(20.0, 0.5), PaceKind::Behind);
        assert_eq!(classify(60.1, 0.5), PaceKind::Ahead);
        assert_eq!(classify(39.9, 0.5), PaceKind::Behind);
        assert_eq!(classify(0.0, 0.0), PaceKind::OnTrack);
        assert_eq!(classify(15.0, 0.0), PaceKind::Ahead);
    }

    #[test]
    fn weekly_pace_examples() {
        let (start, end) = week();
        let mid = ts(2026, 8, 17, 7);
        let on_track = weekly_pace(50.0, mid, Some(start), end);
        assert_eq!(on_track.kind, PaceKind::OnTrack);
        assert!((on_track.elapsed_percent() - 50.0).abs() < 0.1);
        assert!((on_track.projected_percent - 100.0).abs() < 0.2);
        assert_eq!(
            on_track.popup_line(),
            "On track · 50% of week elapsed · ~100% at reset"
        );

        let hot = weekly_pace(80.0, mid, Some(start), end);
        assert_eq!(hot.kind, PaceKind::Ahead);
        assert!((hot.projected_percent - 160.0).abs() < 0.2);
        assert_eq!(
            hot.popup_line(),
            "Ahead of pace · 50% of week elapsed · ~160% at reset"
        );

        let cool = weekly_pace(20.0, mid, Some(start), end);
        assert_eq!(cool.kind, PaceKind::Behind);
        assert!((cool.projected_percent - 40.0).abs() < 0.2);
        assert_eq!(
            cool.popup_line(),
            "Behind pace · 50% of week elapsed · ~40% at reset"
        );
    }

    #[test]
    fn infers_start_from_reset_minus_seven_days() {
        let end = ts(2026, 8, 20, 19);
        let mid = ts(2026, 8, 17, 7);
        let pace = weekly_pace(50.0, mid, None, end);
        assert!((pace.elapsed_fraction - 0.5).abs() < 0.001);
        assert_eq!(weekly_start(None, end), end - WEEK);
    }

    #[test]
    fn early_week_omits_noisy_projection() {
        let (start, end) = week();
        let now = start + Duration::minutes(30);
        let pace = weekly_pace(0.0, now, Some(start), end);
        assert!(pace.elapsed_fraction < EARLY_ELAPSED);
        assert_eq!(pace.kind, PaceKind::OnTrack);
        assert_eq!(pace.popup_line(), "On track · week just started");
    }

    #[test]
    fn maybe_weekly_requires_reset_and_weekly_window() {
        let (start, end) = week();
        let now = ts(2026, 8, 17, 7);
        assert!(
            maybe_weekly_pace(
                10.0,
                now,
                Some(start),
                Some(end),
                Some("USAGE_PERIOD_TYPE_WEEKLY")
            )
            .is_some()
        );
        assert!(
            maybe_weekly_pace(
                10.0,
                now,
                Some(start),
                Some(end),
                Some("USAGE_PERIOD_TYPE_MONTHLY")
            )
            .is_none()
        );
        assert!(maybe_weekly_pace(10.0, now, Some(start), None, Some("WEEKLY")).is_none());
        let month_end = start + Duration::days(30);
        assert!(maybe_weekly_pace(10.0, now, Some(start), Some(month_end), None).is_none());
        assert!(maybe_weekly_pace(10.0, now, None, Some(end), None).is_some());
    }

    #[test]
    fn labels() {
        assert_eq!(PaceKind::OnTrack.label(), "On track");
        assert_eq!(PaceKind::Ahead.label(), "Ahead of pace");
        assert_eq!(PaceKind::Behind.label(), "Behind pace");
    }
}
