//! Quiet refresh of one CLI home's access token.
//!
//! The Grok CLI stores a short-lived access token (`expires_at`) and a
//! refresh token in `<home>/auth.json`. It renews the access token when it
//! runs. `grok models` is an unattended command that boots that auth path.
//! `grok usage` only prints saved session cost and does not.
//!
//! This module never runs `grok login` or `grok logout`, never reads the
//! refresh token, and never puts `GROK_HOME` on the panel process. The child
//! gets `GROK_HOME` for one home, with no TTY. Tokens and emails are not logged.

use crate::auth;
use crate::login::CliInvocation;
use crate::spawn::{self, QuietStop};
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Same window the CLI uses by default (`GROK_AUTH_EARLY_INVALIDATION_SECS`).
pub const EARLY_WITHIN: chrono::Duration = chrono::Duration::seconds(300);

/// Longer than the CLI's cross-process refresh lock (about 45 seconds).
pub const RENEW_TIMEOUT: Duration = Duration::from_secs(60);

const BACKOFF_BASE: Duration = Duration::from_secs(5 * 60);
const BACKOFF_CAP: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackoffState {
    pub failures: u32,
    pub not_before: Option<DateTime<Utc>>,
}

impl BackoffState {
    pub fn allows(&self, now: DateTime<Utc>) -> bool {
        self.not_before.is_none_or(|at| now >= at)
    }

    pub fn note_failure(&mut self, now: DateTime<Utc>) {
        self.failures = self.failures.saturating_add(1);
        let delay = chrono::Duration::from_std(backoff_after(self.failures))
            .unwrap_or(chrono::Duration::seconds(6 * 60 * 60));
        self.not_before = Some(now + delay);
    }

    pub fn note_success(&mut self) {
        self.failures = 0;
        self.not_before = None;
    }
}

/// 5 minutes, doubling, capped at 6 hours.
pub fn backoff_after(failures: u32) -> Duration {
    let shift = failures.saturating_sub(1).min(16);
    BACKOFF_BASE
        .checked_mul(1u32 << shift)
        .unwrap_or(BACKOFF_CAP)
        .min(BACKOFF_CAP)
}

/// One configured icon, already classified. `home_key` is that icon's CLI home.
#[derive(Debug, Clone)]
pub struct RenewInput {
    pub home_key: PathBuf,
    pub busy: bool,
    pub due: bool,
    pub backoff: BackoffState,
}

/// Indexes of homes that should be renewed now. At most one index per home.
/// A busy icon (sign-in or sign-out) blocks every icon that shares its home.
pub fn renewal_indexes(inputs: &[RenewInput], now: DateTime<Utc>) -> Vec<usize> {
    let mut blocked = HashSet::new();
    for input in inputs {
        if input.busy {
            blocked.insert(input.home_key.clone());
        }
    }
    let mut chosen = HashSet::new();
    let mut indexes = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        if blocked.contains(&input.home_key) || !chosen.insert(input.home_key.clone()) {
            continue;
        }
        if input.due && input.backoff.allows(now) {
            indexes.push(index);
        }
    }
    indexes
}

pub fn session_needs_renewal(home: &Path, now: DateTime<Utc>) -> bool {
    auth::session_needs_renewal(home, now, EARLY_WITHIN)
}

#[cfg(test)]
fn auth_needs_renewal(raw: &str, now: DateTime<Utc>) -> bool {
    auth::auth_json_needs_renewal(raw, now, EARLY_WITHIN)
}

pub fn models_invocation(home: &Path, program: &Path) -> CliInvocation {
    CliInvocation {
        program: program.to_path_buf(),
        args: vec!["models".into()],
        grok_home: home.to_path_buf(),
    }
}

