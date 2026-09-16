use crate::spawn::spawn_detached;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const DESKTOP_NAMES: &[&str] = &["grok-bot.desktop", "sand.desktop"];
const BIN_CANDIDATES: &[&str] = &[
    "/opt/Grok Bot/grok-bot",
    "/opt/Grok Bot/sand",
    "/usr/bin/grok-bot",
    "/usr/local/bin/grok-bot",
    "/usr/bin/sand",
];

pub fn open_grok_bot_for(config_dir: Option<&Path>) -> Result<(), String> {
    if let Some(desktop) = find_desktop_for(config_dir) {
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
        if let Some(dir) = config_dir {
            cmd.arg(format!("--user-data-dir={}", dir.display()));
        }
        return crate::spawn::spawn_detached_cmd(&mut cmd, "Grok Bot");
    }
    Err("Grok Bot is not installed".into())
}

fn find_desktop_for(config_dir: Option<&Path>) -> Option<PathBuf> {
    let dirs = desktop_search_dirs();
    let candidates = list_grok_bot_desktops(&dirs);
    pick_desktop(&candidates, config_dir)
}

fn desktop_search_dirs() -> Vec<PathBuf> {
    let mut dirs = application_dirs(
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("XDG_DATA_DIRS"),
    );
    dirs.extend(autostart_dirs(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    ));
    dirs
}

fn autostart_dirs(xdg_config_home: Option<PathBuf>, home: Option<PathBuf>) -> Vec<PathBuf> {
    if let Some(xdg) = xdg_config_home.filter(|p| !p.as_os_str().is_empty()) {
        vec![xdg.join("autostart")]
    } else if let Some(home) = home.filter(|p| !p.as_os_str().is_empty()) {
        vec![home.join(".config/autostart")]
    } else {
        Vec::new()
    }
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

#[cfg(test)]
fn find_desktop_in(dirs: &[PathBuf]) -> Option<PathBuf> {
    pick_desktop(&list_grok_bot_desktops(dirs), None)
}

fn list_grok_bot_desktops(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in dirs {
        for name in DESKTOP_NAMES {
            push_desktop(&mut out, dir.join(name));
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with("grok-bot") && name.ends_with(".desktop") {
                push_desktop(&mut out, path);
            }
        }
    }
    out
}

fn push_desktop(out: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_file() && !out.contains(&path) {
        out.push(path);
    }
}

fn pick_desktop(candidates: &[PathBuf], config_dir: Option<&Path>) -> Option<PathBuf> {
    if candidates.is_empty() {
        return None;
    }
    let Some(dir) = config_dir else {
        return candidates.first().cloned();
    };
    let mut best: Option<(&PathBuf, i32)> = None;
    for desktop in candidates {
        let score = desktop_match_score(desktop, dir);
        match best {
            Some((_, best_score)) if best_score >= score => {}
            _ if score > 0 => best = Some((desktop, score)),
            _ => {}
        }
    }
    best.and_then(|(path, score)| (score >= 40).then(|| path.clone()))
}

fn desktop_slug(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn desktop_match_score(desktop: &Path, config_dir: &Path) -> i32 {
    let dir_name = config_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let dir_slug = desktop_slug(dir_name);
    let stem = desktop.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let stem_slug = desktop_slug(stem);
    let mut score = 0;
    if let Ok(text) = std::fs::read_to_string(desktop) {
        let dir_str = config_dir.to_string_lossy();
        if !dir_str.is_empty() && text.contains(dir_str.as_ref()) {
            score += 100;
        }
        if !dir_name.is_empty() && text.contains(dir_name) {
            score += 80;
        }
    }
    if !dir_slug.is_empty() && stem_slug == dir_slug {
        score += 60;
    }
    let suffix = dir_slug.strip_prefix("grokbot").unwrap_or("");
    if !suffix.is_empty() && stem_slug.contains(suffix) {
        score += 50;
    }
    if matches!(stem, "grok-bot" | "sand") && dir_name == "Grok Bot" {
        score += 30;
    }
    score
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

    #[test]
    fn lists_profile_desktops_and_matches_simple_systems() {
        let dir = temp_dir("profiles");
        fs::write(
            dir.join("grok-bot.desktop"),
            "[Desktop Entry]\nName=Grok Bot\n",
        )
        .unwrap();
        fs::write(
            dir.join("grok-bot-personal.desktop"),
            "[Desktop Entry]\nName=Grok Bot Personal\n",
        )
        .unwrap();
        fs::write(
            dir.join("grok-bot-simple-systems.desktop"),
            "[Desktop Entry]\nName=Grok Bot Simple Systems\n",
        )
        .unwrap();
        let listed = list_grok_bot_desktops(std::slice::from_ref(&dir));
        assert!(listed.iter().any(|p| p.ends_with("grok-bot.desktop")));
        assert!(
            listed
                .iter()
                .any(|p| p.ends_with("grok-bot-simple-systems.desktop"))
        );

        let work = PathBuf::from("/home/jeff/.config/Grok Bot Simple Systems");
        let picked = pick_desktop(&listed, Some(&work)).unwrap();
        assert!(picked.ends_with("grok-bot-simple-systems.desktop"));

        let personal = PathBuf::from("/home/jeff/.config/Grok Bot");
        let picked = pick_desktop(&listed, Some(&personal)).unwrap();
        assert!(picked.ends_with("grok-bot.desktop"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn autostart_dirs_use_xdg_config() {
        assert_eq!(
            autostart_dirs(Some(PathBuf::from("/tmp/cfg")), None)[0],
            PathBuf::from("/tmp/cfg/autostart")
        );
        assert_eq!(
            autostart_dirs(Some(PathBuf::from("")), Some(PathBuf::from("/tmp/home")))[0],
            PathBuf::from("/tmp/home/.config/autostart")
        );
    }

    #[test]
    fn slug_match_ignores_spaces_and_hyphens() {
        assert_eq!(
            desktop_slug("Grok Bot Simple Systems"),
            "grokbotsimplesystems"
        );
        assert_eq!(
            desktop_slug("grok-bot-simple-systems"),
            "grokbotsimplesystems"
        );
    }
}
