use anyhow::Result;
use fcm::FcmResponse;
use serde::Serialize;
use std::collections::HashMap;

use crate::events::{AircraftState, Event};
use crate::ogn::ddb::Device;

pub struct FirebaseNotificationSender {
    client: fcm::Client,
    api_key: String,
    topic_id: String,
    ddb: HashMap<String, Device>,
}

impl FirebaseNotificationSender {
    pub fn new<S: Into<String>>(api_key: S, topic_id: S) -> Self {
        Self {
            client: fcm::Client::new(),
            api_key: api_key.into(),
            topic_id: topic_id.into(),
            ddb: HashMap::default(),
        }
    }

    pub fn set_ddb(&mut self, ddb: HashMap<String, Device>) {
        self.ddb = ddb;
    }

    pub async fn notify_event(&self, event: &Event) -> Result<FcmResponse> {
        log::debug!("Sending notification for event {:?}", event);
        let to = format!("/topics/{}", self.topic_id);
        let mut message_builder = fcm::MessageBuilder::new(&self.api_key, &to);
        message_builder
            .data(&self.prepare_message_data(event))?
            .priority(fcm::Priority::High)
            .delay_while_idle(false)
            .time_to_live(0);
        let message = message_builder.finalize();
        self.client.send(message).await.map_err(Into::into)
    }

    fn prepare_message_data(&self, event: &Event) -> EventData {
        match event {
            Event::AircraftChangedState(e) => {
                let aircraft_immatriculation = self
                    .ddb
                    .get(&e.aircraft_id)
                    .map(|d| d.registration.as_str())
                    .unwrap_or("")
                    .to_string();
                let data = AircraftChangedStateData {
                    aircraft_id: e.aircraft_id.clone(),
                    aircraft_immatriculation,
                    date: e.date.timestamp(),
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
    date: i64,
}
