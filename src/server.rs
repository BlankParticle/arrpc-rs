use crate::{
    ipc::{IpcCommand, IpcFrame, IpcMessage, IpcServer},
    structs::ActivityMessage,
};
use anyhow::Result;
use serde_json::json;
use tokio::{sync::mpsc, task};

pub struct Server;

impl Server {
    pub async fn try_bind() -> Result<mpsc::Receiver<ActivityMessage>> {
        let mut ipc = IpcServer::try_bind().await?;
        let (tx, rx) = mpsc::channel(1);
        task::spawn(async move {
            let mut client_id = None;
            loop {
                if let Some((id, msg)) = ipc.recv().await {
                    match msg {
                        IpcMessage::Frame(frame) => {
                            if let Some(args) = frame.args {
                                tx.send(ActivityMessage::new(
                                    Some(args.activity),
                                    id.to_string(),
                                    args.pid,
                                    &client_id,
                                ))
                                .await
                                .unwrap();
                            };
                        }

                        IpcMessage::Handshake(data) => {
                            client_id.replace(data.client_id);
                            ipc.send(
                                id,
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
