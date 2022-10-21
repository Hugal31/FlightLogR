use anyhow::{format_err, Context, Result};
use chrono::{DateTime, NaiveTime, Utc};
use dms_coordinates::{Bearing, DMS};
use itertools::Itertools;
use pest::{iterators::Pair, Parser};
use pest_derive::Parser;

use super::{
    Comment, PositionReport, PositionReportCoordinates, PositionReportDataExtension,
    PositionReportTime, Report, Symbol, DHM,
};

#[derive(Parser)]
#[grammar = "aprs/aprs_v2.pest"]
pub struct APRSParser;

impl APRSParser {
    pub fn parse(&self, s: &str) -> Result<Report> {
        let aprs_ast = <Self as Parser<Rule>>::parse(Rule::aprs_report, s)?
            .next()
            .unwrap();

        let mut rules = aprs_ast.into_inner();
        let _header = rules.next().unwrap();
        Self::parse_information_field(rules.next().unwrap())
    }

    fn parse_information_field(information_field_ast: Pair<Rule>) -> Result<Report> {
        match information_field_ast.as_rule() {
            Rule::position_report => {
                Self::parse_position_report(information_field_ast.into_inner().next().unwrap())
                    .map(Report::PositionReport)
            }
            _ => unreachable!(),
        }
    }

    fn parse_position_report(pair: Pair<Rule>) -> Result<PositionReport> {
        let rule_type = pair.as_rule();
        let mut inner = pair.into_inner();
        let timestamp = if let Rule::position_report_with_timestamp = rule_type {
            Some(Self::parse_timestamp(inner.next().unwrap())?)
        } else {
            None
        };

        let body = inner.next().unwrap();
        let (symbol, position, data_extension) = Self::parse_position_report_body(body)?;
        let comments = if let Some(comments_ast) = inner.next() {
            Self::parse_comments(comments_ast)?
        } else {
            vec![]
        };

        Ok(PositionReport {
            timestamp,
            symbol,
            position,
            data_extension,
            comments,
        })
    }

    fn parse_timestamp(pair: Pair<Rule>) -> Result<PositionReportTime> {
        match pair.as_rule() {
            Rule::time_hms => {
                let digits = &pair.as_str()[0..6];
                let hour = (&digits[0..2]).parse().context("invalid hour")?;
                let minutes = (&digits[2..4]).parse().context("invalid minutes")?;
                let seconds = (&digits[4..6]).parse().context("invalid seconds")?;
                Ok(PositionReportTime::HMS(NaiveTime::from_hms(
                    hour, minutes, seconds,
                )))
            }
            Rule::time_dhm => {
                let digits = &pair.as_str()[0..6];
                let timezone_indicator = pair.as_str().chars().nth(6).unwrap();
                let day = (&digits[0..2]).parse().context("invalid day")?;
                let hour = (&digits[2..4]).parse().context("invalid hour")?;
                let minutes = (&digits[4..6]).parse().context("invalid minutes")?;
                Ok(PositionReportTime::DHM(DHM::new(
                    day,
                    hour,
                    minutes,
                    timezone_indicator == 'z',
                )))
            }
            _ => unreachable!(),
        }
    }

    fn parse_position_report_body(
        pair: Pair<Rule>,
    ) -> Result<(
        Symbol,
        PositionReportCoordinates,
        Option<PositionReportDataExtension>,
    )> {
        let mut inner = pair.into_inner();
        let coordinates_ast = inner.next().unwrap();
        match coordinates_ast.as_rule() {
            Rule::latitude_longitude_symbol => {
                let (coords, symbol) = Self::parse_coords_and_symbol(coordinates_ast)?;
                let data_extension = inner
                    .next()
                    .map(Self::parse_position_report_data_extension)
                    .transpose()?;
                Ok((symbol, coords, data_extension))
            }
            Rule::compressed_location_and_symbol => {
                todo!()
            }
            _ => unreachable!(),
        }
    }

