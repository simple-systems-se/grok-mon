use crate::spawn::{detach, spawn_detached};
use std::path::PathBuf;
use std::process::Command;

const DESKTOP_NAMES: &[&str] = &["sand.desktop", "grok-bot.desktop"];
const BIN_CANDIDATES: &[&str] = &[
    "/opt/Grok Bot/sand",
    "/usr/bin/grok-bot",
    "/usr/local/bin/grok-bot",
    "/usr/bin/sand",
    "/opt/grokbot-linux-port/grok-bot",
];

pub fn open_grok_bot() -> Result<(), String> {
    if let Some(desktop) = find_desktop() {
        if let Some(path) = desktop.to_str()
            && spawn_detached("gio", &["launch", path]).is_ok()
        {
            return Ok(());
        }
        if let Some(stem) = desktop.file_stem().and_then(|s| s.to_str())
            && spawn_detached("gtk-launch", &[stem]).is_ok()
        {
            return Ok(());
        }
    }
    if let Some(bin) = find_binary() {
        let mut cmd = Command::new(bin);
        return detach(&mut cmd)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to launch Grok Bot: {e}"));
    }
    Err("Grok Bot is not installed".into())
}

fn find_desktop() -> Option<PathBuf> {
    find_desktop_in(&application_dirs(
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    ))
}

fn find_binary() -> Option<PathBuf> {
    find_binary_in(BIN_CANDIDATES.iter().map(PathBuf::from).chain(path_bins()))
}

fn application_dirs(xdg_data_home: Option<PathBuf>, home: Option<PathBuf>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(xdg) = xdg_data_home {
        dirs.push(xdg.join("applications"));
    } else if let Some(home) = home {
        dirs.push(home.join(".local/share/applications"));
    }
    dirs.push(PathBuf::from("/usr/local/share/applications"));
    dirs.push(PathBuf::from("/usr/share/applications"));
    dirs
}

fn find_desktop_in(dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in dirs {
        for name in DESKTOP_NAMES {
            let path = dir.join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn find_binary_in(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|path| path.is_file())
}

fn path_bins() -> Vec<PathBuf> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(&path)
        .flat_map(|dir| [dir.join("grok-bot"), dir.join("sand")])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grok-mon-bot-launch-{name}-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn prefers_sand_desktop_over_grok_bot() {
        let dir = temp_dir("desktop");
        fs::write(dir.join("grok-bot.desktop"), "[Desktop Entry]\n").unwrap();
        fs::write(dir.join("sand.desktop"), "[Desktop Entry]\n").unwrap();
        assert_eq!(
            find_desktop_in(std::slice::from_ref(&dir)).as_deref(),
            Some(dir.join("sand.desktop").as_path())
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_first_existing_binary() {
        let dir = temp_dir("bin");
        let missing = dir.join("missing");
        let present = dir.join("grok-bot");
        fs::write(&present, b"").unwrap();
        assert_eq!(
            find_binary_in([missing, present.clone()]).as_deref(),
            Some(present.as_path())
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn application_dirs_prefer_xdg() {
        let dirs = application_dirs(
            Some(PathBuf::from("/tmp/xdg-data")),
            Some(PathBuf::from("/tmp/home")),
        );
        assert_eq!(dirs[0], PathBuf::from("/tmp/xdg-data/applications"));
        assert!(
            dirs.iter()
                .any(|d| d.as_path() == std::path::Path::new("/usr/share/applications"))
        );
    }
}
