use std::{pin::{pin, Pin}, task::{Context, Poll}, future::Future};
use std::ops::DerefMut;
use std::process::Output;
use anyhow::Result;
use tokio::{
    io::{AsyncWriteExt, BufReader, AsyncBufReadExt},
    net::TcpStream
};
use tokio_stream::Stream;
use futures::StreamExt as _;

struct AutoClient {
    url: String,
    stream: BufReader<TcpStream>,
}

impl AutoClient {
    pub async fn new(url: &str) -> Result<Self> {
        Ok(Self {
            url: url.to_owned(),
            stream: BufReader::new(Self::connect(url).await?),
        })
    }

    async fn connect(url: &str) -> Result<TcpStream> {
        println!("Logging in");
        let stream = TcpStream::connect(url).await?;
        println!("Logged in");
        Ok(stream)
    }

    async fn reconnect(&mut self) -> Result<()> {
        self.stream.shutdown().await?;
        let new_stream = Self::connect(&self.url).await?;
        self.stream = BufReader::new(new_stream);
        Ok(())
    }

    async fn get_line(&mut self) -> Result<String> {
        loop {
            let mut string = String::new();
            if self.stream.read_line(&mut string).await? > 0 {
                return Ok(string);
            } else {
                self.reconnect().await?;
            }
        }
    }

    pub fn as_stream(self) -> impl Stream<Item=Result<String>> {
        futures::stream::unfold(self, |mut client| async {
            let res = client.get_line().await;
            Some((res, client))
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let mut client = AutoClient::new("localhost:7777").await?;
    let mut stream = pin!(client.as_stream());
    while let Some(Ok(s)) = stream.next().await {
        println!("Line is: {}", s);
    }
    Ok(())
}
