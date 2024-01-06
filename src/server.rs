use crate::{
    ipc::{
        server::IpcServer,
        structs::{IpcCommand, IpcFrame, IpcMessage},
    },
    structs::{IpcActivityMessage, IpcPartialActivityMessage},
};
use anyhow::Result;
use serde_json::json;
use tokio::{sync::mpsc, task};

pub struct Server;

impl Server {
    pub async fn try_bind() -> Result<mpsc::Receiver<IpcActivityMessage>> {
        let mut ipc = IpcServer::try_bind().await?;
        let (tx, rx) = mpsc::channel(1);
        task::spawn(async move {
            let mut client_id = None;
            loop {
                if let Some((socket_id, msg)) = ipc.recv().await {
                    match msg {
                        IpcMessage::Frame(frame) => {
                            if let Some(args) = frame.args {
                                tx.send(IpcPartialActivityMessage::to_full_message(
                                    Some(args.activity),
                                    args.pid,
                                    socket_id.to_string(),
                                    &client_id,
                                ))
                                .await
                                .unwrap();
                            };
                        }

                        IpcMessage::Handshake(data) => {
                            client_id.replace(data.client_id);
                            ipc.send(
                                socket_id,
                                IpcCommand::Frame(IpcFrame {
                                    cmd: "DISPATCH".to_string(),
                                    evt: Some("READY".to_string()),
                                    args: None,
                                    data: Some(json!({
                                      "v": 1,
                                      "user": {
                                        "id": "1045800378228281345",
                                        "username": "arRPC",
                                        "discriminator": "0000",
                                        "avatar": "cfefa4d9839fb4bdf030f91c2a13e95c",
                                        "flags": 0,
                                        "premium_type": 0,
                                      },
                                      "config": {
                                        "api_endpoint": "//discord.com/api",
                                        "cdn_host": "cdn.discordapp.com",
                                        "environment": "production"
                                      }
                                    })),
                                    nonce: "".to_string(),
                                }),
                            )
                            .await
                            .unwrap();
                        }
                        _ => {}
                    }
                }
            }
        });
        Ok(rx)
    }
}
