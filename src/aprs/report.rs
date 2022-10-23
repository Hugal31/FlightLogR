use std::{
    fmt::{self, Display, Formatter, Write},
    str::FromStr,
    time::Duration,
};

use anyhow::{format_err, Result};
use chrono::{Date, DateTime, Datelike, NaiveTime, TimeZone, Timelike, Utc};
use dms_coordinates::DMS;

pub mod parsing;

pub type Point = geo::Point<f64>;
pub type Symbol = [u8; 2];

#[derive(Clone, Debug)]
pub enum Report {
    PositionReport(PositionReport),
    StatusReport(StatusReport),
}

impl Report {
    pub fn timestamp(&self) -> Option<APRSTimestamp> {
        match self {
            Self::PositionReport(report) => report.timestamp.clone(),
            Self::StatusReport(report) => Some(report.timestamp.clone()),
        }
    }

    pub fn datetime(&self) -> Option<Result<DateTime<Utc>>> {
        self.datetime_with_now(Utc::now())
    }

    pub fn datetime_with_now(&self, now: DateTime<Utc>) -> Option<Result<DateTime<Utc>>> {
        self.timestamp().map(|t| t.guess_datetime(now))
    }
}

impl Display for Report {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::PositionReport(report) => Display::fmt(report, f),
            Self::StatusReport(report) => Display::fmt(report, f),
        }
    }
}

impl FromStr for Report {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parsing::APRSParser.parse(s)
    }
}

#[derive(Clone, Debug)]
pub struct PositionReport {
    pub timestamp: Option<APRSTimestamp>,
    pub symbol: Symbol,
    pub position: PositionReportCoordinates,
    pub data_extension: Option<PositionReportDataExtension>,
    pub comments: Vec<Comment>,
}

impl PositionReport {
    // Maybe store the enhanced value?
    pub fn point(&self) -> Point {
        if let Some((lat, lon)) = self.position_precision_enhancement() {
            let original_lat = DMS::from_decimal_degrees(self.position.point.y(), true);
            let original_lon = DMS::from_decimal_degrees(self.position.point.x(), false);
            let enhanced_lat = DMS::new(
                original_lat.degrees,
                original_lat.minutes,
                original_lat.seconds + (lat as f64 / 10.),
                original_lat.bearing,
            );
            let enhanced_lon = DMS::new(
                original_lon.degrees,
                original_lon.minutes,
                original_lon.seconds + (lon as f64 / 10.),
                original_lon.bearing,
            );

            geo::Point(
                (
                    enhanced_lon.to_decimal_degrees(),
                    enhanced_lat.to_decimal_degrees(),
                )
                    .into(),
            )
        } else {
            self.position.point
        }
    }

    pub fn course(&self) -> Option<u32> {
        self.data_extension.as_ref().and_then(|de| de.course())
    }

    pub fn speed(&self) -> Option<f32> {
        self.data_extension.as_ref().and_then(|de| de.speed())
    }

    pub fn altitude(&self) -> Option<f64> {
        self.comments
            .iter()
            .filter_map(|c| match c {
                &Comment::Altitude(f) => Some(f),
                _ => None,
            })
            .next()
    }

    pub fn position_precision_enhancement(&self) -> Option<(u8, u8)> {
        self.comments
            .iter()
            .filter_map(|c| match c {
                &Comment::PositionPrecisionEnhancement { lat, lon } => Some((lat, lon)),
                _ => None,
            })
            .next()
    }
}

impl Display for PositionReport {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(timestamp) = &self.timestamp {
            Display::fmt(timestamp, f)?;
        }

        match &self.data_extension {
            Some(PositionReportDataExtension::CompressedData { .. }) => {
                f.write_char(self.symbol[0] as _)?;
                self.position.format_compressed(f)?;
                f.write_char(self.symbol[1] as _)?;
            }
            _ => {
                PositionReportCoordinates::format_lat(self.position.point.y(), f)?;
                f.write_char(self.symbol[0] as _)?;
                PositionReportCoordinates::format_lon(self.position.point.x(), f)?;
                f.write_char(self.symbol[1] as _)?;
            }
        }
        if let Some(de) = &self.data_extension {
            Display::fmt(de, f)?;
        }

