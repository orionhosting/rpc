#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use interprocess::TryClone;
use interprocess::local_socket::traits::Listener;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};
use thiserror::Error;
use tracing::{error, info};

use crate::auto_launch::handle_startup_registry;
use crate::config::AppConfig;
use crate::rpc::{DiscordWorker, RpcCommand, RpcState};
use crate::ui::UiCommand;

mod auto_launch;
mod config;
mod instance;
mod logging;
mod rpc;
mod tray;
mod ui;
mod updater;

type UiStream = Arc<Mutex<Option<Stream>>>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("rpc error: {0}")]
    Rpc(#[from] crate::rpc::RpcError),
    #[error("tray error: {0}")]
    Tray(#[from] crate::tray::TrayError),
    #[error("instance error: {0}")]
    Instance(#[from] crate::instance::InstanceError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
pub enum AppCommand {
    UiCommand(UiCommand),
    RpcCommand(RpcCommand),
    RpcStateChanged(RpcState),
    ExecuteUpdate,
    /// Shutdown everything.
    Quit,
}

fn main() -> Result<(), AppError> {
    logging::init();

    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--ui") {
        let ipc_name = args.get(pos + 1).expect("Pipe name is missing").clone();
        let show_on_start = args.iter().any(|a| a == "--show");
        ui::run_ui(ipc_name, show_on_start);
        return Ok(());
    }

    let is_autostart = args.iter().any(|a| a == "--autostart");
    run_backend(is_autostart)
}

fn run_backend(is_autostart: bool) -> Result<(), AppError> {
    info!("starting the backend");

    let (tx, rx) = mpsc::channel();
    let tray_tx = tx.clone();
    let instance_tx = tx.clone();
    let updater_tx = tx.clone();

    // Stop the process if another instance is running
    match instance::init_instance(move || {
        let _ = instance_tx.send(AppCommand::UiCommand(UiCommand::ShowWindow));
    })? {
        instance::Role::Secondary => {
            info!("another instance is already running, exiting...");
            return Ok(());
        }
        instance::Role::Primary => {}
    }

    // Clean the registry
    handle_startup_registry();

    // Start the auto-updater worker
    updater::spawn_update_worker(updater_tx, Duration::from_hours(2));

    // Start the Discord RPC worker
    let rpc = DiscordWorker::spawn(AppConfig::default(), tx.clone())?;
    let rpc_tx = rpc.sender();
    let _ = rpc_tx.send(RpcCommand::Restart);

    // Start the UI worker
    let ui_stream = Arc::new(Mutex::new(None));
    spawn_ui_worker(ui_stream.clone(), !is_autostart, tx.clone());

    // Start the commands worker
    let (rpc_status_tx, rpc_status_rx) = mpsc::channel::<RpcState>();
    spawn_commands_worker(tx, rx, ui_stream, rpc_tx, rpc_status_tx);

    // Start the tray in the main thread
    if let Err(e) = tray::run_tray(tray_tx, rpc_status_rx) {
        error!("tray error: {e}");
    }

    // Quit the app

    rpc.shutdown()?;
    Ok(())
}

/// Start the UI, and restart it when it closes.
fn spawn_ui_worker(ui_stream: UiStream, mut show_on_start: bool, tx: Sender<AppCommand>) {
    info!("spawning the ui worker");

    let ipc_name = format!("@orion_rpc_{}", std::process::id());
    let listener = ListenerOptions::new()
        .name(ipc_name.clone().to_ns_name::<GenericNamespaced>().unwrap())
        .create_sync()
        .expect("Failed to create the named pipe");

    thread::spawn(move || {
        let exe = std::env::current_exe().unwrap();

        loop {
            let mut command = Command::new(&exe);
            command.arg("--ui").arg(&ipc_name);

            if show_on_start {
                show_on_start = false;
                command.arg("--show");
            }

            let mut child = command.spawn().expect("Failed to start the UI process");

            if let Ok(stream) = listener.accept() {
                // Listen for ui commands
                if let Ok(stream_read) = stream.try_clone() {
                    let tx_clone = tx.clone();
                    thread::spawn(move || {
                        use std::io::BufRead;
                        let reader = std::io::BufReader::new(stream_read);
                        for line in reader.lines() {
                            if let Ok(cmd) = line {
                                if cmd == "ExecuteUpdate" {
                                    let _ = tx_clone.send(AppCommand::ExecuteUpdate);
                                }
                            }
                        }
                    });
                }
                *ui_stream.lock().unwrap() = Some(stream);
            }

            let _ = child.wait();
            *ui_stream.lock().unwrap() = None; // Clean the stream

            thread::sleep(Duration::from_millis(100));
        }
    });
}

/// Starts the thread handling the app commands.
fn spawn_commands_worker(
    tx: Sender<AppCommand>,
    rx: Receiver<AppCommand>,
    ui_stream: UiStream,
    rpc_tx: Sender<RpcCommand>,
    rpc_status_tx: Sender<RpcState>,
) {
    info!("spawning the commands worker");
    let is_updating = Arc::new(AtomicBool::new(false));

    thread::spawn(move || {
        while let Ok(cmd) = rx.recv() {
            match cmd {
                AppCommand::UiCommand(cmd) => {
                    if let Some(stream) = ui_stream.lock().unwrap().as_mut() {
                        if let Ok(json_payload) = serde_json::to_string(&cmd) {
                            let _ = writeln!(stream, "{}", json_payload);
                        }
                    }
                }
                AppCommand::RpcCommand(cmd) => {
                    let _ = rpc_tx.send(cmd);
                }
                AppCommand::RpcStateChanged(state) => {
                    let _ = rpc_status_tx.send(state);
                }
                AppCommand::ExecuteUpdate => {
                    updater::run_update_install(Arc::clone(&is_updating), tx.clone());
                }
                AppCommand::Quit => {
                    let _ = rpc_tx.send(rpc::RpcCommand::Shutdown);
                    if let Some(stream) = ui_stream.lock().unwrap().as_mut() {
                        if let Ok(json_payload) = serde_json::to_string(&UiCommand::Quit) {
                            let _ = writeln!(stream, "{}", json_payload);
                        }
                    }
                    std::process::exit(0);
                }
            }
        }
    });
}
