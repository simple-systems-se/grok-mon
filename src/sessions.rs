use serde::Deserialize;
use std::fs;
use std::path::Path;

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

/// Live CLI sessions for one icon's home. The process `GROK_HOME` is ignored.
pub fn live_sessions_at(home: &Path) -> LiveSessions {
    let path = home.join("active_sessions.json");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_follow_the_icon_home() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("grok-mon-sessions-{nanos}"));
        let sim = root.join("sim");
        let pers = root.join("pers");
        fs::create_dir_all(&sim).unwrap();
        fs::create_dir_all(&pers).unwrap();
        let pid = std::process::id();
        fs::write(
            sim.join("active_sessions.json"),
            format!(r#"[{{"pid":{pid},"cwd":"/tmp/alpha"}}]"#),
        )
        .unwrap();
        fs::write(
            pers.join("active_sessions.json"),
            format!(r#"[{{"pid":{pid},"cwd":"/tmp/beta"}}]"#),
        )
        .unwrap();
        assert_eq!(live_sessions_at(&sim).names, ["alpha".to_string()]);
        assert_eq!(live_sessions_at(&pers).names, ["beta".to_string()]);
        assert!(live_sessions_at(&root.join("empty")).names.is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
