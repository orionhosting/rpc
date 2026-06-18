use std::{
    io::{Read, Write},
    thread,
};

use interprocess::local_socket::{GenericNamespaced, ListenerOptions, prelude::*};
use thiserror::Error;

const SOCKET_NAME: &str = "orion-rpc-v1";

#[derive(Debug, Error)]
pub enum InstanceError {
    #[error("failed to bind ipc socket: {0}")]
    Bind(std::io::Error),

    #[error("failed to connect to running instance: {0}")]
    Connect(std::io::Error),

    #[error("failed to signal running instance: {0}")]
    Signal(std::io::Error),
}

pub enum Role {
    /// We are the first instance.
    Primary,
    /// Another instance is already running.
    Secondary,
}

/// Checks for a running instance.
///
/// - If another instance is already running, we send it a `show` signal.
/// - If no instances was running, we listen for a future `show` signal.
pub fn init_instance<F: Fn() + Send + 'static>(on_show: F) -> Result<Role, InstanceError> {
    let name = SOCKET_NAME
        .to_ns_name::<GenericNamespaced>()
        .expect("invalid socket name");

    match ListenerOptions::new().name(name.clone()).create_sync() {
        // No instance running
        Ok(listener) => {
            thread::Builder::new()
                .name("ipc-listener".into())
                .spawn(move || {
                    for conn in listener.incoming() {
                        let Ok(mut conn) = conn else { continue };
                        let mut buf = [0u8; 4];
                        if conn.read_exact(&mut buf).is_ok() && &buf == b"show" {
                            on_show();
                        }
                    }
                })
                .expect("failed to spawn ipc-listener thread");

            Ok(Role::Primary)
        }
        // Another instance is running
        Err(_) => {
            let mut stream = LocalSocketStream::connect(name).map_err(InstanceError::Connect)?;

            stream.write_all(b"show").map_err(InstanceError::Signal)?;

            Ok(Role::Secondary)
        }
    }
}
