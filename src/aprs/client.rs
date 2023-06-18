use std::io::{BufRead, BufReader};
use std::{
    fmt::{self, Display, Formatter, Write as _},
    io::{Read, Result as IoResult, Write},
};

use anyhow::{format_err, Result};

use super::Report;

#[derive(Clone, Debug)]
pub struct Credentials {
    pub user: String,
    pub password: String,
    pub app_name: String,
    pub app_version: String,
}

#[derive(Clone, Debug)]
pub struct Filter {
    pub negative: bool,
    pub spec: FilterSpec,
}

impl Filter {
    pub fn new(spec: FilterSpec) -> Self {
        Self {
            negative: false,
            spec,
        }
    }

    pub fn negate(spec: FilterSpec) -> Self {
        Self {
            negative: true,
            spec,
        }
    }
}

impl Display for Filter {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.negative {
            f.write_char('-')?;
        }
        Display::fmt(&self.spec, f)
    }
}

#[derive(Clone, Debug)]
pub enum FilterSpec {
    Default,
    /// Range around a geo-location
    Range {
        // Latitude degrees
        lat: i32,
        // Longitude degrees
        lon: i32,
        // Range in km
        range: u32,
    },
    /// Range around my position reports, distance in km.
    MyRange(u32),
}

impl Display for FilterSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => f.write_str("default"),
            Self::Range { lat, lon, range } => write!(f, "r/{}/{}/{}", lat, lon, range),
            Self::MyRange(range) => write!(f, "m/{}", range),
        }
    }
}

pub struct APRSClient<R: Read> {
    stream: BufReader<R>,
}

impl<RW: Read + Write> APRSClient<RW> {
    pub fn login(
        mut stream: RW,
        creds: &Credentials,
        filters: &[Filter],
        verify_login: bool,
    ) -> Result<Self> {
        login_to_aprs(&mut stream, creds, filters)?;
        let mut buf_reader = BufReader::new(stream);

        if verify_login {
            let mut line = String::new();
            loop {
                line.clear();
                buf_reader.read_line(&mut line)?;
                if line.starts_with('#') {
                    if line.starts_with("# aprs") {
                        // Connexion comment
                    } else if line.starts_with("# logresp") && line.contains("unverified") {
                        return Err(format_err!(
                            "invalid credentials: got response \"{}\"",
                            line
                        ));
                    } else {
                        // Any other comment, accept as logged in
                        break;
                    }
                }
            }
        }

        Ok(Self { stream: buf_reader })
    }
}

impl<R: Read> APRSClient<R> {
    pub fn reports(self) -> Reports<R> {
        Reports::new_buf(self.stream)
    }
}

pub struct Reports<R: Read> {
    stream: BufReader<R>,
}

impl<R: Read> Reports<R> {
    pub fn new(stream: R) -> Self {
        Self::new_buf(BufReader::new(stream))
    }

    pub fn new_buf(stream: BufReader<R>) -> Self {
        Self { stream }
    }
}

impl<R: Read> Iterator for Reports<R> {
    type Item = Result<Report>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = String::new();
        loop {
            match self.stream.read_line(&mut line) {
                Ok(0) => return None,
                Ok(_) if line.starts_with('#') => (),
                Ok(_) => return Some(line.parse()),
                Err(e) => return Some(Err(e.into())),
            }
            line.clear();
        }
    }
}

pub fn login_to_aprs<W: Write>(
    stream: &mut W,
    creds: &Credentials,
    filters: &[Filter],
) -> IoResult<()> {
    write!(
        stream,
        "user {} pass {} vers {} {}",
        creds.user, creds.password, creds.app_name, creds.app_version
    )?;
    if !filters.is_empty() {
        write!(stream, " filter")?;
        for filter in filters {
            write!(stream, " {}", filter)?;
        }
    }
    writeln!(stream)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filters_fmt() {
        assert_eq!(Filter::new(FilterSpec::MyRange(42)).to_string(), "m/42");
        assert_eq!(
            Filter::negate(FilterSpec::Range {
                lat: 40,
                lon: 18,
                range: 10
            })
            .to_string(),
            "-r/40/18/10"
        );
    }
}
