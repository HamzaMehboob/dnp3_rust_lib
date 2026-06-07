//! Tokio-based TCP transport for link-layer frames.

use std::time::Duration;

use dnp3_core::{ProtocolError, Result as ProtocolResult};
use dnp3_link::{encoded_payload_len, LinkFrame, START_BYTES};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{sleep, timeout};

/// Result type for TCP transport operations.
pub type Result<T> = std::result::Result<T, TcpTransportError>;

/// TCP transport errors.
#[derive(Debug, Error)]
pub enum TcpTransportError {
    /// Protocol validation failed.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A timeout elapsed.
    #[error("operation timed out")]
    Timeout,
    /// The connection has been shut down.
    #[error("connection shut down")]
    Shutdown,
}

/// TCP master/client transport configuration.
#[derive(Debug, Clone)]
pub struct TcpClientConfig {
    /// Remote endpoint in `host:port` form.
    pub endpoint: String,
    /// Connection timeout.
    pub connect_timeout: Duration,
    /// Read timeout.
    pub read_timeout: Duration,
    /// Delay before reconnect attempts.
    pub reconnect_delay: Duration,
}

impl TcpClientConfig {
    /// Creates a client config for an endpoint.
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            connect_timeout: Duration::from_secs(5),
            read_timeout: Duration::from_secs(5),
            reconnect_delay: Duration::from_secs(1),
        }
    }
}

/// TCP outstation/server listener configuration.
#[derive(Debug, Clone)]
pub struct TcpServerConfig {
    /// Bind endpoint in `host:port` form.
    pub bind: String,
    /// Per-connection read timeout.
    pub read_timeout: Duration,
}

impl TcpServerConfig {
    /// Creates a server config for a bind endpoint.
    pub fn new(bind: impl Into<String>) -> Self {
        Self {
            bind: bind.into(),
            read_timeout: Duration::from_secs(5),
        }
    }
}

/// A single TCP connection carrying link-layer frames.
#[derive(Debug)]
pub struct TcpConnection {
    stream: TcpStream,
    read_timeout: Duration,
}

impl TcpConnection {
    /// Creates a connection from an accepted or connected stream.
    pub fn new(stream: TcpStream, read_timeout: Duration) -> Self {
        Self {
            stream,
            read_timeout,
        }
    }

    /// Reads one complete link frame.
    pub async fn read_frame(&mut self) -> Result<LinkFrame> {
        read_frame(&mut self.stream, self.read_timeout).await
    }

    /// Writes one complete link frame.
    pub async fn write_frame(&mut self, frame: &LinkFrame) -> Result<()> {
        self.stream.write_all(&frame.encode()).await?;
        self.stream.flush().await?;
        Ok(())
    }

    /// Gracefully shuts down the TCP write side.
    pub async fn shutdown(&mut self) -> Result<()> {
        self.stream.shutdown().await?;
        Ok(())
    }
}

/// Reconnecting TCP client transport.
#[derive(Debug)]
pub struct TcpClient {
    config: TcpClientConfig,
    connection: Option<TcpConnection>,
    shutdown: bool,
}

impl TcpClient {
    /// Creates a new disconnected TCP client.
    pub fn new(config: TcpClientConfig) -> Self {
        Self {
            config,
            connection: None,
            shutdown: false,
        }
    }

    /// Connects if disconnected.
    pub async fn connect(&mut self) -> Result<()> {
        if self.shutdown {
            return Err(TcpTransportError::Shutdown);
        }
        if self.connection.is_some() {
            return Ok(());
        }
        let stream = timeout(
            self.config.connect_timeout,
            TcpStream::connect(&self.config.endpoint),
        )
        .await
        .map_err(|_| TcpTransportError::Timeout)??;
        self.connection = Some(TcpConnection::new(stream, self.config.read_timeout));
        Ok(())
    }

    /// Drops the current connection without opening a new one.
    pub fn disconnect(&mut self) {
        self.connection = None;
    }

    /// Drops the current connection, waits, and connects again.
    pub async fn reconnect(&mut self) -> Result<()> {
        self.disconnect();
        sleep(self.config.reconnect_delay).await;
        self.connect().await
    }

    /// Sends one frame, reconnecting once after a write failure.
    pub async fn send_frame(&mut self, frame: &LinkFrame) -> Result<()> {
        self.connect().await?;
        let first = self
            .connection
            .as_mut()
            .ok_or(TcpTransportError::Shutdown)?
            .write_frame(frame)
            .await;
        if first.is_ok() {
            return first;
        }
        self.reconnect().await?;
        self.connection
            .as_mut()
            .ok_or(TcpTransportError::Shutdown)?
            .write_frame(frame)
            .await
    }