        let mut first = false;
        for comment in &self.comments {
            if first {
                first = false;
            } else {
                f.write_char(' ')?;
            }
            Display::fmt(comment, f)?;
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum APRSTimestamp {
    HMS(NaiveTime),
    DHM(DHM),
}

impl APRSTimestamp {
    pub fn precision(&self) -> Duration {
        match self {
            Self::HMS(_) => Duration::from_secs(1),
            Self::DHM(_) => Duration::from_secs(60),
        }
    }

    pub fn guess_datetime(&self, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
        match *self {
            Self::HMS(naive_time) => guess_date(naive_time, now),
            Self::DHM(DHM { day, time, utc }) => {
                if utc {
                    let today = now.date();
                    let date = today.clone();
                    match date.with_day(day) {
                        Some(d) if d <= today => d
                            .and_time(time)
                            .ok_or_else(|| format_err!("could not compose time")),
                        // Try last month
                        _ => last_month(date)
                            .and_then(|d| d.with_day(day))
                            .and_then(|d| d.and_time(time))
                            .ok_or_else(|| format_err!("could not guess date")),
                    }
                } else {
                    // TODO Implement timezone guess
                    Err(format_err!(
                        "could not guess the datetime from a local time"
                    ))
                }
            }
        }
    }
}

fn last_month<Tz: TimeZone>(date: Date<Tz>) -> Option<Date<Tz>> {
    if date.month0() == 0 {
        date.with_year(date.year() - 1)
            .and_then(|d| d.with_month0(11))
    } else {
        date.with_month(date.month() - 1)
    }
}

impl Display for APRSTimestamp {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::HMS(time) => write!(
                f,
                "{:02}{:02}{:02}h",
                time.hour(),
                time.minute(),
                time.second()
            ),
            Self::DHM(dhm) => <DHM as Display>::fmt(dhm, f),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DHM {
    pub day: u32,
    pub time: NaiveTime,
    pub utc: bool,
}

impl DHM {
    pub fn new(day: u32, hour: u32, minute: u32, utc: bool) -> Self {
        Self {
            day,
            time: NaiveTime::from_hms(hour, minute, 0),
            utc,
        }
    }
}

impl Display for DHM {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let indicator = match self.utc {
            true => 'z',
            false => '/',
        };

        write!(
            f,
            "{:02}{:02}{:02}{}",
            self.day,
            self.time.hour(),
            self.time.minute(),
            indicator
        )
    }
}

#[derive(Clone, Debug)]
pub struct PositionReportCoordinates {
    pub point: Point,
    /// Number of given digits
    pub lat_digits: u8,
    pub lon_digits: u8,
}

impl PositionReportCoordinates {
    pub fn from_point(point: Point) -> Self {
        Self {
            point,
            lat_digits: 6,
            lon_digits: 7,
        }
    }

    fn format_lat(latitude: f64, f: &mut Formatter<'_>) -> fmt::Result {
        let lat = DMS::from_decimal_degrees(latitude, true);
        write!(
            f,
            "{:02}{:02}.{:02.0}{}",
            lat.degrees, lat.minutes, lat.seconds, lat.bearing
        )
    }

    fn format_lon(longitude: f64, f: &mut Formatter<'_>) -> fmt::Result {
        let lon = DMS::from_decimal_degrees(longitude, false);
        write!(
            f,
            "{:03}{:02}.{:02.0}{}",
            lon.degrees, lon.minutes, lon.seconds, lon.bearing
        )
    }

    fn format_compressed(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let lat = 380926. * (90. - self.point.y());
        let lon = 190463. * (180. + self.point.x());
        write!(f, "{:!>4}{:!>4}", Base91(lat as _), Base91(lon as _))
    }
}

impl Display for PositionReportCoordinates {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Self::format_lat(self.point.y(), f)?;
        Self::format_lon(self.point.x(), f)
    }
}

#[derive(Copy, Clone, Debug, Default)]
struct Base91(u32);

impl Display for Base91 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut buff = String::with_capacity(5);
        let mut n = self.0;
        while n > 91 {
            let d = n % 91;
            buff.push((d as u8 + b'!') as _);
            n /= 91;
        }
        buff.push((n as u8 + b'!') as _);

