use anyhow::Result;
use fcm_notification::{FcmNotification, NotificationPayload};
use serde::Serialize;
use std::collections::HashMap;

use crate::events::{AircraftState, Event};
use ogn::ddb::Device;

pub struct FirebaseNotificationSender {
    topic_id: String,
    ddb: HashMap<u32, Device>,
    fcm_client: FcmNotification,
}

impl FirebaseNotificationSender {
    pub fn new<S: Into<String>>(token_path: &str, topic_id: S) -> Result<Self> {
        Ok(Self {
            topic_id: topic_id.into(),
            ddb: HashMap::default(),
            fcm_client: FcmNotification::new(token_path)?,
        })
    }

    pub fn set_ddb(&mut self, ddb: HashMap<u32, Device>) {
        self.ddb = ddb;
    }

    pub async fn notify_event(&self, event: &Event) -> Result<()> {
        log::debug!("Sending notification for event {:?}", event);
        let data = self.prepare_message_data(event);
        // TODO Have priority
        let notification = NotificationPayload {
            topic: Some(&self.topic_id),
            data: Some(serde_json::to_value(data)?),
            ..Default::default()
        };
        self.fcm_client
            .send_notification(&notification)
            .await
            .map_err(Into::into)
    }

    fn prepare_message_data(&self, event: &Event) -> EventData {
        match event {
            Event::AircraftChangedState(e) => {
                let aircraft_immatriculation = self
                    .ddb
                    .get(&e.aircraft_id)
                    .or_else(|| self.ddb.get(&e.aircraft_id))
                    .map(|d| d.registration.as_str())
                    .unwrap_or("")
                    .to_string();
                let data = AircraftChangedStateData {
                    aircraft_id: format!("{:X}", e.aircraft_id),
                    aircraft_immatriculation,
                    date: e.date.timestamp().to_string(),
                };
                match e.new_state {
                    AircraftState::Airborne => EventData::AircraftTookOff(data),
                    AircraftState::OnGround => EventData::AircraftLanded(data),
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
enum EventData {
    #[serde(rename = "aircraft_took_off")]
    AircraftTookOff(AircraftChangedStateData),
    #[serde(rename = "aircraft_landed")]
    AircraftLanded(AircraftChangedStateData),
}

#[derive(Clone, Debug, Serialize)]
struct AircraftChangedStateData {
    aircraft_id: String,
    aircraft_immatriculation: String,
    date: String,
}
