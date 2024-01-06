use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_text: Option<String>,
}

impl Assets {
    pub fn is_empty(&self) -> bool {
        self.large_image.is_none()
            && self.large_text.is_none()
            && self.small_image.is_none()
            && self.small_text.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Button {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timestamps {
    pub start: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcPartialActivity {
    pub state: String,
    pub details: String,
    #[serde(skip_serializing_if = "Assets::is_empty")]
    pub assets: Assets,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<Button>,
    pub instance: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamps: Option<Timestamps>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcPartialActivityMessage {
    pub activity: Option<IpcPartialActivity>,
    #[serde(rename = "socketId")]
    pub socket_id: String,
    pub pid: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcActivityMetadata {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub button_urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcActivity {
    pub application_id: String,
    pub state: String,
    pub details: String,
    pub flags: u64,
    pub r#type: u64,
    #[serde(skip_serializing_if = "Assets::is_empty")]
    pub assets: Assets,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<String>,
    pub metadata: IpcActivityMetadata,
    pub instance: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamps: Option<Timestamps>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcActivityMessage {
    pub activity: Option<IpcActivity>,
    pub socket_id: String,
    pub pid: usize,
}

impl IpcPartialActivityMessage {
    pub fn to_full_message(
        partial: Option<IpcPartialActivity>,
        pid: usize,
        socket_id: String,
        client_id: &Option<String>,
    ) -> IpcActivityMessage {
        IpcActivityMessage {
            activity: if let Some(activity) = partial {
                let activity = IpcActivity {
                    application_id: client_id.clone().unwrap_or_default(),
                    state: activity.state,
                    details: activity.details,
                    flags: activity.instance as u64,
                    r#type: 0,
                    assets: activity.assets,
                    buttons: activity
                        .buttons
                        .iter()
                        .map(|button| button.label.clone())
                        .collect(),
                    metadata: IpcActivityMetadata {
                        button_urls: activity
                            .buttons
                            .iter()
                            .map(|button| button.url.clone())
                            .collect(),
                    },
                    instance: activity.instance,
                    timestamps: activity.timestamps,
                };

                Some(activity)
            } else {
                None
            },
            socket_id,
            pid,
        }
    }
}