        let reversed: String = buff.chars().rev().collect();
        f.pad_integral(true, "!", &reversed)
    }
}

#[derive(Clone, Debug)]
pub enum PositionReportDataExtension {
    CourseSpeed {
        /// If 0, invalid.
        course: u32,
        speed: u32,
    },
    PHG,
    RadioRange,
    DFSSignalStrength,
    CompressedData {
        cs: [u8; 2],
        indicator: u8,
    },
}

impl PositionReportDataExtension {
    pub fn compressed_from_chars(chars: [u8; 3]) -> Self {
        Self::CompressedData {
            cs: [chars[0], chars[1]],
            indicator: chars[2],
        }
    }

    pub fn course(&self) -> Option<u32> {
        match self {
            Self::CourseSpeed { course, .. } if *course != 0 => Some(*course),
            Self::CompressedData { cs, .. } if (b'!'..=b'z').contains(&cs[0]) => {
                Some(((cs[0] - b'!') * 4) as _)
            }
            _ => None,
        }
    }

    pub fn speed(&self) -> Option<f32> {
        match self {
            Self::CourseSpeed { speed, .. } => Some(*speed as _),
            Self::CompressedData { cs, .. } if (b'!'..=b'z').contains(&cs[0]) => {
                Some(1.08f32.powi((cs[1] - b'!') as _) - 1.)
            }
            _ => None,
        }
    }
}

impl Display for PositionReportDataExtension {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::CourseSpeed { course, speed } => write!(f, "{:03}/{:03}", course, speed),
            Self::CompressedData { cs, indicator } => write!(
                f,
                "{}{}{}",
                cs[0] as char, cs[1] as char, *indicator as char
            ),
            _ => todo!(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Comment {
    /// Altitude AMSL in feet
    Altitude(f64),
    /// Third decimal digit to add to the seconds of lat and lon data.
    PositionPrecisionEnhancement {
        lat: u8,
        lon: u8,
    },
    Id(String),
    /// Flight level, i.e. altitude at 1013.25 hpa in hundreds of feet.
    FlightLevel(f64),
    /// Vertical speed in feet/m,
    ClimbRate(f64),
    /// Turning rate in degrees/min, positive is clockwise
    TurningRate(f64),
    Unknown(String),
}

impl Display for Comment {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Altitude(a) => write!(f, "/A={:06.0}", a),
            Self::PositionPrecisionEnhancement { lat, lon } => write!(f, "!W{}{}!", lat, lon),
            Self::Id(s) => write!(f, "id{}", s),
            Self::FlightLevel(fl) => write!(f, "FL{:03.2}", fl),
            Self::ClimbRate(cr) => write!(f, "{:+.1}fpm", cr),
            Self::TurningRate(tr) => write!(f, "{:+.1}rot", tr),
            Self::Unknown(s) => f.write_str(s),
        }
    }
}

#[derive(Clone, Debug)]
pub struct StatusReport {
    pub timestamp: APRSTimestamp,
    pub text: String,
}

impl Display for StatusReport {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.timestamp, self.text)
    }
}

