use std::time::Duration;

pub const APP_NAME: &str = "Orion RPC";

pub const REPO_URL: &str = "https://github.com/orionhosting/rpc";
pub const REPO_OWNER: &str = "orionhosting";
pub const REPO_NAME: &str = "rpc";

pub const CURRENT_APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Application configuration.
#[derive(Debug)]
pub struct AppConfig {
    pub discord_app_id: &'static str,
    pub rpc_name: &'static str,
    pub rpc_details: &'static str,
    pub rpc_state: &'static str,
    pub rpc_asset_large_text: &'static str,
    pub rpc_url: &'static str,
    pub connect_retries: u32,
    pub connect_delay: Duration,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            discord_app_id: "1477495267920973907",
            rpc_name: "Orion Hosting",
            rpc_details: "Free Host",
            rpc_state: "orionhost.xyz",
            rpc_asset_large_text: "Join Orion!",
            rpc_url: "https://orionhost.xyz",
            connect_retries: 5,
            connect_delay: Duration::from_secs(3),
        }
    }
}
