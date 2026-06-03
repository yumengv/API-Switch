use crate::state_version::TrayVersions;

#[cfg(all(feature = "tray", not(mobile)))]
use std::sync::{Mutex, OnceLock};

#[cfg(all(feature = "tray", not(mobile)))]
const TRAY_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

#[cfg(all(feature = "tray", not(mobile)))]
const TRAY_INTERACTION_PAUSE: std::time::Duration = std::time::Duration::from_secs(6);

#[cfg(all(feature = "tray", not(mobile)))]
static TRAY_REFRESH_PAUSED_UNTIL: OnceLock<Mutex<Option<std::time::Instant>>> = OnceLock::new();

pub(crate) fn should_refresh_tray(
    previous: TrayVersions,
    current: TrayVersions,
    interaction_pause_active: bool,
) -> bool {
    current.has_changed_since(previous) && !interaction_pause_active
}

pub(crate) fn next_seen_versions(
    previous: TrayVersions,
    current: TrayVersions,
    refreshed: bool,
) -> TrayVersions {
    if refreshed {
        current
    } else {
        previous
    }
}

#[cfg(all(feature = "tray", not(mobile)))]
pub(crate) fn mark_tray_interaction() {
    let lock = TRAY_REFRESH_PAUSED_UNTIL.get_or_init(|| Mutex::new(None));
    if let Ok(mut paused_until) = lock.lock() {
        *paused_until = Some(std::time::Instant::now() + TRAY_INTERACTION_PAUSE);
    }
}

#[cfg(all(feature = "tray", not(mobile)))]
fn interaction_pause_active() -> bool {
    let lock = TRAY_REFRESH_PAUSED_UNTIL.get_or_init(|| Mutex::new(None));
    let Ok(mut paused_until) = lock.lock() else {
        return true;
    };

    match *paused_until {
        Some(until) if std::time::Instant::now() < until => true,
        Some(_) => {
            *paused_until = None;
            false
        }
        None => false,
    }
}

#[cfg(all(feature = "tray", not(mobile)))]
pub(crate) fn start_tray_refresh_consumer(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_seen = crate::state_version::tray_versions();

        loop {
            tokio::time::sleep(TRAY_POLL_INTERVAL).await;
            let current = crate::state_version::tray_versions();

            if should_refresh_tray(last_seen, current, interaction_pause_active()) {
                crate::refresh_tray_if_enabled(&app);
                last_seen = next_seen_versions(last_seen, current, true);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{next_seen_versions, should_refresh_tray};
    use crate::state_version::TrayVersions;

    #[test]
    fn refreshes_when_tray_relevant_versions_change_without_pause() {
        let previous = TrayVersions {
            pool: 1,
            channel: 1,
            token: 1,
        };
        let current = TrayVersions {
            pool: 2,
            channel: 1,
            token: 1,
        };

        assert!(should_refresh_tray(previous, current, false));
    }

    #[test]
    fn skips_refresh_during_recent_tray_interaction() {
        let previous = TrayVersions {
            pool: 1,
            channel: 1,
            token: 1,
        };
        let current = TrayVersions {
            pool: 2,
            channel: 1,
            token: 1,
        };

        assert!(!should_refresh_tray(previous, current, true));
    }

    #[test]
    fn keeps_previous_versions_when_refresh_is_paused() {
        let previous = TrayVersions {
            pool: 1,
            channel: 1,
            token: 1,
        };
        let current = TrayVersions {
            pool: 2,
            channel: 1,
            token: 1,
        };

        assert_eq!(next_seen_versions(previous, current, false), previous);
        assert!(should_refresh_tray(previous, current, false));
    }
}
