use anyhow::Result;
use bytes::{Buf, BufMut, BytesMut};
use serde::{Deserialize, Serialize};
use serde_json::{from_slice, to_vec, Value};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    io::AsyncReadExt,
    net::UnixStream,
    sync::{broadcast, Mutex},
};
use tracing::debug;

use crate::structs::IpcPartialActivity;

pub type IpcClientMap = Arc<Mutex<HashMap<usize, broadcast::Sender<IpcCommand>>>>;

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

    pub async fn try_decode(stream: &mut UnixStream) -> Result<IpcMessage> {
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

#[derive(Debug, Serialize, Deserialize)]
pub struct HandshakeMessage {
    #[serde(rename = "v")]
    pub version: i32,
    pub client_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CloseMessage {
    pub code: CloseCodes,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcFrameArgs {
    pub activity: IpcPartialActivity,
    pub pid: usize,
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
