//! Weekly usage-pace math for Build and Bot.
//!
//! Model: **constant burn from the period start**. The billing payload does not
//! include a separate recent rate, so used percent divided by time since the
//! period opened is the burn rate. Time until the remaining quota hits zero is
//! `remaining% / rate`. That is the same linear projection as “used % ÷ elapsed
//! fraction,” expressed as a clock time instead of a used percent at reset.
//!
//! Popup copy:
//! - quota hits zero **before** reset → `Will run out in 2d 4h`
//! - quota lasts until reset → `Won't run out before reset · ~N% left at reset`
//! - almost no time has passed and usage is still small → `Week just started`
//! - already at the cap → `Out of credits`; within half a percent → `At limit`
//! - weekly window with a non-positive start→end → `Pace unavailable`
//!
//! Period start comes from the billing payload when present. Otherwise (Bot, or
//! Build without a start) it is `next reset − 7 days`. Pace is only shown for a
//! weekly window: `period_type` contains `WEEKLY`, or start→end is 6–8 days.
//! API prepaid has no weekly window, so it is skipped. A missing reset hides
//! the line.

use chrono::{DateTime, Duration, Utc};

/// Elapsed fraction below this is the start of the week. A burn rate from a
/// shorter sample is noise unless usage is already material (see
/// [`EARLY_USAGE_FLOOR`]).
pub const EARLY_ELAPSED: f32 = 0.01;

/// Used percent below this, while still inside [`EARLY_ELAPSED`], does not
/// produce an exhaustion estimate.
pub const EARLY_USAGE_FLOOR: f32 = 5.0;

/// Used percent at or above this (and below 100) is treated as already at the cap.
pub const AT_LIMIT_USED: f32 = 99.5;

pub const WEEK: Duration = Duration::days(7);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaceStatus {
    /// Too little of the week has passed to trust a burn rate.
    Early,
    /// Remaining quota hits zero before reset, at this constant-burn ETA.
    Exhausts { eta: Duration },
    /// Constant burn does not empty the quota before reset.
    /// `left_percent` is the unused percent projected at reset (0–100).
    Lasts { left_percent: f32 },
    /// Within half a percent of the cap.
    AtLimit,
    /// Used percent is at the cap.
    OutOfCredits,
    /// Reset exists, but start→end is not a positive window.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pace {
    pub status: PaceStatus,
    pub elapsed_fraction: f32,
    pub used_percent: f32,
    /// Linear used percent at reset (`used / elapsed`), capped at 999.
    /// Zero when there is nothing to project.
    pub projected_percent: f32,
}

