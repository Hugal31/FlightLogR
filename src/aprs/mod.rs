use std::str::FromStr;

use chrono::{DateTime, Utc};
use quantities::{
    duration::{HOUR, MINUTE},
    length::{MILE, FOOT, Length},
    speed::{Speed},
};

type Point = geo::Point<f64>;

mod parsing;

pub fn nautical_mile() -> Length {
    1.151 * MILE
}

pub fn knots() -> Speed {
    nautical_mile() / (1.0 * HOUR)
}

#[derive(Debug, Default)]
pub struct Report {
    pub time: DateTime<Utc>,
    /// Geo location of the record
    pub coordinates: Point,
    pub symbol: [char; 2],
    /// Ground track in degrees
    pub track: f64,
    /// Ground speed in knots
    pub speed: f64,
    /// Altitude AMSL in feet
    pub altitude: f64,
    pub aircraft_id: Option<u32>,
    /// Flight level, i.e. altitude at 1013.25 hpa in hundreds of feet.
    pub flight_level: Option<f64>,
    /// Vertical speed in feet/m,
    pub climb_rate: Option<f64>,
    /// Turning rate in degrees/min
    pub turning_rate: Option<f64>,
}

#[cfg(feature="quantities")]
impl Report {
    pub fn get_altitude(&self) -> Length {
        self.altitude * FOOT
    }

    pub fn get_speed(&self) -> Speed {
        self.speed * knots()
    }

    pub fn get_flight_level(&self) -> Option<Length> {
        self.flight_level.map(|fl| fl * 100.0 * FOOT)
    }

    pub fn get_climb_rate(&self) -> Option<Speed> {
        self.climb_rate.map(|cr| cr * FOOT / (1. * MINUTE))
    }
}

impl FromStr for Report {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parsing::APRSParser::default().parse(s)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[cfg(feature = "quantities")]
    #[test]
    fn test_units() {
        let report = Report {
            altitude: 1000.0,
            speed: 42.0,
            climb_rate: Some(320.0),
            flight_level: Some(330.0),
            .. Default::default()
        };

        assert_eq!(report.get_altitude(), 1000.0 * FOOT);
        assert_eq!(report.get_speed(), 42.0 * nautical_mile() / (1.0 * HOUR));
        assert_eq!(report.get_flight_level(), Some(33000.0 * FOOT));
        assert_eq!(report.get_climb_rate(), Some(320.0 * FOOT / (1.0 * MINUTE)));
    }
}
