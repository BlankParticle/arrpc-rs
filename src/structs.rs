use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub struct ActivityMessage {
    pub activity: Option<Value>,
    #[serde(rename = "socketId")]
    pub socket_id: String,
    pub pid: u32,
}

impl ActivityMessage {
    pub fn new(
        mut activity: Option<Value>,
        socket_id: String,
        pid: u32,
        client_id: &Option<String>,
    ) -> Self {
        if let Some(activity) = &mut activity {
            if let Some(client_id) = client_id {
                let activity = activity.as_object_mut().unwrap();
                activity.insert("application_id".to_string(), client_id.clone().into());
                activity.insert(
                    "flags".to_string(),
                    Value::Number(
                        if activity
                            .get_key_value("instance")
                            .unwrap()
                            .1
                            .as_bool()
                            .unwrap()
                        {
                            1.into()
                        } else {
                            0.into()
                        },
                    ),
                );
                activity.insert("type".to_string(), 0.into());
                if let Some(buttons) = &mut activity.get("buttons") {
                    let buttons = buttons.as_array().unwrap();
                    let button_urls = buttons.iter().map(|b| b["url"].clone()).collect();
                    let button_labels = buttons.iter().map(|b| b["label"].clone()).collect();
                    activity.insert(
                        "metadata".into(),
                        json!({ "button_urls": Value::Array(button_urls) }),
                    );
                    activity.insert("buttons".into(), Value::Array(button_labels));
                }
            }
        };

        Self {
            activity,
            socket_id,
            pid,
        }
    }
}
