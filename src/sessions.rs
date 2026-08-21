use crate::auth::grok_home;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, Default)]
pub struct LiveSessions {
    pub count: usize,
    pub names: Vec<String>,
}

#[derive(Deserialize)]
struct SessionRow {
    pid: Option<u32>,
    cwd: Option<String>,
}

pub fn live_sessions() -> LiveSessions {
    let path = grok_home().join("active_sessions.json");
    let Ok(raw) = fs::read_to_string(path) else {
        return LiveSessions::default();
    };
    let Ok(rows) = serde_json::from_str::<Vec<SessionRow>>(&raw) else {
        return LiveSessions::default();
    };

    let mut names = Vec::new();
    for row in rows {
        let Some(pid) = row.pid else { continue };
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            continue;
        }
        let label = row
            .cwd
            .as_deref()
            .and_then(|cwd| std::path::Path::new(cwd).file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("session")
            .to_string();
        names.push(label);
    }
    LiveSessions {
        count: names.len(),
        names,
    }
}
