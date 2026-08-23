use super::secrets::grok_bot_config_dir;
use data_encoding::BASE32;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct BotRoster {
    pub count: usize,
    pub unread: u32,
    pub names: Vec<String>,
    pub needs_you: bool,
    pub running: bool,
}

#[derive(Debug, Clone, Default)]
pub struct BotRow {
    pub name: String,
    pub unread: u32,
    pub last_activity_at: i64,
    pub hidden: bool,
    pub awaiting: bool,
}

#[derive(Deserialize)]
struct RosterFile {
    value: RosterValue,
}

#[derive(Deserialize)]
struct RosterValue {
    rows: Vec<RosterRow>,
}

#[derive(Deserialize)]
struct RosterRow {
    name: Option<String>,
    #[serde(rename = "unreadCount")]
    unread_count: Option<u32>,
    #[serde(rename = "lastActivityAt")]
    last_activity_at: Option<i64>,
    #[serde(rename = "isHiddenFromSidebar")]
    is_hidden_from_sidebar: Option<bool>,
    #[serde(rename = "awaitingUserResponse")]
    awaiting_user_response: Option<bool>,
}

#[derive(Deserialize)]
struct SessionMarker {
    pid: Option<u32>,
}

pub fn live_roster() -> BotRoster {
    live_roster_from(&grok_bot_config_dir())
}

pub fn live_roster_from(config_dir: &Path) -> BotRoster {
    let running = session_running(&config_dir.join("sand-session-marker.json"));
    let Some(rows) = load_rows(&config_dir.join("sand-client-persistence")) else {
        return BotRoster {
            running,
            ..BotRoster::default()
        };
    };
    summarize(rows, running)
}

pub fn summarize(mut rows: Vec<BotRow>, running: bool) -> BotRoster {
    rows.retain(|row| !row.hidden && !row.name.is_empty());
    rows.sort_by_key(|a| std::cmp::Reverse(a.last_activity_at));
    let unread = rows.iter().map(|r| r.unread).sum();
    let needs_you = rows.iter().any(|r| r.awaiting);
    let count = rows.len();
    let names = rows.into_iter().take(3).map(|r| r.name).collect();
    BotRoster {
        count,
        unread,
        names,
        needs_you,
        running,
    }
}

pub fn parse_roster_json(bytes: &[u8]) -> Vec<BotRow> {
    let Ok(file) = serde_json::from_slice::<RosterFile>(bytes) else {
        return Vec::new();
    };
    file.value
        .rows
        .into_iter()
        .filter_map(|row| {
            let name = row.name.filter(|s| !s.is_empty())?;
            Some(BotRow {
                name,
                unread: row.unread_count.unwrap_or(0),
                last_activity_at: row.last_activity_at.unwrap_or(0),
                hidden: row.is_hidden_from_sidebar.unwrap_or(false),
                awaiting: row.awaiting_user_response.unwrap_or(false),
            })
        })
        .collect()
}

pub fn session_running(path: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(marker) = serde_json::from_str::<SessionMarker>(&raw) else {
        return false;
    };
    let Some(pid) = marker.pid else {
        return false;
    };
    Path::new(&format!("/proc/{pid}")).exists()
}

fn load_rows(persist_dir: &Path) -> Option<Vec<BotRow>> {
    let mut best: Option<(std::time::SystemTime, Vec<u8>)> = None;
    let entries = std::fs::read_dir(persist_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("blob") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(key) = decode_base32_key(stem) else {
            continue;
        };
        if !key.ends_with(".roster.last-roster") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let mtime = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        match &best {
            Some((best_mtime, _)) if *best_mtime >= mtime => {}
            _ => best = Some((mtime, bytes)),
        }
    }
    best.map(|(_, bytes)| parse_roster_json(&bytes))
}

pub fn decode_base32_key(stem: &str) -> Option<String> {
    let upper = stem.to_ascii_uppercase();
    let pad = (8 - upper.len() % 8) % 8;
    let padded = format!("{upper}{}", "=".repeat(pad));
    let bytes = BASE32.decode(padded.as_bytes()).ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
pub fn encode_base32_key(key: &str) -> String {
    BASE32
        .encode(key.as_bytes())
        .trim_end_matches('=')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process;

    #[test]
    fn decode_roundtrip() {
        let key = "sand.client.slice.account.grok%7Cuser.roster.last-roster";
        let stem = encode_base32_key(key);
        assert_eq!(decode_base32_key(&stem).as_deref(), Some(key));
    }

    #[test]
    fn roster_skips_hidden_and_sorts() {
        let json = include_bytes!("../../tests/fixtures/bot_roster.json");
        let rows = parse_roster_json(json);
        let live = summarize(rows, true);
        assert_eq!(live.count, 2);
        assert_eq!(live.unread, 3);
        assert_eq!(live.names, vec!["Chief", "Research"]);
        assert!(live.needs_you);
        assert!(live.running);
    }

    #[test]
    fn session_marker_live_pid() {
        let dir = std::env::temp_dir().join(format!("grok-mon-bot-roster-{}", process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("marker.json");
        fs::write(
            &path,
            format!(
                r#"{{"version":1,"pid":{},"appVersion":"0.16.0"}}"#,
                process::id()
            ),
        )
        .unwrap();
        assert!(session_running(&path));
        fs::write(&path, r#"{"version":1,"pid":1}"#).unwrap();
        // pid 1 exists on linux
        assert!(session_running(&path));
        fs::write(&path, r#"{"version":1,"pid":2147483646}"#).unwrap();
        assert!(!session_running(&path));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_roster_prefers_newer_mtime() {
        let dir = std::env::temp_dir().join(format!("grok-mon-bot-mtime-{}", process::id()));
        let persist = dir.join("sand-client-persistence");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&persist).unwrap();

        let older_key = "sand.client.slice.account.old.roster.last-roster";
        let newer_key = "sand.client.slice.account.new.roster.last-roster";
        let older_path = persist.join(format!("{}.blob", encode_base32_key(older_key)));
        let newer_path = persist.join(format!("{}.blob", encode_base32_key(newer_key)));
        fs::write(
            &older_path,
            br#"{"schemaVersion":2,"value":{"rows":[{"name":"Old","unreadCount":1,"lastActivityAt":1}]}}"#,
        )
        .unwrap();
        fs::write(
            &newer_path,
            br#"{"schemaVersion":2,"value":{"rows":[{"name":"New","unreadCount":1,"lastActivityAt":2}]}}"#,
        )
        .unwrap();

        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(100);
        let new = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(200);
        fs::File::open(&older_path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        fs::File::open(&newer_path)
            .unwrap()
            .set_modified(new)
            .unwrap();

        let live = live_roster_from(&dir);
        assert_eq!(live.names, vec!["New"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_roster_from_persistence_dir() {
        let dir = std::env::temp_dir().join(format!("grok-mon-bot-persist-{}", process::id()));
        let persist = dir.join("sand-client-persistence");
        fs::create_dir_all(&persist).unwrap();
        let key = "sand.client.slice.account.demo.roster.last-roster";
        let path = persist.join(format!("{}.blob", encode_base32_key(key)));
        fs::write(
            &path,
            include_bytes!("../../tests/fixtures/bot_roster.json"),
        )
        .unwrap();
        fs::write(
            dir.join("sand-session-marker.json"),
            format!(r#"{{"pid":{}}}"#, process::id()),
        )
        .unwrap();
        let live = live_roster_from(&dir);
        assert_eq!(live.count, 2);
        assert!(live.running);
        let _ = fs::remove_dir_all(&dir);
    }
}
