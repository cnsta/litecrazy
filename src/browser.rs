use log::{info, warn};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::{self, WindowMode};
use crate::tray::notifications::NotificationState;

const APP_ID: &str = "litecrazy";

/// chromium based browsers required for web app
const CHROMIUM_BINARIES: &[&str] = &[
    "chromium",
    "chromium-browser",
    "google-chrome-stable",
    "google-chrome",
    "brave-browser",
    "brave",
    "vivaldi-stable",
    "vivaldi",
    "microsoft-edge-stable",
    "microsoft-edge",
    "thorium-browser",
    "ungoogled-chromium",
    "opera",
];

const FLATPAK_APPS: &[&str] = &[
    "org.chromium.Chromium",
    "com.google.Chrome",
    "com.brave.Browser",
    "com.vivaldi.Vivaldi",
    "com.microsoft.Edge",
];

/// Extra directories to search when `PATH` is unhelpful.
fn extra_search_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/run/current-system/sw/bin"),
        PathBuf::from("/nix/var/nix/profiles/default/bin"),
        PathBuf::from("/var/lib/flatpak/exports/bin"),
    ];

    // Only build the user-specific paths when we actually know the user;
    // an empty $USER would otherwise produce "/etc/profiles/per-user//bin".
    if let Ok(user) = std::env::var("USER") {
        if !user.is_empty() {
            dirs.push(PathBuf::from(format!("/etc/profiles/per-user/{user}/bin")));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            dirs.push(PathBuf::from(format!("{home}/.nix-profile/bin")));
            dirs.push(PathBuf::from(format!("{home}/.local/bin")));
            dirs.push(PathBuf::from(format!(
                "{home}/.local/share/flatpak/exports/bin"
            )));
        }
    }

    dirs.retain(|p| p.is_dir());
    dirs
}

enum Launcher {
    Chromium(PathBuf),
    Flatpak(&'static str),
    Fallback,
}

pub fn open_configurator() {
    let url = config::configurator_url();

    match resolve_launcher() {
        Launcher::Chromium(path) => {
            if spawn(&path, &configured_browser_args(&url)) {
                info!("Opened {} in {}", url, path.display());
                return;
            }
            warn!(
                "{} failed to start, falling back to xdg-open",
                path.display()
            );
            fallback(&url);
        }
        Launcher::Flatpak(app_id) => {
            let mut args = vec!["run".to_string(), app_id.to_string()];
            args.extend(configured_browser_args(&url));
            if spawn("flatpak", &args) {
                info!("Opened {} in flatpak {}", url, app_id);
                return;
            }
            fallback(&url);
        }
        Launcher::Fallback => fallback(&url),
    }
}

fn browser_args(url: &str, mode: WindowMode, extra: Vec<String>) -> Vec<String> {
    let mut args = match mode {
        WindowMode::App => vec![format!("--app={url}"), format!("--class={APP_ID}")],
        WindowMode::Tab => vec![url.to_string()],
    };
    args.extend(extra);
    args
}

fn configured_browser_args(url: &str) -> Vec<String> {
    browser_args(url, config::window_mode(), config::browser_extra_args())
}

/// Decide what to launch, honouring an explicit override first.
fn resolve_launcher() -> Launcher {
    if let Some(name) = config::browser_override() {
        match find_executable(&name) {
            Some(path) => {
                info!("Using browser from LITECRAZY_BROWSER: {}", path.display());
                return Launcher::Chromium(path);
            }
            None => warn!(
                "LITECRAZY_BROWSER is set to '{}' but it isn't executable; auto-detecting instead",
                name
            ),
        }
    }

    for &name in CHROMIUM_BINARIES {
        if let Some(path) = find_executable(name) {
            info!("Found Chromium-based browser: {}", path.display());
            return Launcher::Chromium(path);
        }
    }

    if find_executable("flatpak").is_some() {
        for &app in FLATPAK_APPS {
            if flatpak_installed(app) {
                info!("Found Flatpak browser: {}", app);
                return Launcher::Flatpak(app);
            }
        }
    }

    Launcher::Fallback
}

/// Last resort: let the desktop decide, and tell the user why that might
/// not be good enough.
fn fallback(url: &str) {
    warn!(
        "No Chromium-based browser found; handing {} to xdg-open",
        url
    );

    NotificationState::send_notification(
        "No Chromium-based browser found",
        "The Pulsar configurator needs WebHID, which Firefox and Safari don't support. \
         Install Chromium, Chrome, Brave, Vivaldi or Edge, or point $LITECRAZY_BROWSER \
         at the binary you want.",
        "dialog-warning",
    );

    if !spawn("xdg-open", &[url.to_string()]) {
        warn!("xdg-open is not available either, giving up on {}", url);
    }
}

fn find_executable(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let path = PathBuf::from(name);
        return is_executable(&path).then_some(path);
    }

    let path_dirs = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();

    path_dirs
        .into_iter()
        .chain(extra_search_dirs())
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// `flatpak info <id>` exits non-zero when the app isn't installed.
fn flatpak_installed(app_id: &str) -> bool {
    Command::new("flatpak")
        .args(["info", app_id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn spawn(program: impl AsRef<std::ffi::OsStr>, args: &[String]) -> bool {
    use std::os::unix::process::CommandExt;

    let program = program.as_ref();
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);

    match cmd.spawn() {
        Ok(child) => {
            std::thread::spawn(move || {
                let mut child = child;
                let _ = child.wait();
            });
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            warn!("Failed to spawn {}: {}", program.to_string_lossy(), e);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_paths_are_taken_at_face_value() {
        assert!(find_executable("/bin/sh").is_some());
        assert!(find_executable("/definitely/not/here").is_none());
    }

    #[test]
    fn bare_names_are_looked_up_on_path() {
        assert!(find_executable("sh").is_some());
        assert!(find_executable("nonexistent-browser-xyz").is_none());
    }

    #[test]
    fn app_mode_args_carry_url_and_class() {
        let args = browser_args("https://example.test/", WindowMode::App, vec![]);
        assert_eq!(args, ["--app=https://example.test/", "--class=litecrazy"]);
    }

    #[test]
    fn tab_mode_passes_the_url_positionally() {
        let args = browser_args("https://example.test/", WindowMode::Tab, vec![]);
        assert_eq!(args, ["https://example.test/"]);
    }

    #[test]
    fn extra_args_are_appended() {
        let args = browser_args(
            "https://example.test/",
            WindowMode::Tab,
            vec!["--ozone-platform=wayland".into()],
        );
        assert_eq!(args.last().unwrap(), "--ozone-platform=wayland");
    }

    #[test]
    fn directories_are_not_executables() {
        assert!(!is_executable(Path::new("/tmp")));
    }
}
