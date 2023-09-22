use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Duration, Utc};
use itertools::Itertools as _;

use aprs::{PositionReport, Report};

const AIRBORNE_VEL: f32 = 30.;
const GROUNDED_VEL: f32 = 10.;
const EVENT_MIN_DURATION: i64 = 10;

pub trait DateSource {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemDateTimeSource;

impl DateSource for SystemDateTimeSource {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub struct FixedDateTimeSource(pub DateTime<Utc>);

impl DateSource for FixedDateTimeSource {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

pub struct EventDetector {
    database: HashMap<String, AircraftStatus>,
    maintain_counter: u32,
    datetime_source: Box<dyn DateSource>,
}

impl EventDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_date_source(datetime_source: Box<dyn DateSource>) -> Self {
        Self {
            datetime_source,
            ..Default::default()
        }
    }

    pub fn add_report(&mut self, report: &Report) -> Option<Event> {
        if let Report::PositionReport(pr) = report {
            let report = self.add_position_report(pr);
            self.maintain();
            return report;
        }
        None
    }

    fn add_position_report(&mut self, report: &PositionReport) -> Option<Event> {
        if let Some(id) = report.id() {
            let status: &mut AircraftStatus = self.database.entry(id.clone()).or_default();
            if let Some(stamped_report) = Beacon::new(report.clone(), self.datetime_source.now()) {
                if status
                    .reports
                    .back()
                    .map(|r| r.timestamp < stamped_report.timestamp)
                    .unwrap_or(true)
                {
                    log::debug!(
                        "Got report of type {:?} for {id} at {}",
                        Self::report_state(&stamped_report),
                        stamped_report.timestamp
                    );
                    status.reports.push_back(stamped_report);
                }
                if let Some((date, state)) = Self::detect_event(status) {
                    if status.state != Some(state) {
                        status.state = Some(state);
                        log::info!("{date} Got event {state:?} at for {id}");
                        return Some(Event::AircraftChangedState(AircraftChangeStatedEvent {
                            aircraft_id: id.clone(),
                            new_state: state,
                            date,
                        }));
                    }
                }
            }
        }
        None
    }

    fn detect_event(status: &AircraftStatus) -> Option<(DateTime<Utc>, AircraftState)> {
        let groups = status.reports.iter().group_by(|b| Self::report_state(b));
        let grouped_states = groups
            .into_iter()
            .map(|(s, g)| (s, g.cloned().collect::<Vec<_>>()))
            .collect::<Vec<_>>();
        let ((state_before, beacons_before), (state_now, beacons_now)) =
            grouped_states.into_iter().tuple_windows().last()?;

        let state_now = state_now?;

        log::debug!("{state_now:?}");

        let first = beacons_now.first().unwrap();
        let last = beacons_now.last().unwrap();
        if (last.timestamp - first.timestamp) < Duration::seconds(EVENT_MIN_DURATION) {
            return None;
        }

        let first_timestamp = if state_before.is_none() {
            beacons_before.first().unwrap().timestamp
        } else {
            first.timestamp
        };

        Some((first_timestamp, state_now))
    }

    fn report_state(report: &Beacon) -> Option<AircraftState> {
        if report.speed >= AIRBORNE_VEL {
            Some(AircraftState::Airborne)
        } else if report.speed <= GROUNDED_VEL {
            Some(AircraftState::OnGround)
        } else {
            None
        }
    }

    /// Remove old reports
    fn maintain(&mut self) {
        self.maintain_counter += 1;
        if self.maintain_counter == 100 {
            // Remove the last reports for all aircrafts execpt the last 10.
            for (_, s) in self.database.iter_mut() {
                if s.reports.len() > 10 {
                    s.reports.drain(..(s.reports.len() - 10));
                }
            }

            self.maintain_counter = 0;
        }
    }
}

impl Default for EventDetector {
    fn default() -> Self {
        Self {
            database: HashMap::new(),
            maintain_counter: 0,
            datetime_source: Box::new(SystemDateTimeSource),
        }
    }
}

#[derive(Clone, Debug)]
struct Beacon {
    timestamp: DateTime<Utc>,
    speed: f32,
}

impl Beacon {
    pub fn new(report: PositionReport, now: DateTime<Utc>) -> Option<Self> {
        let timestamp = match report.timestamp.as_ref().map(|t| t.guess_datetime(now)) {
            Some(Ok(dt)) => dt,
            Some(Err(e)) => {
                log::warn!("Could not guess report timestamp: {e}");
                Utc::now()
            }
            _ => Utc::now(),
        };
        let speed = report.speed()?;
        Some(Self { timestamp, speed })
    }
}

#[derive(Clone, Debug)]
struct AircraftStatus {
    state: Option<AircraftState>,
    reports: VecDeque<Beacon>,
}

impl Default for AircraftStatus {
    fn default() -> Self {
        AircraftStatus {
            state: None,
            reports: Default::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    AircraftChangedState(AircraftChangeStatedEvent),
}

#[derive(Clone, Debug)]
pub struct AircraftChangeStatedEvent {
    pub aircraft_id: String,
    pub new_state: AircraftState,
    pub date: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AircraftState {
    Airborne,
    OnGround,
}
