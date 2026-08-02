use std::time::Duration;

pub const DEFAULT_URL: &str = "https://bbb.pulsar.gg/";

const ENV_URL: &str = "LITECRAZY_URL";
const ENV_INTERVAL: &str = "LITECRAZY_INTERVAL";
const ENV_THRESHOLD: &str = "LITECRAZY_LOW_THRESHOLD";
const ENV_BROWSER: &str = "LITECRAZY_BROWSER";
const ENV_BROWSER_ARGS: &str = "LITECRAZY_BROWSER_ARGS";
const ENV_PAUSE_MINUTES: &str = "LITECRAZY_PAUSE_MINUTES";
const ENV_WINDOW_MODE: &str = "LITECRAZY_WINDOW_MODE";

const DEFAULT_INTERVAL_SECS: u64 = 60;
const INTERVAL_MIN_SECS: u64 = 10;
const INTERVAL_MAX_SECS: u64 = 3600;

const DEFAULT_THRESHOLD: u8 = 20;

const DEFAULT_PAUSE_MINUTES: u64 = 10;
const PAUSE_MAX_MINUTES: u64 = 240;

/// URL opened by the "Open Configurator" menu item.
pub fn configurator_url() -> String {
    std::env::var(ENV_URL)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_URL.to_string())
}

pub fn battery_interval() -> Duration {
    Duration::from_secs(parse_u64(
        std::env::var(ENV_INTERVAL).ok(),
        DEFAULT_INTERVAL_SECS,
        INTERVAL_MIN_SECS,
        INTERVAL_MAX_SECS,
    ))
}

/// Low-battery notification threshold in percent. `0` disables notifications.
pub fn low_battery_threshold() -> u8 {
    parse_u64(
        std::env::var(ENV_THRESHOLD).ok(),
        DEFAULT_THRESHOLD as u64,
        0,
        100,
    ) as u8
}

/// Explicit browser binary (name or absolute path). Skips auto-detection.
pub fn browser_override() -> Option<String> {
    std::env::var(ENV_BROWSER)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    App,
    Tab,
}

pub fn window_mode() -> WindowMode {
    match std::env::var(ENV_WINDOW_MODE)
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("tab") | Some("window") => WindowMode::Tab,
        _ => WindowMode::App,
    }
}

pub fn browser_extra_args() -> Vec<String> {
    std::env::var(ENV_BROWSER_ARGS)
        .ok()
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

pub fn poll_pause_duration() -> Duration {
    Duration::from_secs(
        parse_u64(
            std::env::var(ENV_PAUSE_MINUTES).ok(),
            DEFAULT_PAUSE_MINUTES,
            0,
            PAUSE_MAX_MINUTES,
        ) * 60,
    )
}

fn parse_u64(raw: Option<String>, default: u64, min: u64, max: u64) -> u64 {
    match raw.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => match s.parse::<u64>() {
            Ok(v) => v.clamp(min, max),
            Err(_) => {
                log::warn!("Could not parse '{}' as a number, using {}", s, default);
                default
            }
        },
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_falls_back_to_default() {
        assert_eq!(parse_u64(None, 60, 10, 3600), 60);
        assert_eq!(parse_u64(Some(String::new()), 60, 10, 3600), 60);
        assert_eq!(parse_u64(Some("   ".into()), 60, 10, 3600), 60);
    }

    #[test]
    fn garbage_falls_back_to_default() {
        assert_eq!(parse_u64(Some("sixty".into()), 60, 10, 3600), 60);
        assert_eq!(parse_u64(Some("-5".into()), 60, 10, 3600), 60);
    }

    #[test]
    fn values_are_clamped() {
        assert_eq!(parse_u64(Some("1".into()), 60, 10, 3600), 10);
        assert_eq!(parse_u64(Some("99999".into()), 60, 10, 3600), 3600);
    }

    #[test]
    fn surrounding_whitespace_is_tolerated() {
        assert_eq!(parse_u64(Some(" 120 ".into()), 60, 10, 3600), 120);
    }

    #[test]
    fn threshold_zero_is_allowed() {
        assert_eq!(parse_u64(Some("0".into()), 20, 0, 100), 0);
    }
}