impl Pace {
    /// Popup caption, e.g. `Will run out in 2d 4h`.
    pub fn popup_line(&self) -> String {
        match self.status {
            PaceStatus::Early => "Week just started".into(),
            PaceStatus::Exhausts { eta } => format!("Will run out in {}", format_eta(eta)),
            PaceStatus::Lasts { left_percent } => format!(
                "Won't run out before reset · ~{:.0}% left at reset",
                left_percent.round()
            ),
            PaceStatus::AtLimit => "At limit".into(),
            PaceStatus::OutOfCredits => "Out of credits".into(),
            PaceStatus::Unavailable => "Pace unavailable".into(),
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

/// Time until remaining quota hits zero at a constant burn from `start`.
/// `None` when usage is zero, the clock is still at the start, or the cap
/// is already reached (there is no positive remaining quota to time).
pub fn exhaustion_eta(
    used_percent: f32,
    now: DateTime<Utc>,
    start: DateTime<Utc>,
) -> Option<Duration> {
    let used = clamp_percent(used_percent) as f64;
    if used <= 0.0 || used >= 100.0 {
        return None;
    }
    let elapsed_ms = (now - start).num_milliseconds();
    if elapsed_ms <= 0 {
        return None;
    }
    let eta_ms = ((100.0 - used) / used) * (elapsed_ms as f64);
    // A weekly reset is at most a few days out. Anything longer than a year
    // cannot land before reset, and it does not fit in a `Duration` we want
    // to format. Callers treat this as "does not exhaust before reset."
    let max_ms = Duration::days(366).num_milliseconds() as f64;
    if !eta_ms.is_finite() || eta_ms < 0.0 || eta_ms > max_ms {
        return None;
    }
    Some(Duration::milliseconds(eta_ms.round() as i64))
}

/// True when the constant-burn ETA is strictly before `resets_at`.
pub fn exhausts_before_reset(eta: Duration, now: DateTime<Utc>, resets_at: DateTime<Utc>) -> bool {
    eta < (resets_at - now)
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
    let projected = if (resets_at - start).num_milliseconds() <= 0 {
        0.0
    } else {
        projected_use(used, elapsed)
    };
    Pace {
        status: pace_status(used, elapsed, projected, now, start, resets_at),
        elapsed_fraction: elapsed,
        used_percent: used,
        projected_percent: projected,
    }
}

/// Pace when the window is weekly and a reset time exists.
/// Missing reset, or a non-weekly window, hides the line (`None`).
/// A weekly window whose start is not before its end is
/// [`PaceStatus::Unavailable`].
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

fn pace_status(
    used: f32,
    elapsed: f32,
    projected: f32,
    now: DateTime<Utc>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> PaceStatus {
    if (end - start).num_milliseconds() <= 0 {
        return PaceStatus::Unavailable;
    }
    if used >= 100.0 {
        return PaceStatus::OutOfCredits;
    }
    if used >= AT_LIMIT_USED {
        return PaceStatus::AtLimit;
    }
    let elapsed_ms = (now - start).num_milliseconds();
    if elapsed_ms <= 0 || (elapsed < EARLY_ELAPSED && used < EARLY_USAGE_FLOOR) {
        return PaceStatus::Early;
    }
    if used <= 0.0 {
        return PaceStatus::Lasts {
            left_percent: 100.0,
        };
    }
    // `None` here is a non-finite or absurdly long ETA (usage is already
    // known to be in (0, 100) with a positive elapsed time). That burn does
    // not empty the quota before a weekly reset.
    match exhaustion_eta(used, now, start) {
        Some(eta) if exhausts_before_reset(eta, now, end) => PaceStatus::Exhausts { eta },
        _ => PaceStatus::Lasts {
            left_percent: (100.0 - projected).clamp(0.0, 100.0),
        },
    }
}

/// Short ETA: `2d 4h`, `2d`, `3h 20m`, `21h`, `45m`, or `under 1m`.
/// Day-scale values round to the nearest hour. Shorter values keep minutes.
fn format_eta(eta: Duration) -> String {
    let secs = eta.num_seconds().max(0);
    if secs < 30 {
        return "under 1m".into();
    }
    let total_mins = (secs + 30) / 60;
    let days = total_mins / 1_440;
    if days > 0 {
        let mut hours = ((total_mins % 1_440) + 30) / 60;
        let mut days = days;
        if hours >= 24 {
            days += 1;
            hours = 0;
        }
        if hours == 0 {
            format!("{days}d")
        } else {
            format!("{days}d {hours}h")
        }
    } else {
        let hours = total_mins / 60;
        let mins = total_mins % 60;
        if hours > 0 && mins > 0 {
            format!("{hours}h {mins}m")
        } else if hours > 0 {
            format!("{hours}h")
        } else {
            format!("{mins}m")
        }
    }
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
    fn exhaustion_eta_scales_remaining_quota() {
        let (start, _) = week();
        let after_two_days = start + Duration::days(2);
        let eta = exhaustion_eta(48.0, after_two_days, start).unwrap();
        assert_eq!(eta, Duration::hours(52));

        let mid = start + Duration::days(3) + Duration::hours(12);
        let hot = exhaustion_eta(80.0, mid, start).unwrap();
        assert_eq!(hot, Duration::hours(21));

        assert!(exhaustion_eta(0.0, mid, start).is_none());
        assert!(exhaustion_eta(100.0, mid, start).is_none());
        assert!(exhaustion_eta(40.0, start, start).is_none());
    }

    #[test]
    fn exhausts_before_reset_is_strict() {
        let (start, end) = week();
        let mid = start + Duration::days(3) + Duration::hours(12);
        let exact = Duration::days(3) + Duration::hours(12);
        assert!(!exhausts_before_reset(exact, mid, end));
        assert!(exhausts_before_reset(
            exact - Duration::seconds(1),
            mid,
            end
        ));
        assert!(!exhausts_before_reset(Duration::days(4), mid, end));
    }

    #[test]
    fn will_run_out_before_reset() {
        let (start, end) = week();
        let now = start + Duration::days(2);
        let pace = weekly_pace(48.0, now, Some(start), end);
        assert_eq!(
            pace.status,
            PaceStatus::Exhausts {
                eta: Duration::hours(52)
            }
        );
        assert_eq!(pace.popup_line(), "Will run out in 2d 4h");
        assert!(!pace.popup_line().contains("On track"));
        assert!(!pace.popup_line().contains("of week elapsed"));

        let mid = start + Duration::days(3) + Duration::hours(12);
        let hot = weekly_pace(80.0, mid, Some(start), end);
        assert!((hot.projected_percent - 160.0).abs() < 0.2);
        assert_eq!(hot.popup_line(), "Will run out in 21h");

        let with_minutes = weekly_pace(75.0, start + Duration::hours(10), Some(start), end);
        assert_eq!(with_minutes.popup_line(), "Will run out in 3h 20m");

        let whole_days = weekly_pace(50.0, start + Duration::days(2), Some(start), end);
        assert_eq!(whole_days.popup_line(), "Will run out in 2d");
    }

    #[test]
    fn fast_early_burn_still_reports_eta() {
        let (start, end) = week();
        let now = start + Duration::minutes(30);
        let pace = weekly_pace(40.0, now, Some(start), end);
        assert!(pace.elapsed_fraction < EARLY_ELAPSED);
        assert_eq!(pace.popup_line(), "Will run out in 45m");
    }

    #[test]
    fn wont_run_out_before_reset() {
        let (start, end) = week();
        let mid = start + Duration::days(3) + Duration::hours(12);

        let even = weekly_pace(50.0, mid, Some(start), end);
        assert!((even.projected_percent - 100.0).abs() < 0.2);
        assert_eq!(
            even.popup_line(),
            "Won't run out before reset · ~0% left at reset"
        );

        let cool = weekly_pace(20.0, mid, Some(start), end);
        assert_eq!(cool.status, PaceStatus::Lasts { left_percent: 60.0 });
        assert!((cool.projected_percent - 40.0).abs() < 0.2);
        assert_eq!(
            cool.popup_line(),
            "Won't run out before reset · ~60% left at reset"
        );

        let unused = weekly_pace(0.0, mid, Some(start), end);
        assert_eq!(
            unused.popup_line(),
            "Won't run out before reset · ~100% left at reset"
        );

        // A trace of usage must not turn into a multi-week ETA string.
        let trace = weekly_pace(0.0001, mid, Some(start), end);
        assert!(matches!(trace.status, PaceStatus::Lasts { .. }));
        assert_eq!(
            trace.popup_line(),
            "Won't run out before reset · ~100% left at reset"
        );
    }

    #[test]
    fn at_limit_and_out_of_credits() {
        let (start, end) = week();
        let mid = start + Duration::days(3) + Duration::hours(12);
        assert_eq!(
            weekly_pace(100.0, mid, Some(start), end).popup_line(),
            "Out of credits"
        );
        assert_eq!(
            weekly_pace(100.0, start + Duration::minutes(5), Some(start), end).popup_line(),
            "Out of credits"
        );
        assert_eq!(
            weekly_pace(99.5, mid, Some(start), end).popup_line(),
            "At limit"
        );
        assert_eq!(
            weekly_pace(99.9, mid, Some(start), end).popup_line(),
            "At limit"
        );
        let almost = weekly_pace(99.4, mid, Some(start), end);
        assert!(almost.popup_line().starts_with("Will run out in "));
    }

    #[test]
    fn early_week_stays_calm() {
        let (start, end) = week();
        let now = start + Duration::minutes(30);
        let unused = weekly_pace(0.0, now, Some(start), end);
        assert!(unused.elapsed_fraction < EARLY_ELAPSED);
        assert_eq!(unused.status, PaceStatus::Early);
        assert_eq!(unused.popup_line(), "Week just started");

        let trickle = weekly_pace(1.0, now, Some(start), end);
        assert_eq!(trickle.popup_line(), "Week just started");

        let before = weekly_pace(2.0, start - Duration::hours(1), Some(start), end);
        assert_eq!(before.popup_line(), "Week just started");
    }

    #[test]
    fn tiny_eta_is_under_a_minute() {
        let (start, end) = week();
        // 99.4% used over ~55 minutes → about 20 seconds of quota left.
        let now = start + Duration::seconds(3_313);
        let pace = weekly_pace(99.4, now, Some(start), end);
        assert_eq!(pace.popup_line(), "Will run out in under 1m");
    }

    #[test]
    fn infers_start_from_reset_minus_seven_days() {
        let end = ts(2026, 8, 20, 19);
        let mid = ts(2026, 8, 17, 7);
        let pace = weekly_pace(50.0, mid, None, end);
        assert!((pace.elapsed_fraction - 0.5).abs() < 0.001);
        assert_eq!(weekly_start(None, end), end - WEEK);
        assert_eq!(
            pace.popup_line(),
            "Won't run out before reset · ~0% left at reset"
        );
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
    fn broken_weekly_window_is_unavailable() {
        let end = ts(2026, 8, 20, 19);
        let pace = maybe_weekly_pace(40.0, end, Some(end), Some(end), Some("WEEKLY")).unwrap();
        assert_eq!(pace.status, PaceStatus::Unavailable);
        assert_eq!(pace.popup_line(), "Pace unavailable");
    }

    #[test]
    fn format_eta_rounds_day_scale_to_hours() {
        assert_eq!(format_eta(Duration::hours(52)), "2d 4h");
        assert_eq!(format_eta(Duration::hours(48)), "2d");
        assert_eq!(
            format_eta(Duration::hours(51) + Duration::minutes(40)),
            "2d 4h"
        );
        assert_eq!(format_eta(Duration::minutes(45)), "45m");
        assert_eq!(format_eta(Duration::seconds(20)), "under 1m");
        assert_eq!(format_eta(Duration::seconds(45)), "1m");
    }
}
