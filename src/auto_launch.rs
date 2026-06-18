use auto_launch::AutoLaunchBuilder;
use std::env;

use crate::config::APP_NAME;

/// Get the auto launcher builder.
fn get_auto_launch() -> Result<auto_launch::AutoLaunch, String> {
    let current_exe =
        env::current_exe().map_err(|e| format!("Failed to get current exe path: {e}"))?;

    let exe_str = current_exe
        .to_str()
        .ok_or_else(|| "Invalid executable path characters".to_string())?;

    AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(exe_str)
        .set_args(&["--autostart"])
        .build()
        .map_err(|e| format!("Failed to build auto-launch helper: {e}"))
}

/// Checks if auto-launch is enabled and valid.
pub fn is_startup_enabled() -> bool {
    if let Ok(auto) = get_auto_launch() {
        auto.is_enabled().unwrap_or(false)
    } else {
        false
    }
}

/// Enable or disable auto-launch.
pub fn set_startup(enable: bool) -> Result<(), String> {
    let auto = get_auto_launch()?;
    if enable {
        auto.enable()
            .map_err(|e| format!("Failed to enable startup: {e}"))
    } else {
        auto.disable()
            .map_err(|e| format!("Failed to disable startup: {e}"))
    }
}

/// Call on app startup. If the registry key exists but the path is wrong
/// it removes the key out of the registry.
pub fn handle_startup_registry() {
    if let Ok(auto) = get_auto_launch() {
        // if disabled or the path is invalid
        if !auto.is_enabled().unwrap_or(false) {
            // this will deletes it from the registry
            let _ = auto.disable();
        }
    }
}