    fn parse_coords_and_symbol(pair: Pair<Rule>) -> Result<(PositionReportCoordinates, [u8; 2])> {
        let mut inner = pair.into_inner();
        let latitude_str = inner.next().unwrap().as_str();
        let symbol_1 = inner.next().unwrap().as_str().bytes().next().unwrap();
        let longitude_str = inner.next().unwrap().as_str();
        let symbol_2 = inner.next().unwrap().as_str().bytes().next().unwrap();

        let (latitude, lat_digits) = Self::parse_latlon_number(latitude_str)?;
        let (longitude, lon_digits) = Self::parse_latlon_number(longitude_str)?;

        Ok((
            PositionReportCoordinates {
                point: geo::Point((longitude, latitude).into()),
                lat_digits,
                lon_digits,
            },
            [symbol_1, symbol_2],
        ))
    }

    /// Given a potentially ambiguous coordinate, such as "420 .  S" return the number and
    /// the number of digits.
    fn parse_latlon_number(s: &str) -> Result<(f64, u8)> {
        let (before_dot, after_dot) = s
            .splitn(2, '.')
            .collect_tuple()
            .ok_or_else(|| format_err!("invalid coordinate: \"{}\"", s))?;
        let (degrees_str, minutes_str) =
            before_dot.split_at(if before_dot.len() == 5 { 3 } else { 2 });
        let (seconds_str, bearing) = after_dot.split_at(2);
        let degrees = degrees_str.parse()?;
        let (minutes, n_digits_minutes) = Self::parse_ambiguous_number_pair(minutes_str)?;
        let (seconds, n_digits_seconds) = Self::parse_ambiguous_number_pair(seconds_str)?;
        let bearing = match bearing {
            "N" => Bearing::North,
            "E" => Bearing::East,
            "S" => Bearing::South,
            "W" => Bearing::West,
            _ => return Err(format_err!("invalid coordinate bearing: {}", s)),
        };
        Ok((
            DMS::new(degrees, minutes, seconds as _, bearing).to_decimal_degrees(),
            n_digits_minutes + n_digits_seconds,
        ))
    }

    fn parse_ambiguous_number_pair(s: &str) -> Result<(i32, u8)> {
        match s {
            "  " => Ok((0, 0)),
            ambiguous if ambiguous.ends_with(' ') => (&ambiguous[0..1])
                .parse()
                .map(|n: i32| (n * 10, 1))
                .map_err(Into::into),
            s => s.parse().map(|n| (n, 2)).map_err(Into::into),
        }
    }

    fn parse_position_report_data_extension(
        pair: Pair<Rule>,
    ) -> Result<PositionReportDataExtension> {
        match pair.as_rule() {
            Rule::course_speed => {
                let (course_ast, speed_ast) = pair.into_inner().collect_tuple().unwrap();
                let course = if let Rule::null_three_digits = course_ast.as_rule() {
                    0
                } else {
                    course_ast.as_str().parse()?
                };
                let speed = if let Rule::null_three_digits = speed_ast.as_rule() {
                    0
                } else {
                    speed_ast.as_str().parse()?
                };
                Ok(PositionReportDataExtension::CourseSpeed { course, speed })
            }
            Rule::phg | Rule::radio_range | Rule::dfs_signal_strength => todo!(),
            _ => {
                dbg!(pair);
                unreachable!()
            }
        }
    }

