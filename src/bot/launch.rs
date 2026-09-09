use crate::spawn::spawn_detached;
use std::ffi::OsString;
use std::path::PathBuf;

const DESKTOP_NAMES: &[&str] = &["grok-bot.desktop", "sand.desktop"];
const BIN_CANDIDATES: &[&str] = &[
    "/opt/Grok Bot/grok-bot",
    "/opt/Grok Bot/sand",
    "/usr/bin/grok-bot",
    "/usr/local/bin/grok-bot",
    "/usr/bin/sand",
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
        let mut cmd = std::process::Command::new(bin);
        return crate::spawn::spawn_detached_cmd(&mut cmd, "Grok Bot");
    }
    Err("Grok Bot is not installed".into())
}

fn find_desktop() -> Option<PathBuf> {
    find_desktop_in(&application_dirs(
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("XDG_DATA_DIRS"),
    ))
}

fn find_binary() -> Option<PathBuf> {
    find_binary_in(BIN_CANDIDATES.iter().map(PathBuf::from).chain(path_bins()))
}

fn application_dirs(
    xdg_data_home: Option<PathBuf>,
    home: Option<PathBuf>,
    xdg_data_dirs: Option<OsString>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(xdg) = xdg_data_home.filter(|p| !p.as_os_str().is_empty()) {
        dirs.push(xdg.join("applications"));
    } else if let Some(home) = home.filter(|p| !p.as_os_str().is_empty()) {
        dirs.push(home.join(".local/share/applications"));
    }
    let data_dirs = xdg_data_dirs
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| OsString::from("/usr/local/share:/usr/share"));
    for dir in std::env::split_paths(&data_dirs) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let apps = dir.join("applications");
        if !dirs.contains(&apps) {
            dirs.push(apps);
        }
    }
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
    fn prefers_grok_bot_desktop_over_sand() {
        let dir = temp_dir("desktop");
        fs::write(dir.join("grok-bot.desktop"), "[Desktop Entry]\n").unwrap();
        fs::write(dir.join("sand.desktop"), "[Desktop Entry]\n").unwrap();
        assert_eq!(
            find_desktop_in(std::slice::from_ref(&dir)).as_deref(),
            Some(dir.join("grok-bot.desktop").as_path())
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
            None,
        );
        assert_eq!(dirs[0], PathBuf::from("/tmp/xdg-data/applications"));
        assert!(
            dirs.iter()
                .any(|d| d.as_path() == std::path::Path::new("/usr/share/applications"))
        );
    }

    #[test]
    fn application_dirs_empty_xdg_falls_back_and_reads_data_dirs() {
        let dirs = application_dirs(
            Some(PathBuf::from("")),
            Some(PathBuf::from("/tmp/home")),
            Some(OsString::from("/opt/share:/usr/share")),
        );
        assert_eq!(
            dirs[0],
            PathBuf::from("/tmp/home/.local/share/applications")
        );
        assert_eq!(dirs[1], PathBuf::from("/opt/share/applications"));
        assert!(
            dirs.iter()
                .any(|d| d.as_path() == std::path::Path::new("/usr/share/applications"))
        );
    }
}
