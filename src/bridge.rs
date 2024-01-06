use crate::structs::IpcActivityMessage;
use anyhow::Result;
use futures_util::{lock::Mutex, SinkExt, StreamExt};
use owo_colors::OwoColorize;
use serde_json::to_string;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{
    net::{TcpListener, TcpStream},
    select,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task,
};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{debug, info};

pub enum BridgeCommand {
    Message(Box<IpcActivityMessage>),
    Close,
}

type ClientMap = Arc<Mutex<HashMap<SocketAddr, UnboundedSender<BridgeCommand>>>>;
type ActivityMap = Arc<Mutex<HashMap<String, IpcActivityMessage>>>;

#[derive(Debug)]
pub struct BridgeServer {
    client_map: ClientMap,
    activity_map: ActivityMap,
}

impl BridgeServer {
    pub async fn try_bind() -> Result<BridgeServer> {
        let client_map = ClientMap::new(Mutex::new(HashMap::new()));
        let activity_map = ActivityMap::new(Mutex::new(HashMap::new()));
        let listener = TcpListener::bind("127.0.0.1:1337").await?;
        info!(
            "{} {}",
            "Bridge Started on port".cyan(),
            "1337".yellow().bold()
        );
        task::spawn(Self::accept_loop(
            listener,
            client_map.clone(),
            activity_map.clone(),
        ));
        Ok(Self {
            client_map,
            activity_map,
        })
    }

    async fn accept_loop(
        listener: TcpListener,
        client_map: ClientMap,
        activity_map: ActivityMap,
    ) -> Result<()> {
        loop {
            let (stream, addr) = listener.accept().await?;
            let (tx, rx) = mpsc::unbounded_channel();
            client_map.lock().await.insert(addr, tx);

            info!("{}", "New Web Client connected!".green());
            task::spawn(Self::handle_stream(
                stream,
                addr,
                rx,
                client_map.clone(),
                activity_map.clone(),
            ));
        }
    }

    async fn handle_stream(
        stream: TcpStream,
        addr: SocketAddr,
        mut rx: UnboundedReceiver<BridgeCommand>,
        client_map: ClientMap,
        activity_map: ActivityMap,
    ) -> Result<()> {
        let ws_stream = accept_async(stream).await?;
        let (mut write, mut read) = ws_stream.split();

        // Catch up on activity
        for (_, msg) in activity_map.lock().await.iter() {
            if msg.activity.is_some() {
                let msg = to_string(&msg)?;
                write.send(Message::Text(msg)).await?;
            }
        }

        loop {
            select! {
                msg = rx.recv() => {
                    if let Some(msg) = msg {
                        match msg {
                            BridgeCommand::Message(msg) => {
                                let msg = to_string(&msg)?;
                                write.send(Message::Text(msg)).await?;
                            }
                            BridgeCommand::Close => {
                                for (_, msg) in activity_map.lock().await.iter_mut() {
                                    msg.activity = None;
                                    let msg = to_string(&msg)?;
                                    write.send(Message::Text(msg)).await?;
                                }
                                return Ok(())
                            },
                        }
                    } else {
                        break;
                    }
                }
                evt = read.next() => {
                    match evt {
                        None => break,
                        Some(msg) => {
                            match msg {
                                Ok(msg) => {
                                    match msg {
                                        Message::Close(_) => break,
                                        e => {
                                            debug!("{}", e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    debug!("{}",e);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        client_map.lock().await.remove(&addr);
        info!("{}", "Web Client Disconnected!".red());
        Ok(())
    }

    pub async fn send_activity(&self, msg: IpcActivityMessage) -> Result<()> {
        self.activity_map
            .lock()
            .await
            .insert(msg.socket_id.clone(), msg.clone());
        self.client_map
            .lock()
            .await
            .iter()
            .try_for_each(|(_, tx)| tx.send(BridgeCommand::Message(Box::new(msg.clone()))))?;
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        info!("{}", "Shutting Down Bridge".magenta());
        self.client_map
            .lock()
            .await
            .iter()
            .try_for_each(|(_, tx)| tx.send(BridgeCommand::Close))?;
        Ok(())
    }
}
