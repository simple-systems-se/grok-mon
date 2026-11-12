//! Device-code sign-in and sign-out for one icon's CLI home.
//!
//! The child process is the only place `GROK_HOME` is set. The panel process
//! is left alone. Sign-in always passes `--device-code` (the documented alias
//! of `--device-auth`). A plain `grok login` or `--oauth` would open the
//! system browser. Sign-out is `grok logout` with that same home.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

pub const GROK_PROGRAM: &str = "grok";

/// Held by tests that mutate process environment variables.
#[cfg(test)]
pub static PROCESS_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliInvocation {
    pub program: String,
    pub args: Vec<String>,
    pub grok_home: PathBuf,
}

impl CliInvocation {
    pub fn apply(&self, cmd: &mut Command) {
        cmd.args(&self.args);
        scope_command(cmd, &self.grok_home);
    }
}

pub fn device_login_invocation(home: &Path) -> CliInvocation {
    CliInvocation {
        program: GROK_PROGRAM.into(),
        args: vec!["login".into(), "--device-code".into()],
        grok_home: home.to_path_buf(),
    }
}

pub fn logout_invocation(home: &Path) -> CliInvocation {
    CliInvocation {
        program: GROK_PROGRAM.into(),
        args: vec!["logout".into()],
        grok_home: home.to_path_buf(),
    }
}

