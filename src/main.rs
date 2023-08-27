use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use flightlogr::aprs::{
    client::{AutoClient, Credentials, Filter, FilterSpec, Reports},
    Report,
};
use serde::Deserialize;
use tokio_stream::{Stream, StreamExt as _};

use flightlogr::events::{DateSource, EventDetector, FixedDateTimeSource, SystemDateTimeSource};
use flightlogr::ogn::OGN_APRS_URL;

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
    #[arg(long)]
    #[serde(skip)]
    now: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Parser)]
struct FilterConfig {
    #[arg(long)]
    latitude: Option<f32>,
    #[arg(long)]
    longitude: Option<f32>,
    /// Range in km.
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
            now,
            ..
        } = self;
        Ok(Config {
            aprs_uri: aprs_uri.unwrap_or_else(|| OGN_APRS_URL.to_owned()),
            aprs_user,
            aprs_password,
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
            now,
        })
    }
}

#[derive(Clone, Debug)]
struct Config {
    aprs_uri: String,
    aprs_user: Option<String>,
    aprs_password: Option<String>,
    filters: Vec<Filter>,
    now: Option<DateTime<Utc>>,
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

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let config = Config::parse();
    init_logging();
    let datetime_source = if let Some(now) = config.now {
        Box::new(FixedDateTimeSource(now)) as Box<dyn DateSource>
    } else {
        Box::new(SystemDateTimeSource) as Box<dyn DateSource>
    };
    let mut report_stream = open_reports(config).await?;
    let mut event_detector = EventDetector::with_date_source(datetime_source);
    while let Some(report) = report_stream.next().await {
        match report {
            Ok(r) => {
                event_detector.add_report(&r);
            }
            Err(e) => log::warn!("could not parse report: {}", e),
        };
    }

    Ok(())
}

fn init_logging() {
    let logging_env = "FLIGHTLOGR_LOG";
    if std::env::var(logging_env).is_ok() {
        env_logger::Builder::from_env(logging_env)
            .try_init()
            .expect("Failed to initialize logging")
    }
}

async fn open_reports(config: Config) -> Result<Box<dyn Stream<Item = Result<Report>> + Unpin>> {
    match config.aprs_uri.as_str() {
        "-" => Ok(Box::new(Reports::new(tokio::io::BufReader::new(
            tokio::io::stdin(),
        )))),
        file_uri if file_uri.starts_with("file://") => open_report_file(&file_uri[7..]).await,
        file_uri if file_uri.starts_with("./") => open_report_file(&file_uri[2..]).await,
        url => {
            if config.filters.is_empty() {
                log::warn!("No filters declared.");
            }
            let client = AutoClient::new(
                url.to_string(),
                Credentials {
                    user: config
                        .aprs_user
                        .ok_or_else(|| anyhow!("missing APRS username"))?,
                    password: config
                        .aprs_password
                        .ok_or_else(|| anyhow!("missing APRS password"))?,
                    app_name: "flightLGo".to_owned(), // env!("CARGO_PKG_NAME").to_owned(),
                    app_version: "0.0.0b1".to_string(), //, env!("CARGO_PKG_VERSION").to_owned(),
                },
                config.filters.clone(),
                false,
            )
            .await?;
            log::info!("Connected to APRS server");
            Ok(Box::new(client))
        }
    }
}

#[cfg(unix)]
async fn open_report_file<P: AsRef<Path>>(
    path: P,
) -> Result<Box<dyn Stream<Item = Result<Report>> + Unpin>> {
    use std::fs::metadata;
    use std::os::unix::fs::FileTypeExt;

    let path = path.as_ref();
    if metadata(path)?.file_type().is_socket() {
        Ok(Box::new(Reports::new(tokio::io::BufReader::new(
            tokio::net::UnixStream::connect(path).await?,
        ))))
    } else {
        Ok(Box::new(Reports::new(tokio::io::BufReader::new(
            tokio::fs::File::open(path).await?,
        ))))
    }
}

#[cfg(not(unix))]
async fn open_report_file<P: AsRef<Path>>(
    path: P,
) -> Result<Box<dyn Stream<Item = Result<Report>> + Unpin>> {
    Ok(Box::new(Reports::new(tokio::fs::File::open(path).await?)))
}
