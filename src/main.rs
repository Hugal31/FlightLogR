use flightlogr::ogn::OGN_APRS_URL;
use std::fs::File;
use std::io::Read;
use std::net::TcpStream;

use anyhow::{anyhow, Result};
use clap::Parser;
use flightlogr::aprs::{
    client::{APRSClient, Credentials, Filter, FilterSpec, Reports},
    Report,
};
use serde::Deserialize;

/// Merged CLI arguments and config.
#[derive(Clone, Debug, Deserialize, Parser)]
#[command(author, version, about, long_about = None)]
struct PartialConfig {
    #[arg(long, help=format!("APRS URL [default: {}]", OGN_APRS_URL))]
    aprs_uri: Option<String>,
    #[arg(long)]
    aprs_user: Option<String>,
    #[arg(long)]
    aprs_password: Option<String>,
    #[arg(short, long, help = "Config file path")]
    #[serde(skip)]
    config_file: Option<String>,
    #[clap(flatten)]
    filters: Option<FilterConfig>,
}

#[derive(Clone, Debug, Deserialize, Parser)]
struct FilterConfig {
    #[arg(long)]
    latitude: Option<i32>,
    #[arg(long)]
    longitude: Option<i32>,
    #[arg(long)]
    range: Option<u32>,
}

impl PartialConfig {
    /// Replace this config missing fields with other's.
    pub fn merge_missing(&mut self, other: PartialConfig) {
        if self.aprs_uri.is_none() {
            self.aprs_uri = other.aprs_uri;
        }
        if self.aprs_user.is_none() {
            self.aprs_user = other.aprs_user;
        }
        if self.aprs_password.is_none() {
            self.aprs_password = other.aprs_password;
        }
        if self.filters.is_none() {
            self.filters = other.filters
        }
    }

    pub fn complete(self) -> Result<Config> {
        let PartialConfig {
            aprs_uri,
            aprs_user,
            aprs_password,
            filters,
            ..
        } = self;
        Ok(Config {
            aprs_uri: aprs_uri.unwrap_or_else(|| OGN_APRS_URL.to_owned()),
            aprs_user: aprs_user,
            aprs_password: aprs_password,
            filters: match filters.as_ref() {
                Some(FilterConfig {
                    latitude: Some(latitude),
                    longitude: Some(longitude),
                    range: Some(range),
                }) => vec![Filter::new(FilterSpec::Range {
                    lat: *latitude,
                    lon: *longitude,
                    range: *range,
                })],
                _ => vec![],
            },
        })
    }
}

#[derive(Clone, Debug)]
struct Config {
    aprs_uri: String,
    aprs_user: Option<String>,
    aprs_password: Option<String>,
    filters: Vec<Filter>,
}

impl Config {
    pub fn parse() -> Config {
        match Self::try_parse() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("error: {}", e);
                std::process::exit(2);
            }
        }
    }

    pub fn try_parse() -> Result<Config> {
        let mut partial_config_cli = PartialConfig::parse();
        if let Some(config_file) = &partial_config_cli.config_file {
            let mut content = String::new();
            File::open(config_file)?.read_to_string(&mut content)?;
            let partial_config_file = toml::from_str(&content)?;
            partial_config_cli.merge_missing(partial_config_file);
        }

        partial_config_cli.complete()
    }
}

fn main() -> Result<()> {
    let config = Config::parse();
    let report_stream = open_reports(config)?;
    for report in report_stream {
        match report {
            Ok(r) => println!("{}", r),
            Err(e) => eprintln!("could not parse report: {}", e),
        };
    }

    Ok(())
}

fn open_reports(config: Config) -> Result<Box<dyn Iterator<Item = Result<Report>>>> {
    match config.aprs_uri.as_str() {
        "-" => Ok(Box::new(Reports::new(std::io::stdin()))),
        file_uri if file_uri.starts_with("file://") => {
            let path = &file_uri[7..];
            let stream = File::open(path)?;
            Ok(Box::new(Reports::new(stream)))
        }
        file_uri if file_uri.starts_with("./") => {
            let path = &file_uri[2..];
            let stream = File::open(path)?;
            Ok(Box::new(Reports::new(stream)))
        }
        url => {
            if config.filters.is_empty() {
                eprintln!("WARNING: No filters declared.");
            }
            let stream = TcpStream::connect(url)?;
            let client = APRSClient::login(
                stream,
                &Credentials {
                    user: config
                        .aprs_user
                        .ok_or_else(|| anyhow!("missing APRS username"))?
                        .to_string(),
                    password: config
                        .aprs_password
                        .ok_or_else(|| anyhow!("missing APRS password"))?
                        .to_string(),
                    app_name: "flightLGo".to_owned(), // env!("CARGO_PKG_NAME").to_owned(),
                    app_version: "0.0.0b1".to_string(), //, env!("CARGO_PKG_VERSION").to_owned(),
                },
                &config.filters,
                false,
            )?;
            Ok(Box::new(client.reports()))
        }
    }
}
