use super::structs::{CloseCodes, CloseMessage, IpcClientMap, IpcCommand, IpcFrame, IpcMessage};
use anyhow::Result;
use owo_colors::OwoColorize;
use std::{
    collections::HashMap,
    env,
    io::ErrorKind,
    sync::atomic::{self, AtomicUsize},
};
use tokio::{
    io::AsyncWriteExt,
    net::{UnixListener, UnixStream},
    select,
    sync::{broadcast, mpsc, Mutex},
    task,
};
use tracing::{debug, info, warn};

static SOCKET_ID: AtomicUsize = AtomicUsize::new(0);

pub struct IpcServer {
    pub path: String,
    _ipc_client_map: IpcClientMap,
    rx_msg: mpsc::Receiver<(usize, IpcMessage)>,
}

impl IpcServer {
    pub async fn try_bind() -> Result<IpcServer> {
        let bind_directory = env::var("XDG_RUNTIME_DIR")
            .or_else(|_| env::var("TMPDIR"))
            .or_else(|_| env::var("TMP"))
            .or_else(|_| env::var("TEMP"))
            .unwrap_or("/tmp".to_string());

        for i in 0u8..10 {
            let path = format!("{}/discord-ipc-{}", bind_directory, i);
            let listener = UnixListener::bind(path.clone());
            match listener {
                Ok(listener) => {
                    info!(
                        "{} {}",
                        "Bound to IPC server at".green(),
                        path.yellow().bold(),
                    );
                    let ipc_client_map = IpcClientMap::new(Mutex::new(HashMap::new()));
                    let (tx_msg, rx_msg) = mpsc::channel(1);
                    task::spawn(Self::accept_loop(listener, tx_msg, ipc_client_map.clone()));
                    return Ok(IpcServer {
                        path,
                        rx_msg,
                        _ipc_client_map: ipc_client_map,
                    });
                }
                Err(e) => match e.kind() {
                    ErrorKind::AddrInUse => {
                        info!(
                            "{} {}, {}",
                            "Socket is not available at".yellow().bold(),
                            path.red().bold(),
                            "Trying next path...".cyan().bold(),
                        );
                        continue;
                    }
                    _ => {
                        info!("Error: {:?}", e);
                        return Err(e.into());
                    }
                },
            }
        }
        Err(anyhow::anyhow!(
            "Failed to bind to IPC server (ran out of paths)"
        ))
    }

    pub async fn accept_loop(
        listener: UnixListener,
        tx_msg: mpsc::Sender<(usize, IpcMessage)>,
        ipc_client_map: IpcClientMap,
    ) -> Result<()> {
        loop {
            let (stream, _) = listener.accept().await?;
            let (tx_cmd, rx_cmd) = broadcast::channel(1);

            ipc_client_map
                .lock()
                .await
                .insert(SOCKET_ID.load(atomic::Ordering::SeqCst), tx_cmd);

            task::spawn(Self::handle_stream(
                stream,
                SOCKET_ID.load(atomic::Ordering::SeqCst),
                rx_cmd,
                tx_msg.clone(),
            ));

            SOCKET_ID.fetch_add(1, atomic::Ordering::SeqCst);
        }
    }

    async fn handle_stream(
        mut stream: UnixStream,
        socket_id: usize,
        mut rx: broadcast::Receiver<IpcCommand>,
        tx: mpsc::Sender<(usize, IpcMessage)>,
    ) -> Result<()> {
        let mut handshake_done = false;
        loop {
            select! {
                event = IpcMessage::try_decode(&mut stream) => {
                    if let Ok(event) = event {
                        match event {
                            IpcMessage::Handshake(handshake_msg) => {
                                if handshake_done {
                                    return Err(anyhow::anyhow!("Handshake sent twice"));
                                }

                                if handshake_msg.version != 1 {
                                    debug!("Invalid Handshake version: {}", handshake_msg.version);
                                    stream
                                        .write_all(
                                            IpcMessage::Close(CloseMessage {
                                                code: CloseCodes::InvalidVersion,
                                                message: "".into(),
                                            })
                                            .try_encode()?
                                            .as_ref(),
                                        )
                                        .await?;
                                    return Err(anyhow::anyhow!("Invalid Handshake version"));
                                }

                                if handshake_msg.client_id.is_empty() {
                                    debug!("Invalid Client ID: {}", handshake_msg.client_id);
                                    stream
                                        .write_all(
                                            IpcMessage::Close(CloseMessage {
                                                code: CloseCodes::InvalidClientID,
                                                message: "".into(),
                                            })
                                            .try_encode()?
                                            .as_ref(),
                                        )
                                        .await?;
                                    return Err(anyhow::anyhow!("Invalid Client ID"));
                                }
                                handshake_done = true;
                                tx.send((socket_id, IpcMessage::Handshake(handshake_msg)))
                                    .await?;
                            }

                            IpcMessage::Ping(data) => {
                                stream
                                    .write_all(
                                        IpcMessage::Pong(data.clone()).try_encode()?.as_ref(),
                                    )
                                    .await?;
                                tx.send((socket_id, IpcMessage::Ping(data))).await?;
                            }

                            IpcMessage::Pong(data) => {
                                tx.send((socket_id, IpcMessage::Pong(data))).await?;
                            }

                            IpcMessage::Frame(data) => {
                                if !handshake_done {
                                    return Err(anyhow::anyhow!(
                                        "Frame Sent before Handshake wasn't done"
                                    ));
                                }
                                stream.write_all(IpcMessage::Frame(IpcFrame { args:None, data: None, cmd: "SET_ACTIVITY".to_string(), nonce: data.nonce.clone(), evt:None }).try_encode()?.as_ref()).await?;
                                tx.send((socket_id, IpcMessage::Frame(data))).await?;
                            }

                            IpcMessage::Close(msg) => {
                                tx.send((socket_id, IpcMessage::Close(msg))).await?;
                                break Ok(());
                            }
                        }
                    }
                }
                cmd = rx.recv() => {
                    if let Ok(cmd) = cmd {
                        stream.write_all(cmd.try_encode()?.as_ref()).await?;
                        if matches!(cmd, IpcCommand::Close) {
                            break Ok(());
                        }
                    }
                }
            }
        }
    }

    pub async fn send(&self, socket_id: usize, command: IpcCommand) -> Result<()> {
        if let Some(sender) = self._ipc_client_map.lock().await.get(&socket_id) {
            sender.send(command)?;
        } else {
            debug!("Failed to send IPC Command ({})", socket_id);
        }
        Ok(())
    }

    pub async fn recv(&mut self) -> Option<(usize, IpcMessage)> {
        self.rx_msg.recv().await
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.path) {
            warn!("Failed to remove IPC socket file at {}", &self.path);
            warn!("Error: {:?}", e);
        }
    }
}
