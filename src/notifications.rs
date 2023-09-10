use anyhow::Result;
use fcm::FcmResponse;
use serde::Serialize;

use crate::events::{AircraftState, Event};

pub struct FirebaseNotificationSender {
    client: fcm::Client,
    api_key: String,
    topic_id: String,
}

impl FirebaseNotificationSender {
    pub fn new<S: Into<String>>(api_key: S, topic_id: S) -> Self {
        Self {
            client: fcm::Client::new(),
            api_key: api_key.into(),
            topic_id: topic_id.into(),
        }
    }

    pub async fn notify_event(&self, event: &Event) -> Result<FcmResponse> {
        /*let notification = match event {
                Event::AircraftChangedState(e) => {
                    let mut notification_builder = fcm::NotificationBuilder::new();
                    notification_builder
                        .title(&format!("{} {:?}", e.aircraft_id, e.new_state))
                        .body(&format!("{} {:?} at {}", e.aircraft_id, e.new_state, e.date));
                    notification_builder.finalize()
                }
        };*/
        log::debug!("Sending notification for event {:?}", event);
        let to = format!("/topics/{}", self.topic_id);
        let mut message_builder = fcm::MessageBuilder::new(&self.api_key, &to);
        message_builder
            //.notification(notification)
            .data(&Self::prepare_message_data(event))?
            .priority(fcm::Priority::High)
            .delay_while_idle(false)
            .time_to_live(120);
        let message = message_builder.finalize();
        self.client.send(message).await.map_err(Into::into)
    }

    fn prepare_message_data(event: &Event) -> EventData {
        match event {
            Event::AircraftChangedState(e) => {
                let data = AircraftChangedStateData {
                    aircraft_id: e.aircraft_id.clone(),
                    aircraft_immatriculation: String::new(),
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
