use anyhow::Result;
use chrono::{DateTime, NaiveTime, Utc};
use itertools::Itertools;
use pest::{
    iterators::Pair,
    Parser,
};
use pest_derive::Parser;
use super::Report;

#[derive(Parser)]
#[grammar="aprs/aprs.pest"]
pub struct APRSParser {
    now: DateTime<Utc>,
}

impl APRSParser {
    pub fn new(now: DateTime<Utc>) -> APRSParser {
        APRSParser {
            now,
        }
    }

    pub fn parse(&self, s: &str) -> Result<Report> {
        let aprs_ast = <Self as Parser<Rule>>::parse(Rule::aprs_record, s)?
            .next().unwrap();

        let mut rules = aprs_ast.into_inner();
        let mut report = self.parse_aprs_report(rules.next().unwrap())?;
        if let Some(comments) = rules.next() {
            Self::parse_comments(comments, &mut report);
        }

        Ok(report)
    }

    fn parse_aprs_report(&self, rule: Pair<Rule>) -> Result<Report> {
        let mut pairs = rule.into_inner();
        let _sender = pairs.next().unwrap().as_str();
        let _receiver = pairs.next().unwrap().as_str();
        let time = pairs.next().unwrap().as_str();
        let coords = pairs.next().unwrap();

        let time = self.parse_datetime(time)?;
        let (coordinates, symbol) = Self::parse_coords_and_symbol(coords)?;
        let track = pairs.next().unwrap().as_str().parse()?;
        let speed = pairs.next().unwrap().as_str().parse()?;
        let altitude = Self::parse_altitude(pairs.next().unwrap())?;

        Ok(Report {
            time,
            coordinates,
            symbol,
            track,
            speed,
            altitude,
            .. Default::default()
        })
    }

    fn parse_coords_and_symbol(pair: Pair<Rule>) -> Result<(geo::Point, [char; 2])> {
        use dms_coordinates::{Bearing, DMS};

        let mut pairs = pair.into_inner();
        let mut latitude = pairs.next().unwrap().into_inner();
        let symbol_1 = pairs.next().unwrap().as_str().chars().next().unwrap();
        let mut longitude = pairs.next().unwrap().into_inner();
        let symbol_2 = pairs.next().unwrap().as_str().chars().next().unwrap();

        let latitude = {
            let latitude_number = latitude.next().unwrap();
            let latitude_bearing = latitude.next().unwrap();
            let (degrees_str, minutes_seconds_str) = latitude_number.as_str().split_at(2);
            let (minutes_str, seconds_str) = minutes_seconds_str.splitn(2, ".").collect_tuple().unwrap();
            let bearing_str = latitude_bearing.as_str();
            DMS::new(degrees_str.parse()?,
                     minutes_str.parse()?,
                     seconds_str.parse()?,
                     if bearing_str == "N" { Bearing::North } else { Bearing::South }
            ).to_decimal_degrees()
        };
        let longitude = {
            let longitude_number = longitude.next().unwrap();
            let longitude_bearing = longitude.next().unwrap();
            let (degrees_str, minutes_seconds_str) = longitude_number.as_str().split_at(3);
            let (minutes_str, seconds_str) = minutes_seconds_str.splitn(2, ".").collect_tuple().unwrap();
            let bearing_str = longitude_bearing.as_str();
            DMS::new(degrees_str.parse()?,
                     minutes_str.parse()?,
                     seconds_str.parse()?,
                     if bearing_str == "E" { Bearing::East } else { Bearing::West }
            ).to_decimal_degrees()
        };

        Ok((geo::Point((longitude, latitude).into()), [symbol_1, symbol_2]))
    }

    fn parse_datetime(&self, time_str: &str) -> chrono::ParseResult<DateTime<Utc>> {
        Self::parse_datetime_with_now(time_str, self.now)
    }

    fn parse_datetime_with_now(time_str: &str, now: DateTime<Utc>) -> chrono::ParseResult<DateTime<Utc>> {
        let time = NaiveTime::parse_from_str(time_str, "%H%M%S")?;

        Ok(Self::guess_date(time, now))
    }

    /// Find the closest datetime between today, yesterday and tomorrow
    fn guess_date(time: NaiveTime, now: DateTime<Utc>) -> DateTime<Utc> {
        let datetime = now.date().and_time(time)
            .expect("date should be valid");

        let one_day = chrono::Duration::days(1);
        let time_from_now = (now - datetime).num_seconds().abs();
        let time_from_yesterday = (now - (datetime - one_day)).num_seconds().abs();
        let time_to_tomorrow = (now - (datetime + one_day)).num_seconds().abs();
        if time_from_yesterday < time_from_now && time_from_yesterday < time_to_tomorrow {
            datetime - one_day
        } else if time_to_tomorrow < time_from_now && time_to_tomorrow < time_from_yesterday {
            datetime + one_day
        } else {
            datetime
        }
    }