#[derive(Debug, Clone)]
pub struct RenewJob {
    pub index: usize,
    pub home: PathBuf,
    pub grok_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewReport {
    pub index: usize,
    pub home: PathBuf,
    pub result: RenewResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenewResult {
    Renewed,
    Failed { reason: FailReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailReason {
    CliNotFound,
    Spawn,
    TimedOut,
    Exit { code: Option<i32> },
}

impl FailReason {
    /// Text that may be logged. Exit code or timeout only. No tokens or emails.
    pub fn log_detail(&self) -> String {
        match self {
            Self::CliNotFound | Self::Spawn => "could not start grok".into(),
            Self::TimedOut => "timed out".into(),
            Self::Exit { code: Some(code) } => format!("exit code {code}"),
            Self::Exit { code: None } => "exit code unknown".into(),
        }
    }
}

pub fn log_renewal(index: usize, home: &Path, result: &RenewResult) {
    let home = home.display().to_string();
    tracing::info!(index, home = %home, "session renewal attempted");
    match result {
        RenewResult::Renewed => {
            tracing::info!(index, home = %home, "session renewal succeeded");
        }
        RenewResult::Failed { reason } => {
            let why = reason.log_detail();
            tracing::warn!(index, home = %home, why = %why, "session renewal failed");
        }
    }
}

pub async fn renew_all(jobs: Vec<RenewJob>) -> Vec<RenewReport> {
    let mut tasks = Vec::with_capacity(jobs.len());
    for job in jobs {
        tasks.push(tokio::task::spawn_blocking(move || {
            let result = renew_configured(&job.home, &job.grok_path, RENEW_TIMEOUT);
            RenewReport {
                index: job.index,
                home: job.home,
                result,
            }
        }));
    }
    let mut reports = Vec::with_capacity(tasks.len());
    for task in tasks {
        if let Ok(report) = task.await {
            reports.push(report);
        }
    }
    reports
}

pub fn renew_configured(home: &Path, grok_path: &str, timeout: Duration) -> RenewResult {
    match crate::grok_cli::resolve_from_env(grok_path) {
        Ok(program) => renew_with(&program, home, timeout),
        Err(_) => RenewResult::Failed {
            reason: FailReason::CliNotFound,
        },
    }
}

/// Run `grok models` for `home` and re-read `auth.json`.
///
/// Success is a session that is no longer due, not merely exit code 0.
/// A command that exits 0 and leaves the access token inside the window is a
/// failure, so the caller can back off instead of spawning on every poll.
pub fn renew_with(program: &Path, home: &Path, timeout: Duration) -> RenewResult {
    let stop = match run_models(program, home, timeout) {
        Ok(stop) => stop,
        Err(_) => {
            return RenewResult::Failed {
                reason: FailReason::Spawn,
            };
        }
    };
    if session_is_fresh(home, Utc::now()) {
        return RenewResult::Renewed;
    }
    let reason = match stop {
        QuietStop::TimedOut => FailReason::TimedOut,
        QuietStop::Exited(status) => FailReason::Exit {
            code: status.code(),
        },
    };
    RenewResult::Failed { reason }
}

fn session_is_fresh(home: &Path, now: DateTime<Utc>) -> bool {
    auth::load_bearer_from(home).is_ok() && !session_needs_renewal(home, now)
}

fn run_models(program: &Path, home: &Path, timeout: Duration) -> Result<QuietStop, std::io::Error> {
    let invocation = models_invocation(home, program);
    let mut cmd = Command::new(&invocation.program);
    invocation.apply(&mut cmd);
    spawn::run_quiet(&mut cmd, timeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

    fn at(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn auth(expires: &str) -> String {
        format!(r#"{{"https://auth.x.ai::id":{{"key":"fixture-token","expires_at":"{expires}"}}}}"#)
    }

    #[test]
    fn near_expiry_and_past_expiry_are_due_and_a_later_token_is_not() {
        let now = at("2026-06-01T00:00:00Z");
        assert!(auth_needs_renewal(&auth("2026-06-01T00:04:00Z"), now));
        assert!(auth_needs_renewal(&auth("2026-06-01T00:05:00Z"), now));
        assert!(!auth_needs_renewal(&auth("2026-06-01T00:05:01Z"), now));
        assert!(auth_needs_renewal(&auth("2026-05-31T00:00:00Z"), now));
        assert!(!auth_needs_renewal(&auth("2026-06-01T12:00:00Z"), now));
    }

    #[test]
    fn missing_invalid_and_undated_sessions_are_not_renewed() {
        let now = at("2026-06-01T00:00:00Z");
        assert!(!auth_needs_renewal("", now));
        assert!(!auth_needs_renewal("{", now));
        assert!(!auth_needs_renewal("{}", now));
        assert!(!auth_needs_renewal(
            r#"{"https://auth.x.ai::id":{"key":"fixture-token"}}"#,
            now
        ));
        assert!(!auth_needs_renewal(
            r#"{"https://auth.x.ai::id":{"key":"","expires_at":"2000-01-01T00:00:00Z"}}"#,
            now
        ));
    }

    #[test]
    fn a_live_fallback_is_not_due_when_the_xai_entry_is_expired() {
        let now = at("2026-06-01T00:00:00Z");
        let raw = r#"{
            "https://auth.x.ai::stale": {"key":"fixture-token","expires_at":"2000-01-01T00:00:00Z"},
            "https://cli.example::": {"key":"fixture-token","expires_at":"2099-01-01T00:00:00Z"}
        }"#;
        assert!(!auth_needs_renewal(raw, now));
        assert!(auth_needs_renewal(
            r#"{"https://auth.x.ai::stale":{"key":"fixture-token","expires_at":"2000-01-01T00:00:00Z"}}"#,
            now
        ));
    }

    #[test]
    fn backoff_doubles_until_six_hours_and_success_clears_it() {
        assert_eq!(backoff_after(1), Duration::from_secs(5 * 60));
        assert_eq!(backoff_after(2), Duration::from_secs(10 * 60));
        assert_eq!(backoff_after(3), Duration::from_secs(20 * 60));
        assert_eq!(backoff_after(4), Duration::from_secs(40 * 60));
        assert_eq!(backoff_after(20), BACKOFF_CAP);

        let now = at("2026-06-01T00:00:00Z");
        let mut state = BackoffState::default();
        assert!(state.allows(now));
        state.note_failure(now);
        assert_eq!(state.failures, 1);
        assert!(!state.allows(now));
        assert!(!state.allows(now + chrono::Duration::minutes(4)));
        assert!(state.allows(now + chrono::Duration::minutes(5)));
        state.note_failure(now);
        assert!(!state.allows(now + chrono::Duration::minutes(9)));
        assert!(state.allows(now + chrono::Duration::minutes(10)));
        state.note_success();
        assert_eq!(state, BackoffState::default());
        assert!(state.allows(now));
    }

    #[test]
    fn selector_runs_one_due_home_and_honors_backoff_and_sign_in() {
        let now = at("2026-06-01T00:00:00Z");
        let fresh = RenewInput {
            home_key: PathBuf::from("/homes/fresh"),
            busy: false,
            due: false,
            backoff: BackoffState::default(),
        };
        let due = RenewInput {
            home_key: PathBuf::from("/homes/due"),
            busy: false,
            due: true,
            backoff: BackoffState::default(),
        };
        assert_eq!(renewal_indexes(&[fresh, due], now), vec![1]);

        let mut waiting = BackoffState::default();
        waiting.note_failure(now);
        let held = RenewInput {
            home_key: PathBuf::from("/homes/due"),
            busy: false,
            due: true,
            backoff: waiting,
        };
        let other = RenewInput {
            home_key: PathBuf::from("/homes/other"),
            busy: false,
            due: true,
            backoff: BackoffState::default(),
        };
        assert_eq!(renewal_indexes(&[held.clone(), other], now), vec![1]);

        let second_same_home = RenewInput {
            home_key: PathBuf::from("/homes/due"),
            busy: false,
            due: true,
            backoff: BackoffState::default(),
        };
        assert_eq!(
            renewal_indexes(&[held, second_same_home], now),
            Vec::<usize>::new()
        );

        let signing_in = RenewInput {
            home_key: PathBuf::from("/homes/due"),
            busy: true,
            due: true,
            backoff: BackoffState::default(),
        };
        let idle_same = RenewInput {
            home_key: PathBuf::from("/homes/due"),
            busy: false,
            due: true,
            backoff: BackoffState::default(),
        };
        assert!(renewal_indexes(&[signing_in, idle_same], now).is_empty());
    }

    #[test]
    fn models_invocation_is_not_login_or_logout() {
        let home = Path::new("/icons/pers/.grok-personal");
        let invocation = models_invocation(home, Path::new("/usr/bin/grok"));
        assert_eq!(invocation.args, ["models"]);
        assert_eq!(invocation.grok_home, home);
        assert!(invocation.args.iter().all(|arg| arg != "login"));
        assert!(invocation.args.iter().all(|arg| arg != "logout"));
        assert!(invocation.args.iter().all(|arg| arg != "--device-code"));
    }

    #[test]
    fn failure_text_is_an_exit_code_or_a_timeout() {
        let reasons = [
            FailReason::CliNotFound,
            FailReason::Spawn,
            FailReason::TimedOut,
            FailReason::Exit { code: Some(2) },
            FailReason::Exit { code: None },
        ];
        assert_eq!(reasons[2].log_detail(), "timed out");
        assert_eq!(reasons[3].log_detail(), "exit code 2");
        assert_eq!(reasons[4].log_detail(), "exit code unknown");
        assert_eq!(reasons[0].log_detail(), "could not start grok");
        for reason in &reasons {
            let text = reason.log_detail();
            assert!(!text.contains('@'), "{text}");
            assert!(!text.to_ascii_lowercase().contains("token"), "{text}");
            assert!(!text.contains("fixture"), "{text}");
        }
    }

    #[test]
    fn child_gets_only_its_home_and_no_tty() {
        let _guard = crate::login::PROCESS_ENV_LOCK.lock().unwrap();
        let previous_home = std::env::var_os("GROK_HOME");
        let previous_file = std::env::var_os("GROK_CREDENTIALS_FILE");
        let previous_out = std::env::var_os("GROK_MON_TEST_OUT");
        let root = temp_dir("env");
        let home = root.join("chip");
        let out = root.join("out");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&out).unwrap();
        fs::write(home.join("auth.json"), auth("2000-01-01T00:00:00Z")).unwrap();
        let program = write_script(
            &root.join("grok"),
            r#"#!/bin/sh
printf '%s\n' "$#" "$1" > "$GROK_MON_TEST_OUT/args"
printf '%s\n' "$GROK_HOME" > "$GROK_MON_TEST_OUT/home"
if [ -t 0 ]; then echo tty; else echo notty; fi > "$GROK_MON_TEST_OUT/tty"
if [ -n "${GROK_CREDENTIALS_FILE+x}" ]; then echo set; else echo unset; fi > "$GROK_MON_TEST_OUT/cred"
exit 0
"#,
        );
        // SAFETY: tests serialize environment changes with PROCESS_ENV_LOCK.
        unsafe { std::env::set_var("GROK_HOME", "/parent/grok-home") };
        unsafe { std::env::set_var("GROK_CREDENTIALS_FILE", "/parent/credentials.json") };
        unsafe { std::env::set_var("GROK_MON_TEST_OUT", &out) };

        let result = renew_with(&program, &home, Duration::from_secs(5));

        assert_eq!(
            result,
            RenewResult::Failed {
                reason: FailReason::Exit { code: Some(0) }
            }
        );
        assert_eq!(fs::read_to_string(out.join("args")).unwrap(), "1\nmodels\n");
        assert_eq!(
            fs::read_to_string(out.join("home")).unwrap().trim(),
            home.to_str().unwrap()
        );
        assert_eq!(fs::read_to_string(out.join("tty")).unwrap().trim(), "notty");
        assert_eq!(
            fs::read_to_string(out.join("cred")).unwrap().trim(),
            "unset"
        );
        assert_eq!(std::env::var("GROK_HOME").unwrap(), "/parent/grok-home");
        assert_eq!(
            std::env::var("GROK_CREDENTIALS_FILE").unwrap(),
            "/parent/credentials.json"
        );
        assert!(session_needs_renewal(&home, Utc::now()));

        restore("GROK_HOME", previous_home);
        restore("GROK_CREDENTIALS_FILE", previous_file);
        restore("GROK_MON_TEST_OUT", previous_out);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_script_that_extends_expiry_counts_as_renewed() {
        let root = temp_dir("refresh");
        let home = root.join("chip");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("auth.json"), auth("2000-01-01T00:00:00Z")).unwrap();
        assert!(session_needs_renewal(&home, Utc::now()));
        let program = write_script(
            &root.join("grok"),
            r#"#!/bin/sh
cat > "$GROK_HOME/auth.json" <<'EOF'
{"https://auth.x.ai::id":{"key":"fixture-token","expires_at":"2099-01-01T00:00:00Z"}}
EOF
exit 0
"#,
        );
        let result = renew_with(&program, &home, Duration::from_secs(5));
        assert_eq!(result, RenewResult::Renewed);
        assert!(!session_needs_renewal(&home, Utc::now()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn timeout_stops_the_child_and_its_group() {
        let _guard = crate::login::PROCESS_ENV_LOCK.lock().unwrap();
        let previous_out = std::env::var_os("GROK_MON_TEST_OUT");
        let root = temp_dir("timeout");
        let home = root.join("chip");
        let out = root.join("out");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&out).unwrap();
        fs::write(home.join("auth.json"), auth("2000-01-01T00:00:00Z")).unwrap();
        let program = write_script(
            &root.join("grok"),
            r#"#!/bin/sh
sleep 30 &
echo $! > "$GROK_MON_TEST_OUT/grand.pid"
sleep 30
"#,
        );
        // SAFETY: tests serialize environment changes with PROCESS_ENV_LOCK.
        unsafe { std::env::set_var("GROK_MON_TEST_OUT", &out) };
        let started = Instant::now();
        let result = renew_with(&program, &home, Duration::from_millis(500));
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "timeout waited {elapsed:?}"
        );
        assert_eq!(
            result,
            RenewResult::Failed {
                reason: FailReason::TimedOut
            }
        );
        let pid = fs::read_to_string(out.join("grand.pid")).unwrap();
        let pid = pid.trim();
        let still = Command::new("kill")
            .args(["-0", "--", pid])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        let _ = Command::new("kill")
            .args(["-KILL", "--", pid])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        assert!(
            !still.success(),
            "grandchild {pid} was still running after the timeout"
        );
        assert!(session_needs_renewal(&home, Utc::now()));
        restore("GROK_MON_TEST_OUT", previous_out);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_cli_does_not_change_grok_home() {
        let _guard = crate::login::PROCESS_ENV_LOCK.lock().unwrap();
        let previous_path = std::env::var_os("PATH");
        let previous_home_env = std::env::var_os("HOME");
        let previous_grok = std::env::var_os("GROK_HOME");
        let root = temp_dir("missing-cli");
        let path_dir = root.join("path");
        let user_home = root.join("user");
        let chip = root.join("chip");
        fs::create_dir_all(&path_dir).unwrap();
        fs::create_dir_all(&user_home).unwrap();
        fs::create_dir_all(&chip).unwrap();
        // SAFETY: tests serialize environment changes with PROCESS_ENV_LOCK.
        unsafe { std::env::set_var("PATH", &path_dir) };
        unsafe { std::env::set_var("HOME", &user_home) };
        unsafe { std::env::set_var("GROK_HOME", "/parent/grok-home") };

        let result = renew_configured(&chip, "", Duration::from_millis(200));
        assert_eq!(
            result,
            RenewResult::Failed {
                reason: FailReason::CliNotFound
            }
        );
        assert_eq!(std::env::var("GROK_HOME").unwrap(), "/parent/grok-home");

        restore("PATH", previous_path);
        restore("HOME", previous_home_env);
        restore("GROK_HOME", previous_grok);
        let _ = fs::remove_dir_all(&root);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "grok-mon-renew-{name}-{}-{nanos}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_script(path: &Path, body: &str) -> PathBuf {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
        path.to_path_buf()
    }

    fn restore(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            // SAFETY: tests serialize environment changes with PROCESS_ENV_LOCK.
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
    }
}
