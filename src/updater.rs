use std::env;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use semver::Version;
use tracing::{info, warn};

use crate::AppCommand;
use crate::config::{CURRENT_APP_VERSION, REPO_NAME, REPO_OWNER};
use crate::ui::UiCommand;

/// Spawns a background thread that checks for updates every `interval`.
/// The thread is killed when an update is found.
pub fn spawn_update_worker(tx: Sender<AppCommand>, interval: Duration) {
    info!("spawning the update worker");

    thread::spawn(move || {
        thread::sleep(Duration::from_secs(20));
        loop {
            // Check for updates, ignoring network errors
            match check_for_updates_silent() {
                Ok(Some(latest_version)) => {
                    let _ = tx.send(AppCommand::UiCommand(UiCommand::UpdateAvailable {
                        version: latest_version,
                    }));
                    break; // Stop the loop
                }
                Ok(_) => {}
                Err(e) => {
                    warn!("failed to check for updates: {}", e.to_string());
                }
            }
            thread::sleep(interval);
        }
    });
}

fn check_for_updates_silent() -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    info!("checking for an update...");

    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()?
        .fetch()?;

    if releases.is_empty() {
        return Ok(None);
    }

    let latest_version_str = &releases[0].version;
    let current_version = Version::parse(CURRENT_APP_VERSION)?;
    let latest_version = Version::parse(latest_version_str)?;

    if latest_version > current_version {
        Ok(Some(latest_version_str.clone()))
    } else {
        Ok(None)
    }
}

pub fn run_update_install(is_updating: Arc<AtomicBool>, tx: Sender<AppCommand>) {
    info!("starting update installation");

    if is_updating
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return; // Already installing
    }

    thread::spawn(move || {
        let current_exe = match env::current_exe() {
            Ok(path) => path,
            Err(e) => {
                let _ = tx.send(AppCommand::UiCommand(UiCommand::UpdateFailed {
                    reason: e.to_string(),
                }));
                is_updating.store(false, Ordering::SeqCst);
                return;
            }
        };

        let bin_name = current_exe
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("orion-rpc.exe");

        let status = self_update::backends::github::Update::configure()
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .bin_name(bin_name)
            .show_download_progress(false)
            .current_version(CURRENT_APP_VERSION)
            .build()
            .and_then(|u| u.update());

        match status {
            Ok(s) if s.updated() => {
                // Spawn the new executable
                let _ = Command::new(current_exe).spawn();

                // Success, quit the app
                let _ = tx.send(AppCommand::Quit);
            }
            Ok(_) => {
                let _ = tx.send(AppCommand::UiCommand(UiCommand::UpdateFailed {
                    reason: "No update was applied.".to_string(),
                }));
                is_updating.store(false, Ordering::SeqCst);
            }
            Err(e) => {
                let _ = tx.send(AppCommand::UiCommand(UiCommand::UpdateFailed {
                    reason: e.to_string(),
                }));
                is_updating.store(false, Ordering::SeqCst);
            }
        }
    });
}