    fn parse_altitude(pair: Pair<Rule>) -> Result<f64> {
        if let Some((reference, altitude_str)) = pair.as_str().splitn(2, "=").collect_tuple() {
            if reference == "A" {
                Ok(altitude_str.parse()?)
            } else {
                Err(anyhow::format_err!("Unknown altitude format {}", pair.as_str()))
            }
        } else {
            Err(anyhow::format_err!("Unknown altitude format {}", pair.as_str()))
        }
    }

    fn parse_comments(pair: Pair<Rule>, report: &mut Report) {
        for rule in pair.into_inner() {
            match rule.as_rule() {
                Rule::id => {
                    if let Ok(aircraft_id) = Self::parse_aircraft_id(rule) {
                        report.aircraft_id.replace(aircraft_id);
                    }
                },
                Rule::climb_rate => {
                    if let Ok(climb_rate) = rule.into_inner().next().unwrap().as_str().parse() {
                        report.climb_rate.replace(climb_rate);
                    }
                },
                Rule::turning_rate => {
                    if let Ok(climb_rate) = rule.into_inner().next().unwrap().as_str().parse() {
                        report.turning_rate.replace(climb_rate);
                    }
                },
                Rule::flight_level => {
                    if let Ok(flight_level) = rule.into_inner().next().unwrap().as_str().parse() {
                        report.flight_level.replace(flight_level);
                    }
                },
                _ => (),
            }
        }
    }

    fn parse_aircraft_id(pair: Pair<Rule>) -> Result<u32> {
        Ok(pair.as_str().split_at(2).1.parse()?)
    }
}

impl Default for APRSParser {
    fn default() -> Self {
        APRSParser::new(Utc::now())
    }
}

#[cfg(test)]
mod test {
    use chrono::NaiveDate;
    use dms_coordinates::{Bearing, DMS};
    use super::*;

    #[test]
    fn test_parse_example_01() {
        let parser = APRSParser::new("2022-10-16 16:00:00Z".parse().unwrap());
        let report: Report = parser.parse("OGN123456>OGNAPP:/123456h5123.45N/00123.45W'180/025/A=001000 !W66! id07123456 -100fpm +1.0rot FL011.00 gps4x5")
            .expect("should have parsed");
        assert_eq!(report.time, "2022-10-16 12:34:56Z".parse::<DateTime<Utc>>().unwrap());
        assert_eq!(report.coordinates.y(), DMS::new(51, 23, 45.0, Bearing::North).to_decimal_degrees());
        assert_eq!(report.coordinates.x(), DMS::new(1, 23, 45.0, Bearing::West).to_decimal_degrees());
        assert_eq!(report.track, 180.);
        assert_eq!(report.speed, 25.);
        assert_eq!(report.altitude, 1000.);
        assert_eq!(report.aircraft_id, Some(7123456));
        assert_eq!(report.climb_rate, Some(-100.));
        assert_eq!(report.turning_rate, Some(1.));
        assert_eq!(report.flight_level, Some(11.));
        assert_eq!(report.symbol, ['/', '\'']);
    }

    #[test]
    fn test_parse_datetime() {
        let today = NaiveDate::from_ymd(2022, 10, 16);
        let morning = DateTime::from_utc(today.and_time(NaiveTime::from_hms(2, 0, 0)), Utc);
        let evening = DateTime::<Utc>::from_utc(today.and_time(NaiveTime::from_hms(22, 0, 0)), Utc);

        assert_eq!(APRSParser::guess_date(NaiveTime::from_hms(1, 58, 0), morning).date_naive(),
                   NaiveDate::from_ymd(2022, 10, 16));
        assert_eq!(APRSParser::guess_date(NaiveTime::from_hms(23, 0, 0), morning).date_naive(),
                   NaiveDate::from_ymd(2022, 10, 15));
        assert_eq!(APRSParser::guess_date(NaiveTime::from_hms(1, 58, 0), evening).date_naive(),
                   NaiveDate::from_ymd(2022, 10, 17));
        assert_eq!(APRSParser::guess_date(NaiveTime::from_hms(23, 0, 0), evening).date_naive(),
                   NaiveDate::from_ymd(2022, 10, 16));
    }
}