    /// Find the closest datetime between today, yesterday and tomorrow
    fn guess_date(time: NaiveTime, now: DateTime<Utc>) -> DateTime<Utc> {
        let datetime = now.date().and_time(time).expect("date should be valid");

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

    fn parse_comments(pair: Pair<Rule>) -> Result<Vec<Comment>> {
        Ok(pair
            .into_inner()
            // Store failures as unknown
            .map(|p| {
                Self::parse_comment(p.clone())
                    .unwrap_or_else(|_| Comment::Unknown(p.as_str().to_owned()))
            })
            .collect())
    }

    fn parse_comment(pair: Pair<Rule>) -> Result<Comment> {
        match pair.as_rule() {
            Rule::altitude => (&pair.as_str()["/A=".len()..])
                .parse()
                .map(Comment::Altitude)
                .map_err(Into::into),
            Rule::position_precision_enhancement => {
                let digits = &pair.as_str()[2..4];
                Ok(Comment::PositionPrecisionEnhancement {
                    lat: (&digits[0..1]).parse()?,
                    lon: (&digits[1..2]).parse()?,
                })
            }
            Rule::climb_rate => Ok(Comment::ClimbRate(
                (pair.into_inner().next().unwrap().as_str()).parse()?,
            )),
            Rule::rotation_rate => Ok(Comment::TurningRate(
                (pair.into_inner().next().unwrap().as_str()).parse()?,
            )),
            Rule::flight_level => Ok(Comment::FlightLevel(
                (pair.into_inner().next().unwrap().as_str()).parse()?,
            )),
            Rule::id => Ok(Comment::Id((&pair.as_str()[2..]).to_string())),
            _ => Ok(Comment::Unknown(pair.as_str().to_owned())),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::aprs::PositionReportTime;
    use chrono::NaiveDate;
    use dms_coordinates::{Bearing, DMS};

    #[test]
    fn test_parse_example_01() {
        let report: Report = "OGN123456>OGNAPP:/123456h5123.45N/00123.45W'180/025/A=001000 !W65! id07123456 -100fpm +1.0rot FL011.00 gps4x5".parse()
            .expect("should have parsed");

        #[allow(irrefutable_let_patterns)]
        if let Report::PositionReport(report) = report {
            assert_eq!(
                report.timestamp,
                Some(PositionReportTime::HMS("12:34:56".parse().unwrap()))
            );
            assert_eq!(report.symbol, [b'/', b'\'']);
            assert_eq!(
                report.point().y(),
                DMS::new(51, 23, 45.6, Bearing::North).to_decimal_degrees()
            );
            assert_eq!(
                report.point().x(),
                DMS::new(1, 23, 45.5, Bearing::West).to_decimal_degrees()
            );
            assert_eq!(report.course(), Some(180));
            assert_eq!(report.speed(), Some(25.0));
            assert_eq!(
                report.comments.iter().find_map(|c| match c {
                    &Comment::Altitude(a) => Some(a),
                    _ => None,
                }),
                Some(1000.0)
            );
            assert_eq!(
                report.comments.iter().find_map(|c| match c {
                    &Comment::PositionPrecisionEnhancement { lat, lon } => Some((lat, lon)),
                    _ => None,
                }),
                Some((6, 5))
            );
            assert_eq!(
                report.comments.iter().find_map(|c| match c {
                    Comment::Id(id) => Some(id as &str),
                    _ => None,
                }),
                Some("07123456")
            );
            assert_eq!(
                report.comments.iter().find_map(|c| match c {
                    &Comment::ClimbRate(cr) => Some(cr),
                    _ => None,
                }),
                Some(-100.0)
            );
            assert_eq!(
                report.comments.iter().find_map(|c| match c {
                    &Comment::TurningRate(tr) => Some(tr),
                    _ => None,
                }),
                Some(1.0)
            );
            assert_eq!(
                report.comments.iter().find_map(|c| match c {
                    &Comment::FlightLevel(fl) => Some(fl),
                    _ => None,
                }),
                Some(11.0)
            );
        }
    }

    #[test]
    fn test_parse_ambiguous_number() {
        assert_eq!(
            APRSParser::parse_ambiguous_number_pair("  ").expect("should parse"),
            (0, 0)
        );
        assert_eq!(
            APRSParser::parse_ambiguous_number_pair("8 ").expect("should parse"),
            (80, 1)
        );
        assert_eq!(
            APRSParser::parse_ambiguous_number_pair("82").expect("should parse"),
            (82, 2)
        );
    }

    #[test]
    fn test_parse_datetime() {
        let today = NaiveDate::from_ymd(2022, 10, 16);
        let morning = DateTime::from_utc(today.and_time(NaiveTime::from_hms(2, 0, 0)), Utc);
        let evening = DateTime::<Utc>::from_utc(today.and_time(NaiveTime::from_hms(22, 0, 0)), Utc);

        assert_eq!(
            APRSParser::guess_date(NaiveTime::from_hms(1, 58, 0), morning).date_naive(),
            NaiveDate::from_ymd(2022, 10, 16)
        );
        assert_eq!(
            APRSParser::guess_date(NaiveTime::from_hms(23, 0, 0), morning).date_naive(),
            NaiveDate::from_ymd(2022, 10, 15)
        );
        assert_eq!(
            APRSParser::guess_date(NaiveTime::from_hms(1, 58, 0), evening).date_naive(),
            NaiveDate::from_ymd(2022, 10, 17)
        );
        assert_eq!(
            APRSParser::guess_date(NaiveTime::from_hms(23, 0, 0), evening).date_naive(),
            NaiveDate::from_ymd(2022, 10, 16)
        );
    }
}
