use std::{
    fmt::{self, Display, Formatter, Write as _},
    future::Future as _,
    io::{BufRead, BufReader, Read, Result as IoResult, Write},
    pin::Pin,
    task::{Context, Poll},
};

use anyhow::{format_err, Result};
use futures::{FutureExt as _, StreamExt as _};
use tokio::{
    io::{
        AsyncBufRead, AsyncBufReadExt as _, AsyncReadExt, AsyncWriteExt,
        BufReader as AsyncBufReader, Error as FutError,
    },
    net::TcpStream,
    pin,
};
use tokio_stream::Stream;

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
        lat: f32,
        // Longitude degrees
        lon: f32,
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

pub struct APRSClient<R> {
    stream: R,
}

impl<RW: Read + Write> APRSClient<BufReader<RW>> {
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

impl<RW: AsyncReadExt + AsyncWriteExt + Unpin> APRSClient<AsyncBufReader<RW>> {
    pub async fn async_login(
        mut stream: RW,
        creds: &Credentials,
        filters: &[Filter],
        verify_login: bool,
    ) -> Result<Self> {
        async_login_to_aprs(&mut stream, creds, filters).await?;
        let mut buf_reader = AsyncBufReader::new(stream);

        if verify_login {
            let mut line = String::new();
            loop {
                line.clear();
                buf_reader.read_line(&mut line).await?;
                if line.starts_with('#') {
                    if line.starts_with("# aprs") {
                        // Connexion comment
                    } else if line.starts_with("# logresp") && line.contains("unverified") {
                        return Err(format_err!("invalid credentials: got response \"{line}\""));
                    } else if line.starts_with("# Invalid username format") {
                        return Err(format_err!("error: got response \"{line}\""));
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

impl<R> APRSClient<R> {
    pub fn reports(self) -> Reports<R> {
        Reports::new(self.stream)
    }
}

pub struct Reports<R> {
    stream: R,
}

impl<R> Reports<R> {
    pub fn new(stream: R) -> Self {
        Self { stream }
    }
}

impl<R: BufRead> Iterator for Reports<R> {
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

impl<R: AsyncBufRead + Unpin> Stream for Reports<R> {
    type Item = Result<Report>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut line: String = String::new();

        loop {
            let read_line = self.stream.read_line(&mut line);
            pin!(read_line);
            match read_line.poll(cx) {
                Poll::Ready(r) => match r {
                    Ok(0) => return Poll::Ready(None),
                    Ok(_) if line.starts_with('#') => {
                        let comment = line.trim_start_matches('#').trim();
                        log::debug!("Received comment {comment}");
                    }
                    Ok(_) => return Poll::Ready(Some(line.parse())),
                    Err(e) => return Poll::Ready(Some(Err(e.into()))),
                },
                Poll::Pending => return Poll::Pending,
            }
            line.clear();
        }
    }
}

/// Auto reconnecting client.
pub struct AutoClient {
    url: String,
    creds: Credentials,
    filters: Vec<Filter>,
    verify_login: bool,
    reports: Reports<AsyncBufReader<TcpStream>>,
}

impl AutoClient {
    pub async fn new(
        url: String,
        creds: Credentials,
        filters: Vec<Filter>,
        verify_login: bool,
    ) -> Result<Self> {
        let client = Self::create_client(&url, &creds, &filters, verify_login).await?;
        let reports = client.reports();
        Ok(Self {
            url,
            creds,
            filters,
            verify_login,
            reports,
        })
    }

    async fn create_client(
        url: &str,
        creds: &Credentials,
        filters: &[Filter],
        verify_login: bool,
    ) -> Result<APRSClient<AsyncBufReader<TcpStream>>> {
        log::debug!("Connecting to APRS server at {url}");
        let stream = TcpStream::connect(&url).await?;
        log::debug!("Authenticating to APRS server");
        let client = APRSClient::async_login(stream, creds, filters, verify_login).await?;
        log::debug!("Authenticated");
        Ok(client)
    }

    async fn reconnect(&mut self) -> Result<()> {
        let client =
            Self::create_client(&self.url, &self.creds, &self.filters, self.verify_login).await?;
        self.reports = client.reports();
        Ok(())
    }
}

impl Stream for AutoClient {
    type Item = Result<Report>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.reports.poll_next_unpin(cx) {
                Poll::Ready(Some(x)) => return Poll::Ready(Some(x)),
                Poll::Ready(None) => {
                    log::info!("Disconnected from APRS server, reconnecting...");
                    let reconnecting = self.reconnect();
                    pin!(reconnecting);
                    match reconnecting.poll_unpin(cx) {
                        Poll::Ready(Ok(())) => (),
                        Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e))),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                Poll::Pending => return Poll::Pending,
            }
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

// Can't believe this doesn't exist somewere
macro_rules! async_write {
    ($dst:expr, $($arg:tt)*) => {
        $dst.write_all(format!($($arg)*).as_bytes())
    };
}

macro_rules! async_writeln {
    ($dst:expr $(,)?) => {
        async_write!($dst, "\n")
    };
    ($dst:expr, $($arg:tt)*) => {
        $dst.write_all(format!($($arg)*).as_bytes())
    };
}

pub async fn async_login_to_aprs<W: AsyncWriteExt + Unpin>(
    stream: &mut W,
    creds: &Credentials,
    filters: &[Filter],
) -> Result<(), FutError> {
    async_write!(
        stream,
        "user {} pass {} vers {} {}",
        creds.user,
        creds.password,
        creds.app_name,
        creds.app_version
    )
    .await?;
    if !filters.is_empty() {
        async_write!(stream, " filter").await?;
        for filter in filters {
            async_write!(stream, " {}", filter).await?;
        }
    }
    async_writeln!(stream).await?;
    stream.flush().await
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
