use std::fs::File;
use std::io::Read;
use std::net::TcpStream;
use flightlogr::ogn::OGN_APRS_URL;

use anyhow::{anyhow, Result};
use clap::Parser;
use serde::{Deserialize};
use flightlogr::aprs::client::{APRSClient, Credentials, Filter, FilterSpec};

/// Merged CLI arguments and config.
#[derive(Clone, Debug, Deserialize, Parser)]
#[command(author, version, about, long_about = None)]
struct PartialConfig {
    #[arg(long, help=format!("APRS URL [default: {}]", OGN_APRS_URL))]
    aprs_url: Option<String>,
    #[arg(long)]
    aprs_user: Option<String>,
    #[arg(long)]
    aprs_password: Option<String>,
    #[arg(short, long, help="Config file path")]
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
        if self.aprs_url.is_none() {
            self.aprs_url = other.aprs_url;
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
            aprs_url,
            aprs_user,
            aprs_password,
            filters,
            ..
        } = self;
        Ok(Config {
            aprs_url: aprs_url.unwrap_or_else(|| OGN_APRS_URL.to_owned()),
            aprs_user: aprs_user.ok_or_else(|| anyhow!("missing APRS username"))?,
            aprs_password: aprs_password.ok_or_else(|| anyhow!("missing APRS password"))?,
            filters: vec![Filter::new(FilterSpec::Range {
                lat: filters.as_ref().ok_or_else(|| anyhow!("missing latitude"))?.latitude.ok_or_else(|| anyhow!("missing latitude"))?,
                lon: filters.as_ref().ok_or_else(|| anyhow!("missing latitude"))?.longitude.ok_or_else(|| anyhow!("missing longitude"))?,
                range: filters.as_ref().ok_or_else(|| anyhow!("missing latitude"))?.range.ok_or_else(|| anyhow!("missing range"))?,
            })]
        })
    }
}

#[derive(Clone, Debug)]
struct Config {
    aprs_url: String,
    aprs_user: String,
    aprs_password: String,
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
    let stream = TcpStream::connect(config.aprs_url)?;
    let client = APRSClient::login(stream, &Credentials {
        user: config.aprs_user.to_string(),
        password: config.aprs_password.to_string(),
        app_name: "flightLGo".to_owned(), // env!("CARGO_PKG_NAME").to_owned(),
        app_version: "0.0.0b1".to_string(),//, env!("CARGO_PKG_VERSION").to_owned(),
    },
                                   &config.filters,
                                   false)?;

    for report in client.reports() {
        match report {
            Ok(r) => println!("{}", r),
            Err(e) => eprintln!("could not parse report: {}", e),
        };
    }

    Ok(())
}
