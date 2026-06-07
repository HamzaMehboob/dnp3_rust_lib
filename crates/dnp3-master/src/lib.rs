//! Master/client role helpers.

use async_trait::async_trait;
use dnp3_app::{ApplicationFragment, FunctionCode};
use dnp3_core::{Address, ProtocolError};
use dnp3_link::LinkFrame;
use dnp3_transport::{fragment, Reassembler, TransportSegment};
use thiserror::Error;

/// Result type for master operations.
pub type Result<T> = std::result::Result<T, MasterError>;

/// Errors returned by the master role.
#[derive(Debug, Error)]
pub enum MasterError {
    /// Protocol validation failed.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// Transport I/O failed.
    #[error("transport error: {0}")]
    Transport(String),
}

/// Abstract frame channel used by the master role.
#[async_trait]
pub trait MasterChannel {
    /// Sends one link frame.
    async fn send_frame(&mut self, frame: LinkFrame) -> Result<()>;

    /// Receives one link frame.
    async fn receive_frame(&mut self) -> Result<LinkFrame>;
}

/// Link-layer dialect used by the master.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkStyle {
    /// IEEE-style controls (0x40 reset, 0x44 user data).
    Standard,
    /// Windows tool-style controls (0xC0 reset, 0xC4 user data).
    #[default]
    Tool,
}

/// Master endpoint configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MasterConfig {
    /// Local master address.
    pub master: Address,
    /// Remote outstation address.
    pub outstation: Address,
    /// Link-layer frame dialect.
    pub link_style: LinkStyle,
}

impl Default for MasterConfig {
    fn default() -> Self {
        Self {
            master: Address::new(1),
            outstation: Address::new(1024),
            link_style: LinkStyle::default(),
        }
    }
}

/// Master/client session state.
#[derive(Debug)]
pub struct MasterSession {
    config: MasterConfig,
    transport_sequence: u8,
    app_sequence: u8,
    polls_sent: u64,
    reassembler: Reassembler,
}

impl MasterSession {
    /// Creates a new master session.
    pub fn new(config: MasterConfig) -> Self {
        Self {
            config,
            transport_sequence: 0,
            app_sequence: 0,
            polls_sent: 0,
            reassembler: Reassembler::new(),
        }
    }

    /// Returns the configured master address.
    pub fn master_address(&self) -> Address {
        self.config.master
    }

    /// Returns the configured outstation address.
    pub fn outstation_address(&self) -> Address {
        self.config.outstation
    }

    /// Sends an integrity poll.
    pub async fn send_integrity_poll<C>(&mut self, channel: &mut C) -> Result<()>
    where
        C: MasterChannel + Send,
    {
        let sequence = self.next_app_sequence();
        let request = match self.config.link_style {
            LinkStyle::Tool => ApplicationFragment::simulator_class_scan(sequence),
            LinkStyle::Standard => ApplicationFragment::integrity_poll(sequence),
        };
        self.polls_sent += 1;
        self.send_application(channel, request).await
    }

    /// Sends any application fragment.
    pub async fn send_application<C>(
        &mut self,
        channel: &mut C,
        fragment_to_send: ApplicationFragment,
    ) -> Result<()>
    where
        C: MasterChannel + Send,
    {
        let encoded = fragment_to_send.encode()?;
        let segments = fragment(&encoded, self.transport_sequence);
        self.transport_sequence = (self.transport_sequence + segments.len() as u8) & 0x3f;

        for segment in segments {
            let frame = match self.config.link_style {
                LinkStyle::Standard => LinkFrame::unconfirmed_user_data(
                    self.config.outstation,
                    self.config.master,
                    segment.encode(),
                )?,
                LinkStyle::Tool => LinkFrame::tool_unconfirmed_user_data(
                    self.config.outstation,
                    self.config.master,
                    segment.encode(),
                )?,
            };
            channel.send_frame(frame).await?;
        }
        Ok(())
    }

    /// Clears transport reassembly state after link loss or parse errors.
    pub fn reset_transport(&mut self) {
        self.reassembler.reset();
    }

    /// Receives the next complete application fragment.
    pub async fn receive_application<C>(&mut self, channel: &mut C) -> Result<ApplicationFragment>
    where
        C: MasterChannel + Send,
    {
        loop {
            let frame = channel.receive_frame().await?;
            if frame.destination != self.config.master {
                continue;
            }
            let segment = TransportSegment::decode(&frame.payload)?;
            match self.reassembler.push(segment) {
                Ok(Some(bytes)) => return Ok(ApplicationFragment::decode(&bytes)?),
                Ok(None) => continue,
                Err(ProtocolError::InvalidSequence { .. } | ProtocolError::InvalidFragment) => {
                    self.reassembler.reset();
                    continue;
                }
                Err(err) => return Err(err.into()),
            }
        }
    }

    /// Sends an integrity poll and waits for one response fragment.
    pub async fn integrity_poll<C>(&mut self, channel: &mut C) -> Result<ApplicationFragment>
    where
        C: MasterChannel + Send,
    {
        self.reset_transport();
        self.send_integrity_poll(channel).await?;
        loop {
            let response = self.receive_application(channel).await?;
            match response.function {
                FunctionCode::Response | FunctionCode::UnsolicitedResponse => return Ok(response),
                _ => self.reset_transport(),
            }
        }
    }

    fn next_app_sequence(&mut self) -> u8 {
        let current = self.app_sequence;
        self.app_sequence = (self.app_sequence + 1) & 0x0f;
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemoryChannel {
        sent: Vec<LinkFrame>,
    }

    #[async_trait]
    impl MasterChannel for MemoryChannel {
        async fn send_frame(&mut self, frame: LinkFrame) -> Result<()> {
            self.sent.push(frame);
            Ok(())
        }

        async fn receive_frame(&mut self) -> Result<LinkFrame> {
            self.sent
                .pop()
                .ok_or(MasterError::Transport("empty".into()))
        }
    }

    #[tokio::test]
    async fn sends_integrity_poll() {
        let mut master = MasterSession::new(MasterConfig::default());
        let mut channel = MemoryChannel::default();
        master.send_integrity_poll(&mut channel).await.unwrap();
        assert_eq!(channel.sent.len(), 1);
        assert_eq!(channel.sent[0].destination, Address::new(1024));
        assert_eq!(channel.sent[0].control.encode(), 0xC4);
    }
}
