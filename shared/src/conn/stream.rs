use super::PeerID;
use anyhow::{anyhow, Result};
use std::fmt::Debug;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug)]
#[allow(dead_code)]
pub(super) enum InnerStream {
    Tcp(TcpStream),
    Rdma(()),
}

#[derive(Debug)]
pub(super) struct Stream {
    pub inner: InnerStream,
    pub peer_id: PeerID,
    pub authenticated: bool,
}

impl Stream {
    pub(super) fn new_tcp(stream: TcpStream, peer_id: PeerID) -> Self {
        Self {
            inner: InnerStream::Tcp(stream),
            peer_id,
            authenticated: false,
        }
    }

    pub(super) async fn readable(&self) -> Result<()> {
        match &self.inner {
            InnerStream::Tcp(s) => loop {
                s.readable().await?;
                match s.try_read(&mut [0; 0]) {
                    Ok(_) => break Ok(()),
                    Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(err) => break Err(anyhow!(err)),
                }
            },
            InnerStream::Rdma(_) => unimplemented!(),
        }
    }

    pub(super) async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        match &mut self.inner {
            InnerStream::Tcp(ref mut s) => {
                s.read_exact(buf).await?;
                Ok(())
            }
            InnerStream::Rdma(_) => unimplemented!(),
        }
    }

    pub(super) async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        match &mut self.inner {
            InnerStream::Tcp(ref mut s) => Ok(s.write_all(buf).await?),
            InnerStream::Rdma(_) => unimplemented!(),
        }
    }
}