pub fn scope_command(cmd: &mut Command, home: &Path) {
    cmd.env("GROK_HOME", home);
    cmd.env_remove("GROK_CREDENTIALS_FILE");
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DevicePrompt {
    pub verification_url: Option<String>,
    pub user_code: Option<String>,
}

impl DevicePrompt {
    pub fn is_complete(&self) -> bool {
        self.verification_url
            .as_ref()
            .is_some_and(|url| url.starts_with("https://"))
            && self
                .user_code
                .as_ref()
                .is_some_and(|code| is_user_code(code))
    }

    fn merge(&mut self, next: &DevicePrompt) {
        if next.verification_url.is_some() {
            self.verification_url.clone_from(&next.verification_url);
        }
        if next.user_code.is_some() {
            self.user_code.clone_from(&next.user_code);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoginPhase {
    #[default]
    Idle,
    Starting,
    Waiting,
    SigningOut,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoginUiState {
    pub phase: LoginPhase,
    pub prompt: DevicePrompt,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginUpdate {
    Started,
    Prompt(DevicePrompt),
    SessionReady,
    Exited { success: bool, message: String },
    Failed(String),
    Canceled,
    SigningOut,
    SignedOut,
}

impl LoginUiState {
    pub fn is_in_progress(&self) -> bool {
        matches!(
            self.phase,
            LoginPhase::Starting | LoginPhase::Waiting | LoginPhase::SigningOut
        )
    }

    pub fn apply(&mut self, update: LoginUpdate) {
        match update {
            LoginUpdate::Started => {
                self.phase = LoginPhase::Starting;
                self.prompt = DevicePrompt::default();
                self.message.clear();
            }
            LoginUpdate::Prompt(next) => {
                if !matches!(self.phase, LoginPhase::Starting | LoginPhase::Waiting) {
                    return;
                }
                self.prompt.merge(&next);
                if self.prompt.is_complete() {
                    self.phase = LoginPhase::Waiting;
                }
            }
            LoginUpdate::SessionReady => {
                if self.phase == LoginPhase::SigningOut {
                    return;
                }
                self.phase = LoginPhase::Succeeded;
                self.prompt = DevicePrompt::default();
                self.message = "Signed in. Loading usage…".into();
            }
            LoginUpdate::Exited { success, message } => {
                if self.phase == LoginPhase::Succeeded || self.phase == LoginPhase::SigningOut {
                    return;
                }
                self.phase = LoginPhase::Failed;
                self.prompt = DevicePrompt::default();
                self.message = if success {
                    "Sign-in ended without a session for this account.".into()
                } else {
                    failure_message(&message)
                };
            }
            LoginUpdate::Failed(message) => {
                self.phase = LoginPhase::Failed;
                self.prompt = DevicePrompt::default();
                self.message = failure_message(&message);
            }
            LoginUpdate::Canceled => {
                self.phase = LoginPhase::Idle;
                self.prompt = DevicePrompt::default();
                self.message = "Sign-in canceled.".into();
            }
            LoginUpdate::SigningOut => {
                self.phase = LoginPhase::SigningOut;
                self.prompt = DevicePrompt::default();
                self.message = "Signing out…".into();
            }
            LoginUpdate::SignedOut => {
                self.phase = LoginPhase::Idle;
                self.prompt = DevicePrompt::default();
                self.message = "Signed out.".into();
            }
        }
    }
}

pub fn parse_device_prompt(text: &str) -> DevicePrompt {
    let text = strip_ansi(text);
    let mut prompt = DevicePrompt::default();
    if let Some(json) = extract_json_object(&text)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(json)
    {
        prompt.verification_url = json_url(&value);
        prompt.user_code = value
            .get("user_code")
            .and_then(|item| item.as_str())
            .map(str::trim)
            .filter(|code| is_user_code(code))
            .map(str::to_string);
    }

    let urls = https_urls(&text);
    if prompt.verification_url.is_none() {
        prompt.verification_url = choose_url(&urls);
    }
    if prompt.user_code.is_none() {
        prompt.user_code = urls.iter().find_map(|url| user_code_from_url(url));
    }
    if prompt.user_code.is_none() {
        prompt.user_code = text.lines().find_map(code_from_line);
    }
    prompt
}

pub fn sanitize_cli_message(text: &str) -> String {
    let redacted = redact_secrets(&strip_ansi(text));
    let mut lines: Vec<&str> = redacted
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "[redacted]")
        .collect();
    if lines.len() > 4 {
        lines = lines.split_off(lines.len() - 4);
    }
    let joined = lines.join(" ");
    let mut out: String = joined.chars().take(240).collect();
    if joined.chars().count() > 240 {
        out.push('…');
    }
    out
}

pub fn failure_message(raw: &str) -> String {
    let clean = sanitize_cli_message(raw);
    if clean.is_empty() {
        "Sign-in failed.".into()
    } else {
        clean
    }
}

pub struct LoginJob {
    child: Child,
    rx: Receiver<LoginJobEvent>,
    stream_message: Option<String>,
    exited: Option<ExitStatus>,
}

enum LoginJobEvent {
    Prompt(DevicePrompt),
    StreamsClosed { message: String },
}

pub struct LoginPoll {
    pub prompt: Option<DevicePrompt>,
    pub finished: Option<LoginFinish>,
}

pub struct LoginFinish {
    pub success: bool,
    pub message: String,
}

impl LoginJob {
    pub fn poll(&mut self) -> LoginPoll {
        let mut prompt = None;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                LoginJobEvent::Prompt(next) => prompt = Some(next),
                LoginJobEvent::StreamsClosed { message } => {
                    self.stream_message = Some(message);
                }
            }
        }
        if self.exited.is_none()
            && let Ok(Some(status)) = self.child.try_wait()
        {
            self.exited = Some(status);
        }
        let finished = match (self.exited, self.stream_message.clone()) {
            (Some(status), Some(message)) => Some(LoginFinish {
                success: status.success(),
                message,
            }),
            _ => None,
        };
        LoginPoll { prompt, finished }
    }
}

impl Drop for LoginJob {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn spawn_device_login(home: &Path) -> Result<LoginJob, String> {
    let invocation = device_login_invocation(home);
    let mut cmd = Command::new(&invocation.program);
    invocation.apply(&mut cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|err| format!("failed to launch grok: {err}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "grok sign-in did not provide output".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "grok sign-in did not provide output".to_string())?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || read_login_output(stdout, stderr, tx));
    Ok(LoginJob {
        child,
        rx,
        stream_message: None,
        exited: None,
    })
}

pub fn run_logout(home: &Path) -> Result<(), String> {
    let invocation = logout_invocation(home);
    let mut cmd = Command::new(&invocation.program);
    invocation.apply(&mut cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd
        .output()
        .map_err(|err| format!("failed to launch grok: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    let message = sanitize_cli_message(&text);
    Err(if message.is_empty() {
        "Sign-out failed.".into()
    } else {
        message
    })
}

pub async fn logout_async(home: PathBuf) -> Result<(), String> {
    tokio::task::spawn_blocking(move || run_logout(&home))
        .await
        .unwrap_or_else(|_| Err("Sign-out failed.".into()))
}

fn read_login_output<R, S>(stdout: R, stderr: S, tx: Sender<LoginJobEvent>)
where
    R: Read + Send + 'static,
    S: Read + Send + 'static,
{
    let (chunk_tx, chunk_rx) = mpsc::channel();
    let stdout_tx = chunk_tx.clone();
    let stdout_thread = thread::spawn(move || read_chunks(stdout, stdout_tx));
    let stderr_thread = thread::spawn(move || read_chunks(stderr, chunk_tx));
    let mut buffer = String::new();
    let mut prompt = DevicePrompt::default();
    while let Ok(chunk) = chunk_rx.recv() {
        buffer.push_str(&chunk);
        trim_buffer(&mut buffer);
        let parsed = parse_device_prompt(&buffer);
        let mut merged = prompt.clone();
        merged.merge(&parsed);
        if merged != prompt {
            prompt = merged.clone();
            if tx.send(LoginJobEvent::Prompt(merged)).is_err() {
                break;
            }
        }
    }
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    let message = sanitize_cli_message(&buffer);
    let _ = tx.send(LoginJobEvent::StreamsClosed { message });
}

fn read_chunks<R: Read>(mut reader: R, tx: Sender<String>) {
    let mut buf = [0u8; 512];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                if tx.send(chunk).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn trim_buffer(buffer: &mut String) {
    const MAX_BUFFER: usize = 12 * 1024;
    const KEEP: usize = 4 * 1024;
    if buffer.len() <= MAX_BUFFER {
        return;
    }
    let mut cut = buffer.len() - KEEP;
    while cut < buffer.len() && !buffer.is_char_boundary(cut) {
        cut += 1;
    }
    buffer.drain(..cut);
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(&text[start..=end])
}

fn json_url(value: &serde_json::Value) -> Option<String> {
    for key in [
        "verification_uri",
        "verification_url",
        "verification_uri_complete",
    ] {
        if let Some(url) = value.get(key).and_then(|item| item.as_str()) {
            let url = url.trim();
            if url.starts_with("https://") {
                return Some(url.to_string());
            }
        }
    }
    None
}

fn https_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find("https://") {
        let tail = &rest[index..];
        let end = tail
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>'))
            .unwrap_or(tail.len());
        let url = tail[..end]
            .trim_end_matches(['.', ',', ')', ']'])
            .to_string();
        if url.starts_with("https://") {
            urls.push(url);
        }
        rest = if end < tail.len() { &tail[end..] } else { "" };
        if rest.is_empty() {
            break;
        }
    }
    urls
}

fn choose_url(urls: &[String]) -> Option<String> {
    urls.iter()
        .find(|url| url.contains("user_code="))
        .cloned()
        .or_else(|| {
            urls.iter()
                .find(|url| {
                    let lower = url.to_ascii_lowercase();
                    lower.contains("device")
                        || lower.contains("oauth")
                        || lower.contains("auth.x.ai")
                        || lower.contains("accounts.x.ai")
                })
                .cloned()
        })
        .or_else(|| urls.first().cloned())
}

fn user_code_from_url(url: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        if key == "user_code" || key == "code" {
            let decoded = value.replace("%2D", "-").replace("%2d", "-");
            if is_user_code(&decoded) {
                return Some(decoded);
            }
        }
    }
    None
}

fn code_from_line(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut search = 0;
    while let Some(relative) = lower[search..].find("code") {
        let index = search + relative;
        let end = index + 4;
        let before_ok = index == 0 || !bytes[index - 1].is_ascii_alphanumeric();
        let after_ok = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        let preceded_by_device = index >= 7 && &lower[index - 7..index] == "device_";
        if before_ok
            && after_ok
            && !preceded_by_device
            && let Some(code) = next_user_code(&line[end..])
        {
            return Some(code);
        }
        search = end;
    }
    None
}

fn next_user_code(text: &str) -> Option<String> {
    let trimmed = text.trim_start_matches(|ch: char| !ch.is_ascii_alphanumeric());
    if trimmed.is_empty() {
        return None;
    }
    let token: String = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .collect();
    let token = token.trim_matches('-');
    if is_user_code(token) {
        Some(token.to_string())
    } else {
        None
    }
}

fn is_user_code(raw: &str) -> bool {
    let code = raw.trim();
    if code.len() < 4 || code.len() > 20 || code.contains('.') {
        return false;
    }
    let mut parts = 0;
    for part in code.split('-') {
        parts += 1;
        if part.is_empty() || part.len() > 10 || !part.chars().all(|ch| ch.is_ascii_alphanumeric())
        {
            return false;
        }
    }
    (1..=2).contains(&parts)
}

fn redact_secrets(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("access_token")
            || lower.contains("refresh_token")
            || lower.contains("device_code")
            || lower.contains("id_token")
            || lower.contains("\"key\"")
            || lower.contains("auth.json")
        {
            out.push_str("[redacted]\n");
            continue;
        }
        let mut redacted = String::new();
        for token in line.split_whitespace() {
            if !redacted.is_empty() {
                redacted.push(' ');
            }
            if looks_like_secret(token) {
                redacted.push_str("[redacted]");
            } else {
                redacted.push_str(token);
            }
        }
        out.push_str(&redacted);
        out.push('\n');
    }
    out
}

fn looks_like_secret(token: &str) -> bool {
    if token.starts_with("https://") || token.starts_with("http://") {
        return false;
    }
    token
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .count()
        >= 24
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_login_is_scoped_and_is_not_browser_oauth() {
        let home = Path::new("/icons/pers/.grok-personal");
        let invocation = device_login_invocation(home);
        assert_eq!(invocation.program, "grok");
        assert_eq!(invocation.args, ["login", "--device-code"]);
        assert_eq!(invocation.grok_home, home);
        assert!(invocation.args.iter().all(|arg| arg != "--oauth"));
        assert!(invocation.args.iter().all(|arg| arg != "logout"));
        assert_ne!(invocation.args, ["login"]);
    }

    #[test]
    fn logout_is_scoped_to_the_given_home_only() {
        let sim = logout_invocation(Path::new("/icons/sim/.grok"));
        let pers = logout_invocation(Path::new("/icons/pers/.grok-personal"));
        assert_eq!(sim.args, ["logout"]);
        assert_eq!(pers.args, ["logout"]);
        assert_ne!(sim.grok_home, pers.grok_home);
        assert!(sim.args.iter().all(|arg| arg != "login"));
        assert!(sim.args.iter().all(|arg| arg != "--device-code"));
        assert!(pers.args.iter().all(|arg| arg != "--oauth"));
    }

    #[test]
    fn child_env_is_the_icon_home_and_the_parent_is_unchanged() {
        let _guard = PROCESS_ENV_LOCK.lock().unwrap();
        let previous_home = std::env::var_os("GROK_HOME");
        let previous_file = std::env::var_os("GROK_CREDENTIALS_FILE");
        // SAFETY: tests serialize environment changes with PROCESS_ENV_LOCK.
        unsafe { std::env::set_var("GROK_HOME", "/parent/grok-home") };
        unsafe { std::env::set_var("GROK_CREDENTIALS_FILE", "/parent/credentials.json") };

        let home = "/icons/pers/.grok-personal";
        let invocation = device_login_invocation(Path::new(home));
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("printf '%s\\n' \"$0\" \"$1\" \"$#\" \"$GROK_HOME\"");
        invocation.apply(&mut cmd);
        let output = cmd.output().unwrap();
        assert!(output.status.success(), "print of child env failed");
        let text = String::from_utf8(output.stdout).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines, ["login", "--device-code", "1", home]);

        let logout = logout_invocation(Path::new("/icons/sim/.grok"));
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("printf '%s\\n' \"$0\" \"$#\" \"$GROK_HOME\"; printenv GROK_CREDENTIALS_FILE");
        logout.apply(&mut cmd);
        let output = cmd.output().unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(
            text.lines().next(),
            Some("logout"),
            "sign-out must not be a login"
        );
        assert!(text.contains("/icons/sim/.grok"));
        assert!(!text.contains("GROK_CREDENTIALS_FILE="));
        assert!(!text.contains("/parent/credentials.json"));

        assert_eq!(std::env::var("GROK_HOME").unwrap(), "/parent/grok-home");
        assert_eq!(
            std::env::var("GROK_CREDENTIALS_FILE").unwrap(),
            "/parent/credentials.json"
        );
        restore("GROK_HOME", previous_home);
        restore("GROK_CREDENTIALS_FILE", previous_file);
    }

    #[test]
    fn parses_url_and_user_code_from_cli_text() {
        let prompt =
            parse_device_prompt("Open https://auth.x.ai/oauth2/device and enter code: ABCD-EFGH\n");
        assert_eq!(
            prompt.verification_url.as_deref(),
            Some("https://auth.x.ai/oauth2/device")
        );
        assert_eq!(prompt.user_code.as_deref(), Some("ABCD-EFGH"));
        assert!(prompt.is_complete());
    }

    #[test]
    fn parses_split_lines_and_query_codes() {
        let prompt =
            parse_device_prompt("Go to: https://auth.x.ai/oauth2/device\nEnter code: WXYZ-1234\n");
        assert_eq!(prompt.user_code.as_deref(), Some("WXYZ-1234"));
        assert!(prompt.verification_url.unwrap().contains("auth.x.ai"));

        let from_query =
            parse_device_prompt("https://auth.x.ai/oauth2/device?user_code=QRST-9876\n");
        assert_eq!(from_query.user_code.as_deref(), Some("QRST-9876"));
        assert!(
            from_query
                .verification_url
                .unwrap()
                .starts_with("https://auth.x.ai/")
        );
    }

    #[test]
    fn json_prompt_uses_the_user_code_not_the_device_code() {
        let raw = r#"{"user_code":"ABCD-EFGH","device_code":"longsecretdevicecodethatmustnotbeshown","verification_uri":"https://auth.x.ai/oauth2/device"}"#;
        let prompt = parse_device_prompt(raw);
        assert_eq!(prompt.user_code.as_deref(), Some("ABCD-EFGH"));
        assert_eq!(
            prompt.verification_url.as_deref(),
            Some("https://auth.x.ai/oauth2/device")
        );
        assert!(!format!("{prompt:?}").contains("longsecretdevicecodethatmustnotbeshown"));
    }

    #[test]
    fn ansi_and_long_tokens_do_not_become_the_user_code() {
        let raw = "\u{1b}[32mEnter code: ABCD-EFGH\u{1b}[0m\nhttps://example.test/callback?code=abcdefghijklmnopqrstuvwxyz0123456789\n";
        let prompt = parse_device_prompt(raw);
        assert_eq!(prompt.user_code.as_deref(), Some("ABCD-EFGH"));
    }

    #[test]
    fn sanitize_drops_tokens_and_credential_lines() {
        let raw = "Sign-in failed\n{\"access_token\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"device_code\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"}\n";
        let clean = sanitize_cli_message(raw);
        assert!(clean.contains("Sign-in failed"));
        assert!(!clean.contains("aaaa"));
        assert!(!clean.contains("bbbb"));
        assert!(!clean.to_ascii_lowercase().contains("access_token"));
        assert!(!clean.to_ascii_lowercase().contains("device_code"));
    }

    #[test]
    fn login_status_reaches_waiting_then_success_without_keeping_the_code() {
        let mut state = LoginUiState::default();
        state.apply(LoginUpdate::Started);
        assert_eq!(state.phase, LoginPhase::Starting);
        state.apply(LoginUpdate::Prompt(parse_device_prompt(
            "Open https://auth.x.ai/oauth2/device and enter code: ABCD-EFGH",
        )));
        assert_eq!(state.phase, LoginPhase::Waiting);
        assert_eq!(state.prompt.user_code.as_deref(), Some("ABCD-EFGH"));
        state.apply(LoginUpdate::SessionReady);
        assert_eq!(state.phase, LoginPhase::Succeeded);
        assert!(state.prompt.user_code.is_none());
        assert!(state.prompt.verification_url.is_none());
        state.apply(LoginUpdate::Exited {
            success: false,
            message: "boom".into(),
        });
        assert_eq!(state.phase, LoginPhase::Succeeded);
    }

    #[test]
    fn failed_exit_is_a_failure_and_cancel_clears_the_code() {
        let mut state = LoginUiState::default();
        state.apply(LoginUpdate::Started);
        state.apply(LoginUpdate::Prompt(DevicePrompt {
            verification_url: Some("https://auth.x.ai/oauth2/device".into()),
            user_code: Some("ABCD-EFGH".into()),
        }));
        state.apply(LoginUpdate::Exited {
            success: false,
            message: "the CLI rejected the flag".into(),
        });
        assert_eq!(state.phase, LoginPhase::Failed);
        assert!(state.message.contains("rejected"));
        assert!(state.prompt.user_code.is_none());

        state.apply(LoginUpdate::Started);
        state.apply(LoginUpdate::Canceled);
        assert_eq!(state.phase, LoginPhase::Idle);
        assert_eq!(state.message, "Sign-in canceled.");
        assert!(state.prompt.user_code.is_none());
    }

    #[test]
    fn success_without_a_session_is_not_a_silent_sign_in() {
        let mut state = LoginUiState::default();
        state.apply(LoginUpdate::Started);
        state.apply(LoginUpdate::Exited {
            success: true,
            message: String::new(),
        });
        assert_eq!(state.phase, LoginPhase::Failed);
        assert!(state.message.contains("without a session"));
    }

    fn restore(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            // SAFETY: tests serialize environment changes with PROCESS_ENV_LOCK.
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
    }
}
