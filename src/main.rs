use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use flightlogr::{
    aprs::{
        client::{AutoClient, Credentials, Filter, FilterSpec, Reports},
        Report,
    },
    events,
};
use serde::Deserialize;
use tokio_stream::{Stream, StreamExt as _};

use flightlogr::events::{DateSource, EventDetector, FixedDateTimeSource, SystemDateTimeSource};
use flightlogr::ogn::ddb::Device;
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
    #[clap(skip)]
    firebase: Option<FirebaseConfig>,
    #[arg(long)]
    #[serde(skip)]
    now: Option<DateTime<Utc>>,
    #[arg(short, action)]
    #[serde(skip)]
    test_push: bool,
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

#[derive(Clone, Debug, Deserialize)]
struct FirebaseConfig {
    api_key: String,
    topic_id: String,
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
        if self.firebase.is_none() {
            self.firebase = other.firebase;
        }
    }

    pub fn complete(self) -> Result<Config> {
        let PartialConfig {
            aprs_uri,
            aprs_user,
            aprs_password,
            filters,
            firebase,
            now,
            test_push,
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
            firebase,
            now,
            test_push,
        })
    }
}

#[derive(Clone, Debug)]
struct Config {
    aprs_uri: String,
    aprs_user: Option<String>,
    aprs_password: Option<String>,
    filters: Vec<Filter>,
    firebase: Option<FirebaseConfig>,
    now: Option<DateTime<Utc>>,
    test_push: bool,
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
    init_logging();
    let config = Config::parse();

    let firebase_conf = config
        .firebase
        .as_ref()
        .ok_or_else(|| anyhow!("Missing firebase configuration"))?;
    let mut sender = flightlogr::notifications::FirebaseNotificationSender::new(
        &firebase_conf.api_key,
        &firebase_conf.topic_id,
    );
    sender.set_ddb(get_ogn_ddb().await?);

    if config.test_push {
        sender
            .notify_event(&crate::events::Event::AircraftChangedState(
                crate::events::AircraftChangeStatedEvent {
                    aircraft_id: "Test".to_string(),
                    new_state: crate::events::AircraftState::Airborne,
                    date: Utc::now(),
                },
            ))
            .await?;
    }

    let datetime_source: Box<dyn DateSource> = if let Some(now) = config.now {
        Box::new(FixedDateTimeSource(now))
    } else {
        Box::new(SystemDateTimeSource)
    };
    let mut report_stream = open_reports(config).await?;
    let mut event_detector = EventDetector::with_date_source(datetime_source);
    while let Some(report) = report_stream.next().await {
        match report {
            Ok(r) => {
                if let Some(event) = event_detector.add_report(&r) {
                    sender.notify_event(&event).await?;
                }
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

async fn get_ogn_ddb() -> Result<HashMap<String, Device>> {
    use flightlogr::ogn::ddb::{index_by_id, read_database, OGN_DDB_URL};
    let body = reqwest::get(OGN_DDB_URL).await?.text().await?;
    read_database(body.as_bytes()).map(index_by_id)
}

async fn open_reports(config: Config) -> Result<Box<dyn Stream<Item = Result<Report>> + Unpin>> {
    match config.aprs_uri.as_str() {
        "-" => Ok(Box::new(Reports::new(tokio::io::BufReader::new(
            tokio::io::stdin(),
        )))),
        file_uri if file_uri.starts_with("file://") => open_report_file(&file_uri[7..]).await,
        file_uri if file_uri.starts_with("./") => open_report_file(&file_uri[2..]).await,
        url => {
            log::debug!("Connecting to {url}");
            if config.filters.is_empty() {
                log::warn!("No filters declared.");
            }
            let client = AutoClient::new(
                url.to_string(),
                Credentials {
                    user: config
                        .aprs_user
                        .ok_or_else(|| anyhow!("missing APRS username"))?,
                    password: config.aprs_password.unwrap_or_else(|| "-1".to_owned()),
                    app_name: env!("CARGO_PKG_NAME").to_owned(),
                    app_version: env!("CARGO_PKG_VERSION").to_owned(),
                },
                config.filters.clone(),
                false,
            )
            .await?;
            log::info!("Connected to APRS server");
            // Meh
            Ok(Box::new(Box::pin(client.as_stream())))
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