    /// Receives one frame, reconnecting once after a read failure.
    pub async fn receive_frame(&mut self) -> Result<LinkFrame> {
        self.connect().await?;
        let first = self
            .connection
            .as_mut()
            .ok_or(TcpTransportError::Shutdown)?
            .read_frame()
            .await;
        match first {
            Ok(frame) => Ok(frame),
            Err(TcpTransportError::Timeout) => Err(TcpTransportError::Timeout),
            Err(_) => {
                self.reconnect().await?;
                self.connection
                    .as_mut()
                    .ok_or(TcpTransportError::Shutdown)?
                    .read_frame()
                    .await
            }
        }
    }

    /// Gracefully shuts down the current connection and prevents future reconnects.
    pub async fn shutdown(&mut self) -> Result<()> {
        self.shutdown = true;
        if let Some(connection) = &mut self.connection {
            connection.shutdown().await?;
        }
        self.connection = None;
        Ok(())
    }
}

/// TCP server listener for accepting outstation connections.
#[derive(Debug)]
pub struct TcpServer {
    listener: TcpListener,
    read_timeout: Duration,
}

impl TcpServer {
    /// Binds a listener.
    pub async fn bind(config: TcpServerConfig) -> Result<Self> {
        let listener = TcpListener::bind(&config.bind).await?;
        Ok(Self {
            listener,
            read_timeout: config.read_timeout,
        })
    }

    /// Returns the listener's local address.
    pub fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }

    /// Accepts one TCP connection.
    pub async fn accept(&self) -> Result<TcpConnection> {
        let (stream, _) = self.listener.accept().await?;
        Ok(TcpConnection::new(stream, self.read_timeout))
    }
}

async fn read_frame(stream: &mut TcpStream, read_timeout: Duration) -> Result<LinkFrame> {
    let mut fixed = [0_u8; 10];
    timeout(read_timeout, stream.read_exact(&mut fixed))
        .await
        .map_err(|_| TcpTransportError::Timeout)??;

    if fixed[..2] != START_BYTES {
        return Err(ProtocolError::InvalidStartBytes.into());
    }
    let length = usize::from(fixed[2]);
    if length < 5 {
        return Err(ProtocolError::InvalidLength {
            expected: 5,
            actual: length,
        }
        .into());
    }
    let payload_len = length - 5;
    let rest_len = encoded_payload_len(payload_len);
    let mut rest = vec![0_u8; rest_len];
    if rest_len != 0 {
        timeout(read_timeout, stream.read_exact(&mut rest))
            .await
            .map_err(|_| TcpTransportError::Timeout)??;
    }

    let mut frame = fixed.to_vec();
    frame.extend_from_slice(&rest);
    Ok(LinkFrame::decode(&frame)?)
}

#[allow(dead_code)]
fn _protocol_result<T>(value: ProtocolResult<T>) -> ProtocolResult<T> {
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use dnp3_core::Address;
    use dnp3_link::LinkFrame;

    #[tokio::test]
    async fn tcp_localhost_round_trip() {
        let server = TcpServer::bind(TcpServerConfig {
            bind: "127.0.0.1:0".into(),
            read_timeout: Duration::from_secs(1),
        })
        .await
        .unwrap();
        let addr = server.local_addr().unwrap();

        let server_task = tokio::spawn(async move {
            let mut connection = server.accept().await.unwrap();
            let frame = connection.read_frame().await.unwrap();
            connection.write_frame(&frame).await.unwrap();
        });

        let mut client = TcpClient::new(TcpClientConfig {
            endpoint: addr.to_string(),
            connect_timeout: Duration::from_secs(1),
            read_timeout: Duration::from_secs(1),
            reconnect_delay: Duration::from_millis(10),
        });
        let frame =
            LinkFrame::unconfirmed_user_data(Address::new(1), Address::new(2), vec![1, 2, 3])
                .unwrap();
        client.send_frame(&frame).await.unwrap();
        assert_eq!(client.receive_frame().await.unwrap(), frame);
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn timeout_behavior() {
        let server = TcpServer::bind(TcpServerConfig {
            bind: "127.0.0.1:0".into(),
            read_timeout: Duration::from_millis(50),
        })
        .await
        .unwrap();
        let addr = server.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let _connection = server.accept().await.unwrap();
            sleep(Duration::from_millis(200)).await;
        });

        let mut client = TcpClient::new(TcpClientConfig {
            endpoint: addr.to_string(),
            connect_timeout: Duration::from_secs(1),
            read_timeout: Duration::from_millis(50),
            reconnect_delay: Duration::from_millis(10),
        });
        client.connect().await.unwrap();
        assert!(matches!(
            client.receive_frame().await,
            Err(TcpTransportError::Timeout)
        ));
        server_task.await.unwrap();
    }
}
