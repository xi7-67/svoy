//! LocalSend integration for sharing images over the local network.
//!
//! This module provides a `ShareManager` that wraps the `localsend` crate's `Client`
//! and handles asynchronous discovery and file transfer in a background thread,
//! communicating state changes back to the UI thread via channels.

use localsend::Client;
use localsend::models::device::DeviceInfo;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

/// Events sent from the share manager to the UI.
#[derive(Debug, Clone)]
pub enum ShareEvent {
    /// A new peer device was discovered.
    PeerDiscovered { fingerprint: String, device: DeviceInfo, addr: SocketAddr },
    /// A peer device was removed or became unreachable.
    PeerLost { fingerprint: String },
    /// File transfer started.
    TransferStarted { peer_fingerprint: String, file_path: PathBuf },
    /// File transfer completed successfully.
    TransferComplete { peer_fingerprint: String },
    /// File transfer failed.
    TransferFailed { peer_fingerprint: String, error: String },
    /// An error occurred in the background service.
    Error(String),
}

/// Commands sent from the UI to the share manager.
#[derive(Debug)]
pub enum ShareCommand {
    /// Request to send a file to a peer.
    SendFile { peer_fingerprint: String, file_path: PathBuf },
    /// Stop the share manager.
    Shutdown,
}

/// A wrapper around the LocalSend client that manages async operations.
pub struct ShareManager {
    /// Channel to send commands to the background task.
    command_tx: mpsc::UnboundedSender<ShareCommand>,
    /// Channel to receive events from the background task.
    event_rx: Arc<Mutex<mpsc::UnboundedReceiver<ShareEvent>>>,
    /// Shared peers list (fingerprint -> (SocketAddr, DeviceInfo)).
    peers: Arc<Mutex<HashMap<String, (SocketAddr, DeviceInfo)>>>,
}

impl ShareManager {
    /// Creates and starts a new ShareManager.
    /// 
    /// This spawns a background thread with a Tokio runtime to handle
    /// LocalSend discovery and file transfers.
    pub fn new() -> Result<Self, String> {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel::<ShareCommand>();
        let (event_tx, event_rx) = mpsc::unbounded_channel::<ShareEvent>();
        let peers: Arc<Mutex<HashMap<String, (SocketAddr, DeviceInfo)>>> = Arc::new(Mutex::new(HashMap::new()));
        let peers_clone = peers.clone();

        // Spawn a background thread for async operations
        std::thread::spawn(move || {
            let rt = match Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = event_tx.send(ShareEvent::Error(format!("Failed to create runtime: {}", e)));
                    return;
                }
            };

            rt.block_on(async move {
                // Initialize the LocalSend client
                let mut client_obj = match Client::default().await {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = event_tx.send(ShareEvent::Error(format!("Failed to create LocalSend client: {:?}", e)));
                        return;
                    }
                };

                // Replace the internal http_client with one that allows invalid certs (needed for LocalSend protocol)
                match reqwest::Client::builder()
                    .danger_accept_invalid_certs(true)
                    .build() 
                {
                    Ok(new_http) => client_obj.http_client = new_http,
                    Err(e) => {
                        let _ = event_tx.send(ShareEvent::Error(format!("Failed to configure HTTP client: {:?}", e)));
                        return;
                    }
                }
                
                let client = Arc::new(client_obj);

                // Start the client (discovery and HTTP server)
                if let Err(e) = client.start().await {
                    let _ = event_tx.send(ShareEvent::Error(format!("Failed to start LocalSend client: {:?}", e)));
                    return;
                }

                // Spawn a task to periodically sync peers
                let client_peers = client.clone();
                let peers_for_sync = peers_clone.clone();
                let event_tx_sync = event_tx.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        
                        let current_peers = client_peers.peers.lock().await;
                        let mut local_peers = peers_for_sync.lock().unwrap();
                        
                        // Check for new peers
                        for (fingerprint, (addr, info)) in current_peers.iter() {
                            if !local_peers.contains_key(fingerprint) {
                                let _ = event_tx_sync.send(ShareEvent::PeerDiscovered {
                                    fingerprint: fingerprint.clone(),
                                    device: info.clone(),
                                    addr: *addr,
                                });
                            }
                            local_peers.insert(fingerprint.clone(), (*addr, info.clone()));
                        }
                        
                        // Check for lost peers
                        let lost: Vec<String> = local_peers.keys()
                            .filter(|k| !current_peers.contains_key(*k))
                            .cloned()
                            .collect();
                        for fingerprint in lost {
                            local_peers.remove(&fingerprint);
                            let _ = event_tx_sync.send(ShareEvent::PeerLost { fingerprint });
                        }
                    }
                });

                // Handle commands from the UI
                while let Some(cmd) = command_rx.recv().await {
                    match cmd {
                        ShareCommand::SendFile { peer_fingerprint, file_path } => {
                            let _ = event_tx.send(ShareEvent::TransferStarted {
                                peer_fingerprint: peer_fingerprint.clone(),
                                file_path: file_path.clone(),
                            });
                            
                            match client.send_file(peer_fingerprint.clone(), file_path.clone()).await {
                                Ok(()) => {
                                    let _ = event_tx.send(ShareEvent::TransferComplete {
                                        peer_fingerprint,
                                    });
                                }
                                Err(e) => {
                                    let _ = event_tx.send(ShareEvent::TransferFailed {
                                        peer_fingerprint,
                                        error: format!("{:?}", e),
                                    });
                                }
                            }
                        }
                        ShareCommand::Shutdown => {
                            break;
                        }
                    }
                }
            });
        });

        Ok(Self {
            command_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            peers,
        })
    }

    /// Sends a file to a peer device.
    pub fn send_file(&self, peer_fingerprint: String, file_path: PathBuf) -> Result<(), String> {
        self.command_tx
            .send(ShareCommand::SendFile { peer_fingerprint, file_path })
            .map_err(|e| format!("Failed to send command: {}", e))
    }

    /// Gets the current list of discovered peers.
    pub fn get_peers(&self) -> HashMap<String, (SocketAddr, DeviceInfo)> {
        self.peers.lock().unwrap().clone()
    }

    /// Polls for events from the background task (non-blocking).
    pub fn poll_events(&self) -> Vec<ShareEvent> {
        let mut events = Vec::new();
        if let Ok(mut rx) = self.event_rx.lock() {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        events
    }

    /// Shuts down the share manager.
    pub fn shutdown(&self) {
        let _ = self.command_tx.send(ShareCommand::Shutdown);
    }
}

impl Drop for ShareManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}
