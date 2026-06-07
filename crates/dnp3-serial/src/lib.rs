//! Serial transport and mock serial endpoints for tests.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dnp3_core::ProtocolError;
use dnp3_link::{encoded_payload_len, LinkFrame, START_BYTES};
pub use serialport::{DataBits, Parity, StopBits};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::time::sleep;

/// Result type for serial transport operations.
pub type Result<T> = std::result::Result<T, SerialTransportError>;

/// Serial transport errors.
#[derive(Debug, Error)]
pub enum SerialTransportError {
    /// Protocol validation failed.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// Serial configuration or driver error.
    #[error(transparent)]
    Serial(#[from] serialport::Error),
    /// I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A timeout elapsed.
    #[error("operation timed out")]
    Timeout,
    /// Mock transport was closed.
    #[error("transport closed")]
    Closed,
}

/// Serial port configuration.
#[derive(Debug, Clone)]
pub struct SerialConfig {
    /// Operating system port name.
    pub port_name: String,
    /// Baud rate.
    pub baud_rate: u32,
    /// Data bits.
    pub data_bits: DataBits,
    /// Parity.
    pub parity: Parity,
    /// Stop bits.
    pub stop_bits: StopBits,
    /// Read/write timeout.
    pub timeout: Duration,
}

impl SerialConfig {
    /// Creates a common 8N1 serial configuration.
    pub fn new(port_name: impl Into<String>, baud_rate: u32) -> Self {
        Self {
            port_name: port_name.into(),
            baud_rate,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            timeout: Duration::from_secs(1),
        }
    }
}

/// Blocking serial transport backed by the operating system serial driver.
pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl SerialTransport {
    /// Opens a serial port.
    pub fn open(config: &SerialConfig) -> Result<Self> {
        let port = serialport::new(&config.port_name, config.baud_rate)
            .data_bits(config.data_bits)
            .parity(config.parity)
            .stop_bits(config.stop_bits)
            .timeout(config.timeout)
            .open()?;
        Ok(Self { port })
    }

    /// Reads one link frame.
    pub fn read_frame(&mut self) -> Result<LinkFrame> {
        read_frame_blocking(&mut self.port)
    }

    /// Writes one link frame.
    pub fn write_frame(&mut self, frame: &LinkFrame) -> Result<()> {
        self.port.write_all(&frame.encode())?;
        self.port.flush()?;
        Ok(())
    }
}

/// A mock serial endpoint for unit and integration tests.
#[derive(Debug, Clone)]
pub struct MockSerialEndpoint {
    inbound: Arc<Mutex<VecDeque<Vec<u8>>>>,
    outbound: Arc<Mutex<VecDeque<Vec<u8>>>>,
    timeout: Duration,
}

impl MockSerialEndpoint {
    /// Creates a connected pair of mock serial endpoints.
    pub fn pair(timeout: Duration) -> (Self, Self) {
        let left_to_right = Arc::new(Mutex::new(VecDeque::new()));
        let right_to_left = Arc::new(Mutex::new(VecDeque::new()));
        (
            Self {
                inbound: right_to_left.clone(),
                outbound: left_to_right.clone(),
                timeout,
            },
            Self {
                inbound: left_to_right,
                outbound: right_to_left,
                timeout,
            },
        )
    }

    /// Sends one frame to the peer endpoint.
    pub async fn write_frame(&self, frame: &LinkFrame) -> Result<()> {
        self.outbound.lock().await.push_back(frame.encode());
        Ok(())
    }

    /// Receives one frame from the peer endpoint.
    pub async fn read_frame(&self) -> Result<LinkFrame> {
        let deadline = Instant::now() + self.timeout;
        loop {
            if let Some(bytes) = self.inbound.lock().await.pop_front() {
                return Ok(LinkFrame::decode(&bytes)?);
            }
            if Instant::now() >= deadline {
                return Err(SerialTransportError::Timeout);
            }
            sleep(Duration::from_millis(5)).await;
        }
    }
}

fn read_frame_blocking<R>(reader: &mut R) -> Result<LinkFrame>
where
    R: Read + ?Sized,
{
    let mut fixed = [0_u8; 10];
    reader.read_exact(&mut fixed)?;
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
    let rest_len = encoded_payload_len(length - 5);
    let mut rest = vec![0_u8; rest_len];
    reader.read_exact(&mut rest)?;
    let mut encoded = fixed.to_vec();
    encoded.extend_from_slice(&rest);
    Ok(LinkFrame::decode(&encoded)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dnp3_core::Address;

    #[tokio::test]
    async fn mock_serial_round_trip() {
        let (left, right) = MockSerialEndpoint::pair(Duration::from_secs(1));
        let frame =
            LinkFrame::unconfirmed_user_data(Address::new(1), Address::new(2), vec![7, 8, 9])
                .unwrap();
        left.write_frame(&frame).await.unwrap();
        assert_eq!(right.read_frame().await.unwrap(), frame);
    }

    #[tokio::test]
    async fn mock_serial_timeout() {
        let (left, _right) = MockSerialEndpoint::pair(Duration::from_millis(20));
        assert!(matches!(
            left.read_frame().await,
            Err(SerialTransportError::Timeout)
        ));
    }
}
