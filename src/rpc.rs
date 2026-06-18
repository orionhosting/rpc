use discord_rich_presence::{
    DiscordIpc, DiscordIpcClient,
    activity::{self, Assets},
};
use std::{
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tracing::{info, warn};

use crate::{AppCommand, config::AppConfig};

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("failed to spawn rpc thread")]
    SpawnThread,

    #[error("rpc thread panicked")]
    ThreadPanicked,

    #[error("failed to set discord activity: {0}")]
    SetActivity(String),
}

#[derive(Debug)]
pub enum RpcCommand {
    /// Restart the RPC.
    Restart,
    /// Stop the RPC.
    Stop,
    /// Shutdown the RPC thread.
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum RpcState {
    Connected,
    Disconnected,
}

pub struct DiscordWorker {
    tx: mpsc::Sender<RpcCommand>,
    handle: thread::JoinHandle<()>,
}

impl DiscordWorker {
    /// Spawn the worker and its thread.
    pub fn spawn(config: AppConfig, main_tx: mpsc::Sender<AppCommand>) -> Result<Self, RpcError> {
        info!("spawning the rpc worker");
        let (tx, rx) = mpsc::channel();

        let handle = thread::Builder::new()
            .name("discord-rpc".into())
            .spawn(move || worker_loop(config, rx, main_tx))
            .map_err(|_| RpcError::SpawnThread)?;

        Ok(Self { tx, handle })
    }

    /// Get a sender channel.
    pub fn sender(&self) -> mpsc::Sender<RpcCommand> {
        self.tx.clone()
    }

    /// Shutdown the app.
    pub fn shutdown(self) -> Result<(), RpcError> {
        let _ = self.tx.send(RpcCommand::Shutdown);
        self.handle.join().map_err(|_| RpcError::ThreadPanicked)?;

        Ok(())
    }
}

fn worker_loop(
    config: AppConfig,
    rx: mpsc::Receiver<RpcCommand>,
    main_tx: mpsc::Sender<AppCommand>,
) {
    thread::sleep(Duration::from_secs(1)); // wait for the tray to start
    let mut client = connect(&config, &main_tx);

    while let Ok(cmd) = rx.recv() {
        match cmd {
            RpcCommand::Restart => {
                if let Some(mut existing_client) = client.take() {
                    let _ = existing_client.close();
                }

                client = connect(&config, &main_tx);

                if let Some(client) = client.as_mut() {
                    if let Err(err) = set_activity(client, &config) {
                        warn!("failed to set activity: {err}");
                        let _ = main_tx.send(AppCommand::RpcStateChanged(RpcState::Disconnected));
                    }
                }
            }

            RpcCommand::Stop => {
                if let Some(client) = client.as_mut() {
                    if let Err(err) = client.clear_activity() {
                        warn!("failed to clear activity: {err}");
                    }
                    let _ = main_tx.send(AppCommand::RpcStateChanged(RpcState::Disconnected));
                }
            }

            RpcCommand::Shutdown => break,
        }
    }

    if let Some(mut client) = client {
        let _ = client.clear_activity();
        let _ = client.close();
    }
}

fn connect(config: &AppConfig, main_tx: &mpsc::Sender<AppCommand>) -> Option<DiscordIpcClient> {
    let mut client = DiscordIpcClient::new(config.discord_app_id);

    for attempt in 1..=config.connect_retries {
        match client.connect() {
            Ok(_) => {
                info!("connected to Discord");
                let _ = main_tx.send(AppCommand::RpcStateChanged(RpcState::Connected));
                return Some(client);
            }

            Err(err) => {
                warn!(
                    "connection attempt {}/{} failed: {}",
                    attempt, config.connect_retries, err
                );

                let _ = main_tx.send(AppCommand::RpcStateChanged(RpcState::Disconnected));
                thread::sleep(config.connect_delay);
            }
        }
    }

    None
}

/// Set the RPC activity.
fn set_activity(client: &mut DiscordIpcClient, config: &AppConfig) -> Result<(), RpcError> {
    let start = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let activity = activity::Activity::new()
        .name(config.rpc_name)
        .details(config.rpc_details)
        .state(config.rpc_state)
        .assets(
            Assets::new()
                .large_text(config.rpc_asset_large_text)
                .large_url(config.rpc_url),
        )
        .state_url(config.rpc_url)
        .timestamps(activity::Timestamps::new().start(start));

    client
        .set_activity(activity)
        .map_err(|e| RpcError::SetActivity(e.to_string()))?;

    Ok(())
}
