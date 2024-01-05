use anyhow::Result;
use bytes::{Buf, BufMut, BytesMut};
use futures_util::lock::Mutex;
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use serde_json::{from_slice, to_vec, Value};
use std::{
    collections::HashMap,
    env,
    io::ErrorKind,
    sync::{
        atomic::{self, AtomicUsize},
        Arc,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    select,
    sync::{broadcast, mpsc},
    task,
};
use tracing::{debug, info, warn};

#[derive(Debug, Serialize, Deserialize)]
pub struct HandshakeMessage {
    #[serde(rename = "v")]
    pub version: i32,
    pub client_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CloseMessage {
    code: CloseCodes,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcFrameArgs {
    pub activity: Value,
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcFrame {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<IpcFrameArgs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    pub cmd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evt: Option<String>,
    pub nonce: String,
}

#[derive(Debug)]
pub enum IpcMessage {
    Handshake(HandshakeMessage),
    Frame(IpcFrame),
    Close(CloseMessage),
    Ping(Value),
    Pong(Value),
}

impl IpcMessage {
    pub fn try_encode(&self) -> Result<BytesMut> {
        let mut buffer = BytesMut::new();

        // Honestly, I don't have a better way to do this
        match self {
            IpcMessage::Handshake(data) => {
                buffer.put_i32_le(0);
                let data = to_vec(data)?;
                buffer.put_i32_le(data.len() as i32);
                buffer.put_slice(data.as_slice());
            }
            IpcMessage::Frame(data) => {
                buffer.put_i32_le(1);
                let data = to_vec(data)?;
                buffer.put_i32_le(data.len() as i32);
                buffer.put_slice(data.as_slice());
            }
            IpcMessage::Close(data) => {
                buffer.put_i32_le(2);
                let data = to_vec(data)?;
                buffer.put_i32_le(data.len() as i32);
                buffer.put_slice(data.as_slice());
            }
            IpcMessage::Ping(data) => {
                buffer.put_i32_le(3);
                let data = to_vec(data)?;
                buffer.put_i32_le(data.len() as i32);
                buffer.put_slice(data.as_slice());
            }
            IpcMessage::Pong(data) => {
                buffer.put_i32_le(4);
                let data = to_vec(data)?;
                buffer.put_i32_le(data.len() as i32);
                buffer.put_slice(data.as_slice());
            }
        }
        Ok(buffer)
    }

    async fn try_decode(stream: &mut UnixStream) -> Result<IpcMessage> {
        let mut info_buffer = BytesMut::with_capacity(8);
        stream.read_buf(&mut info_buffer).await?;
        let msg_type = info_buffer.get_i32_le();
        let data_len = info_buffer.get_i32_le();
        let mut data_buffer = BytesMut::with_capacity(data_len as usize);
        stream.read_buf(&mut data_buffer).await?;
        match msg_type {
            0 => {
                let data = from_slice(&data_buffer)?;
                Ok(IpcMessage::Handshake(data))
            }
            1 => {
                let data = from_slice(&data_buffer)?;
                Ok(IpcMessage::Frame(data))
            }
            2 => {
                if let Ok(data) = from_slice(&data_buffer) {
                    Ok(IpcMessage::Close(data))
                } else {
                    Ok(IpcMessage::Close(CloseMessage {
                        code: CloseCodes::Normal,
                        message: "".into(),
                    }))
                }
            }
            3 => {
                let data = from_slice(&data_buffer)?;
                Ok(IpcMessage::Ping(data))
            }
            4 => {
                let data = from_slice(&data_buffer)?;
                Ok(IpcMessage::Pong(data))
            }
            x => {
                debug!("Invalid IPC Data: ({}) {:?}", x, data_buffer);
                Err(anyhow::anyhow!("Invalid IPC Message Type"))
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CloseCodes {
    Normal = 1000,
    Unsupported = 1003,
    Abnormal = 1006,
    InvalidClientID = 4000,
    InvalidVersion = 4004,
}

#[derive(Debug, Clone)]
pub enum IpcCommand {
    Frame(IpcFrame),
    Close,
}

impl IpcCommand {
    pub fn try_encode(&self) -> Result<BytesMut> {
        match self {
            IpcCommand::Frame(data) => IpcMessage::Frame(data.clone()).try_encode(),
            IpcCommand::Close => Ok(IpcMessage::Close(CloseMessage {
                code: CloseCodes::Normal,
                message: "".into(),
            })
            .try_encode()?),
        }
    }
}

type IpcClientMap = Arc<Mutex<HashMap<usize, broadcast::Sender<IpcCommand>>>>;

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
                        debug!("IPC Event: {:?}", event);
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