/// Find the closest datetime between today, yesterday and tomorrow
fn guess_date(time: NaiveTime, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let today = now.date();
    let yesterday = today.clone() - chrono::Duration::days(1);
    let tomorrow = today.clone() + chrono::Duration::days(1);

    let datetime_today = today
        .and_time(time)
        .ok_or_else(|| format_err!("could not guess datetime {:?} with {}", today, time))?;
    let datetime_tomorrow = tomorrow
        .and_time(time)
        .ok_or_else(|| format_err!("could not guess datetime {:?} with {}", tomorrow, time))?;
    let datetime_yesterday = yesterday
        .and_time(time)
        .ok_or_else(|| format_err!("could not guess datetime {:?} with {}", yesterday, time))?;

    let leeway = chrono::Duration::minutes(30);

    let time_to_tomorrow = datetime_tomorrow.clone() - now.clone();
    let time_from_now = datetime_today.clone() - now;
    if time_to_tomorrow < leeway {
        // If datetime_tomorrow is in 30 minutes, accept it
        Ok(datetime_tomorrow)
    } else if time_from_now < leeway {
        // If datetime_today is in the past OR in less than thirty minutes, this is it
        Ok(datetime_today)
    } else {
        Ok(datetime_yesterday)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::NaiveDate;
    use dms_coordinates::Bearing;
    use float_eq::assert_float_eq;
    use geo::Point;

    #[test]
    fn test_fmt_coordinates() {
        assert_eq!(
            PositionReportCoordinates::from_point(Point((123.765556, -12.582222).into()))
                .to_string(),
            "1234.56S12345.56E"
        );
    }

    #[test]
    fn test_fmt_comment() {
        assert_eq!(Comment::Altitude(1020.0).to_string(), "/A=001020");
        assert_eq!(
            Comment::PositionPrecisionEnhancement { lat: 5, lon: 3 }.to_string(),
            "!W53!"
        );
        assert_eq!(
            Comment::Id("02DF0A52".to_string()).to_string(),
            "id02DF0A52"
        );
        assert_eq!(Comment::FlightLevel(320.0).to_string(), "FL320.00");
        assert_eq!(Comment::ClimbRate(42.0).to_string(), "+42.0fpm");
    }

    #[test]
    fn test_compressed_cst() {
        assert_eq!(
            PositionReportDataExtension::CourseSpeed {
                course: 360,
                speed: 0
            }
            .course(),
            Some(360)
        );
        assert_eq!(
            PositionReportDataExtension::CourseSpeed {
                course: 360,
                speed: 0
            }
            .speed(),
            Some(0.)
        );
        assert_eq!(
            PositionReportDataExtension::CourseSpeed {
                course: 0,
                speed: 0
            }
            .course(),
            None
        );

        let example_compressed_cs =
            PositionReportDataExtension::compressed_from_chars([b'7', b'P', b'!']);
        assert_eq!(example_compressed_cs.course(), Some(88));
        assert_eq!(
            example_compressed_cs.speed().map(|f| (f * 10.) as u32),
            Some(362)
        );
    }

    #[test]
    fn test_position_report_fmt() {
        assert_eq!(
            PositionReport {
                timestamp: None,
                symbol: [b'/', b'^'],
                position: PositionReportCoordinates::from_point(Point(
                    (123.765556, -12.582222).into()
                )),
                data_extension: Some(PositionReportDataExtension::CourseSpeed {
                    course: 120,
                    speed: 100
                }),
                comments: vec![],
            }
            .to_string(),
            "1234.56S/12345.56E^120/100"
        );
        assert_eq!(
            PositionReport {
                timestamp: Some(APRSTimestamp::HMS(NaiveTime::from_hms(3, 4, 56))),
                symbol: [b'/', b'g'],
                position: PositionReportCoordinates::from_point(Point((-72.75, 49.5).into())),
                data_extension: Some(PositionReportDataExtension::CompressedData {
                    cs: [b'7', b'P'],
                    indicator: b'['
                }),
                comments: vec![],
            }
            .to_string(),
            "030456h/5L!!<*e7g7P["
        );
    }

    #[test]
    fn test_format_base91() {
        assert_eq!(Base91(0).to_string(), "!");
        assert_eq!(Base91(1).to_string(), "\"");
        assert_eq!(Base91(20427156).to_string(), "<*e7");
        assert_eq!(format!("{:!>4}", Base91(2)), "!!!#");
    }

    #[test]
    fn test_enhanced_coordinates() {
        let original_lat = DMS::new(32, 12, 15.0, Bearing::South);
        let expected_lat = DMS::new(32, 12, 15.5, Bearing::South);
        let original_lon = DMS::new(140, 21, 3.0, Bearing::East);
        let expected_lon = DMS::new(140, 21, 3.2, Bearing::East);
        let report = PositionReport {
            timestamp: None,
            symbol: [b'/', b'g'],
            position: PositionReportCoordinates::from_point(Point(
                (
                    original_lon.to_decimal_degrees(),
                    original_lat.to_decimal_degrees(),
                )
                    .into(),
            )),
            data_extension: None,
            comments: vec![Comment::PositionPrecisionEnhancement { lat: 5, lon: 2 }],
        };

        let point = report.point();
        assert_float_eq!(point.x(), expected_lon.to_decimal_degrees(), abs <= 1.0e-5);
        assert_float_eq!(point.y(), expected_lat.to_decimal_degrees(), abs <= 1.0e-5);
    }

    #[test]
    fn test_guess_datetime() {
        let today = NaiveDate::from_ymd(2022, 10, 16);
        let morning = DateTime::<Utc>::from_utc(today.and_time(NaiveTime::from_hms(2, 0, 0)), Utc);
        let night = DateTime::<Utc>::from_utc(today.and_time(NaiveTime::from_hms(23, 45, 0)), Utc);

        assert_eq!(
            guess_date(NaiveTime::from_hms(1, 58, 0), morning)
                .expect("should guess")
                .date_naive(),
            NaiveDate::from_ymd(2022, 10, 16)
        );
        assert_eq!(
            guess_date(NaiveTime::from_hms(23, 0, 0), morning)
                .expect("should guess")
                .date_naive(),
            NaiveDate::from_ymd(2022, 10, 15)
        );
        assert_eq!(
            guess_date(NaiveTime::from_hms(23, 50, 0), night)
                .expect("should guess")
                .date_naive(),
            NaiveDate::from_ymd(2022, 10, 16)
        );
        assert_eq!(
            guess_date(NaiveTime::from_hms(0, 5, 0), night)
                .expect("should guess")
                .date_naive(),
            NaiveDate::from_ymd(2022, 10, 17)
        );
        assert_eq!(
            guess_date(NaiveTime::from_hms(1, 0, 0), night)
                .expect("should guess")
                .date_naive(),
            NaiveDate::from_ymd(2022, 10, 16)
        );

        assert_eq!(
            APRSTimestamp::HMS(NaiveTime::from_hms(1, 58, 30))
                .guess_datetime(morning)
                .expect("should guess date"),
            DateTime::parse_from_rfc3339("2022-10-16T01:58:30Z").unwrap()
        );
        assert_eq!(
            APRSTimestamp::DHM(DHM {
                day: 16,
                time: NaiveTime::from_hms(1, 57, 0),
                utc: true
            })
            .guess_datetime(morning)
            .expect("should guess date"),
            DateTime::parse_from_rfc3339("2022-10-16T01:57:00Z").unwrap()
        );
        assert_eq!(
            APRSTimestamp::DHM(DHM {
                day: 15,
                time: NaiveTime::from_hms(1, 57, 0),
                utc: true
            })
            .guess_datetime(morning)
            .expect("should guess date"),
            DateTime::parse_from_rfc3339("2022-10-15T01:57:00Z").unwrap()
        );
        assert_eq!(
            APRSTimestamp::DHM(DHM {
                day: 31,
                time: NaiveTime::from_hms(23, 56, 0),
                utc: true
            })
            .guess_datetime("2022-02-01T01:00:00Z".parse().unwrap())
            .expect("should guess date"),
            DateTime::parse_from_rfc3339("2022-01-31T23:56:00Z").unwrap()
        );
    }
}
