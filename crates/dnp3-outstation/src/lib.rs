//! Outstation/server role helpers.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use dnp3_app::{
    AnalogInput, AnalogOutput, AppControl, AppObject, ApplicationFragment, BinaryInput,
    BinaryOutput, Counter, FunctionCode,
};
use dnp3_core::{Address, ProtocolError};
use dnp3_link::{Direction, LinkFrame, LinkFunction, PrimaryFunction};
use dnp3_transport::{fragment, Reassembler, TransportSegment};
use thiserror::Error;

/// Result type for outstation operations.
pub type Result<T> = std::result::Result<T, OutstationError>;

/// Errors returned by the outstation role.
#[derive(Debug, Error)]
pub enum OutstationError {
    /// Protocol validation failed.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// Transport I/O failed.
    #[error("transport error: {0}")]
    Transport(String),
}

/// Abstract frame channel used by the outstation role.
#[async_trait]
pub trait OutstationChannel {
    /// Sends one link frame.
    async fn send_frame(&mut self, frame: LinkFrame) -> Result<()>;

    /// Receives one link frame.
    async fn receive_frame(&mut self) -> Result<LinkFrame>;
}

/// Outstation endpoint configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutstationConfig {
    /// Local outstation address.
    pub outstation: Address,
}

impl Default for OutstationConfig {
    fn default() -> Self {
        Self {
            outstation: Address::new(1024),
        }
    }
}

/// In-memory outstation data set for examples and tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Database {
    /// Binary inputs.
    pub binary_inputs: Vec<BinaryInput>,
    /// Binary outputs.
    pub binary_outputs: Vec<BinaryOutput>,
    /// Analog inputs.
    pub analog_inputs: Vec<AnalogInput>,
    /// Analog outputs.
    pub analog_outputs: Vec<AnalogOutput>,
    /// Counters.
    pub counters: Vec<Counter>,
}

impl Database {
    /// Creates response objects for an integrity poll.
    pub fn integrity_objects(&self) -> Vec<AppObject> {
        let mut objects = Vec::new();
        if !self.binary_inputs.is_empty() {
            objects.push(AppObject::BinaryInputs(self.binary_inputs.clone()));
        }
        if !self.binary_outputs.is_empty() {
            objects.push(AppObject::BinaryOutputs(self.binary_outputs.clone()));
        }
        if !self.analog_inputs.is_empty() {
            objects.push(AppObject::AnalogInputs(self.analog_inputs.clone()));
        }
        if !self.analog_outputs.is_empty() {
            objects.push(AppObject::AnalogOutputs(self.analog_outputs.clone()));
        }
        if !self.counters.is_empty() {
            objects.push(AppObject::Counters(self.counters.clone()));
        }
        objects
    }
}

/// Outstation/server session state.
#[derive(Debug)]
pub struct OutstationSession {
    config: OutstationConfig,
    database: Arc<RwLock<Database>>,
    transport_sequence: u8,
    reassembler: Reassembler,
}

impl OutstationSession {
    /// Creates a new outstation session.
    pub fn new(config: OutstationConfig, database: Database) -> Self {
        Self::with_shared_database(config, Arc::new(RwLock::new(database)))
    }

    /// Creates a session that shares a database with other sessions or tasks.
    pub fn with_shared_database(
        config: OutstationConfig,
        database: Arc<RwLock<Database>>,
    ) -> Self {
        Self {
            config,
            database,
            transport_sequence: 0,
            reassembler: Reassembler::new(),
        }
    }

    /// Returns the local outstation address.
    pub fn outstation_address(&self) -> Address {
        self.config.outstation
    }

    /// Returns a shared handle to the backing database.
    pub fn shared_database(&self) -> Arc<RwLock<Database>> {
        Arc::clone(&self.database)
    }

    /// Returns a read lock on the backing database.
    pub fn database(&self) -> std::sync::RwLockReadGuard<'_, Database> {
        self.database
            .read()
            .expect("outstation database lock poisoned")
    }

    /// Returns a write lock on the backing database.
    pub fn database_mut(&self) -> std::sync::RwLockWriteGuard<'_, Database> {
        self.database
            .write()
            .expect("outstation database lock poisoned")
    }

    /// Handles one incoming frame and returns response frames, if any.
    pub fn handle_frame(&mut self, frame: LinkFrame) -> Result<Vec<LinkFrame>> {
        if frame.destination != self.config.outstation {
            return Ok(Vec::new());
        }

        match frame.control.function {
            LinkFunction::Primary(PrimaryFunction::ResetLinkStates) => {
                self.reassembler.reset();
                self.transport_sequence = 0;
                // Wire byte 0xC0 decodes as outstation-to-master reset; reply with legacy ACK 0x00.
                let ack = if frame.control.direction == Direction::OutstationToMaster {
                    LinkFrame::legacy_ack(frame.source, self.config.outstation)
                } else {
                    LinkFrame::ack(frame.source, self.config.outstation)
                };
                Ok(vec![ack])
            }
            LinkFunction::Primary(PrimaryFunction::TestLinkStates) => Ok(vec![
                LinkFrame::legacy_ack_with_fcb(
                    frame.source,
                    self.config.outstation,
                    frame.control.fcb,
                ),
            ]),
            LinkFunction::Primary(PrimaryFunction::RequestLinkStatus) => {
                Ok(vec![LinkFrame::link_status(
                    frame.source,
                    self.config.outstation,
                )])
            }
            LinkFunction::Primary(PrimaryFunction::UnconfirmedUserData) => {
                self.handle_user_data(frame)
            }
            LinkFunction::Primary(PrimaryFunction::ConfirmedUserData) => {
                let mut responses = vec![LinkFrame::ack_with_fcb(
                    frame.source,
                    self.config.outstation,
                    frame.control.fcb,
                )];
                responses.extend(self.handle_user_data(frame)?);
                Ok(responses)
            }
            _ => Ok(Vec::new()),
        }
    }

    fn handle_user_data(&mut self, frame: LinkFrame) -> Result<Vec<LinkFrame>> {
        // Freyr-style clients set DIR=1 on primary frames (0xC0 reset, 0xC4 user data).
        let tool_style = frame.control.direction == Direction::OutstationToMaster;
        let segment = TransportSegment::decode(&frame.payload)?;
        let Some(application_bytes) = self.reassembler.push(segment)? else {
            return Ok(Vec::new());
        };
        let request = ApplicationFragment::decode(&application_bytes)?;
        let response = self.handle_application(request)?;
        self.encode_response(frame.source, response, tool_style)
    }

    /// Receives and processes one complete request from a channel.
    pub async fn run_once<C>(&mut self, channel: &mut C) -> Result<()>
    where
        C: OutstationChannel + Send,
    {
        loop {
            let frame = channel.receive_frame().await?;
            let responses = self.handle_frame(frame)?;
            if responses.is_empty() {
                continue;
            }
            for response in responses {
                channel.send_frame(response).await?;
            }
            return Ok(());
        }
    }

    fn handle_application(&self, request: ApplicationFragment) -> Result<ApplicationFragment> {
        match request.function {
            FunctionCode::Read => {
                let include_static = request.objects.iter().any(|object| {
                    matches!(
                        object,
                        AppObject::IntegrityPoll | AppObject::ClassScan(_)
                    )
                });
                let objects = if include_static {
                    self.database()
                        .integrity_objects()
                } else {
                    Vec::new()
                };
                Ok(ApplicationFragment::new(
                    AppControl::new(true, true, false, false, request.control.sequence),
                    FunctionCode::Response,
                    objects,
                ))
            }
            FunctionCode::RecordCurrentTime | FunctionCode::Write => Ok(ApplicationFragment::new(
                AppControl::single(request.control.sequence),
                FunctionCode::Response,
                Vec::new(),
            )),
            other => Err(ProtocolError::UnknownValue {
                field: "unsupported outstation function",
                value: other.encode(),
            }
            .into()),
        }
    }

    fn encode_response(
        &mut self,
        destination: Address,
        response: ApplicationFragment,
        tool_style: bool,
    ) -> Result<Vec<LinkFrame>> {
        let encoded = response.encode()?;
        let segments = fragment(&encoded, self.transport_sequence);
        self.transport_sequence = (self.transport_sequence + segments.len() as u8) & 0x3f;

        segments
            .into_iter()
            .map(|segment| {
                let payload = segment.encode();
                let frame = if tool_style {
                    LinkFrame::unconfirmed_user_data(
                        destination,
                        self.config.outstation,
                        payload,
                    )?
                } else {
                    LinkFrame::outstation_unconfirmed_user_data(
                        destination,
                        self.config.outstation,
                        payload,
                    )?
                };
                Ok(frame)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dnp3_master::{MasterChannel, MasterConfig, MasterSession};

    #[derive(Default)]
    struct Loopback {
        inbound: Vec<LinkFrame>,
        outbound: Vec<LinkFrame>,
    }

    #[async_trait]
    impl OutstationChannel for Loopback {
        async fn send_frame(&mut self, frame: LinkFrame) -> Result<()> {
            self.outbound.push(frame);
            Ok(())
        }

        async fn receive_frame(&mut self) -> Result<LinkFrame> {
            self.inbound
                .pop()
                .ok_or(OutstationError::Transport("empty".into()))
        }
    }

    #[async_trait]
    impl MasterChannel for Loopback {
        async fn send_frame(&mut self, frame: LinkFrame) -> dnp3_master::Result<()> {
            self.inbound.push(frame);
            Ok(())
        }

        async fn receive_frame(&mut self) -> dnp3_master::Result<LinkFrame> {
            self.outbound
                .pop()
                .ok_or(dnp3_master::MasterError::Transport("empty".into()))
        }
    }

    #[test]
    fn reset_link_states_returns_standard_ack() {
        let mut outstation = OutstationSession::new(OutstationConfig::default(), Database::default());
        let frame = LinkFrame::reset_link_states(Address::new(1024), Address::new(1));
        let responses = outstation.handle_frame(frame).unwrap();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].control.encode(), 0x80);
        assert!(matches!(
            responses[0].control.function,
            LinkFunction::Secondary(dnp3_link::SecondaryFunction::Ack)
        ));
    }

    #[test]
    fn tool_reset_control_c0_returns_legacy_ack() {
        let mut outstation = OutstationSession::new(
            OutstationConfig {
                outstation: Address::new(10),
            },
            Database::default(),
        );
        let mut bytes = LinkFrame::reset_link_states(Address::new(10), Address::new(1)).encode();
        bytes[3] = 0xc0;
        let crc = dnp3_core::crc16(&bytes[..8]).to_le_bytes();
        bytes[8..10].copy_from_slice(&crc);
        let frame = LinkFrame::decode(&bytes).unwrap();
        let responses = outstation.handle_frame(frame).unwrap();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].control.encode(), 0x00);
        assert_eq!(
            responses[0].encode(),
            LinkFrame::legacy_ack(Address::new(1), Address::new(10)).encode()
        );
    }

    #[test]
    fn client_class_scan_request_returns_single_integrity_set() {
        let mut outstation = OutstationSession::new(
            OutstationConfig {
                outstation: Address::new(10),
            },
            Database {
                binary_inputs: vec![BinaryInput {
                    index: 0,
                    flags: 0x81,
                }],
                binary_outputs: vec![BinaryOutput {
                    index: 0,
                    flags: 0x81,
                }],
                analog_inputs: vec![AnalogInput {
                    index: 0,
                    flags: 0x81,
                    value: 1234,
                }],
                analog_outputs: vec![AnalogOutput {
                    index: 0,
                    flags: 0x81,
                    value: 5678,
                }],
                counters: vec![Counter {
                    index: 0,
                    flags: 0x81,
                    value: 42,
                }],
            },
        );

        let request = ApplicationFragment::decode(&[
            0xc0, 0x01, 0x3c, 0x01, 0x06, 0x3c, 0x02, 0x06, 0x3c, 0x03, 0x06, 0x3c, 0x04, 0x06,
        ])
        .unwrap();
        let frame = LinkFrame::unconfirmed_user_data(
            Address::new(10),
            Address::new(1),
            TransportSegment {
                header: dnp3_transport::TransportHeader::new(true, true, 0),
                payload: request.encode().unwrap(),
            }
            .encode(),
        )
        .unwrap();

        let responses = outstation.handle_frame(frame).unwrap();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].control.encode(), 0xC4);

        let segment = TransportSegment::decode(&responses[0].payload).unwrap();
        let response = ApplicationFragment::decode(&segment.payload).unwrap();
        assert_eq!(response.function, FunctionCode::Response);
        assert_eq!(response.iin, 0);
        assert_eq!(response.objects.len(), 5);
        let encoded = response.encode().unwrap();
        assert_eq!(&encoded[..4], &[0xc0, 0x81, 0x00, 0x00]);
        // Range16 encoding: index in header, flags-only body (avoids client misreading 0x8100).
        assert_eq!(&encoded[4..12], &[0x01, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x81]);
    }

    #[test]
    fn tool_style_class_scan_request_returns_0x44_response() {
        let mut outstation = OutstationSession::new(
            OutstationConfig {
                outstation: Address::new(10),
            },
            Database::default(),
        );

        let request = ApplicationFragment::decode(&[
            0xc0, 0x01, 0x3c, 0x01, 0x06, 0x3c, 0x02, 0x06, 0x3c, 0x03, 0x06, 0x3c, 0x04, 0x06,
        ])
        .unwrap();
        let mut bytes = LinkFrame::unconfirmed_user_data(
            Address::new(10),
            Address::new(1),
            TransportSegment {
                header: dnp3_transport::TransportHeader::new(true, true, 0),
                payload: request.encode().unwrap(),
            }
            .encode(),
        )
        .unwrap()
        .encode();
        bytes[3] = 0xc4;
        let crc = dnp3_core::crc16(&bytes[..8]).to_le_bytes();
        bytes[8..10].copy_from_slice(&crc);
        let frame = LinkFrame::decode(&bytes).unwrap();

        let responses = outstation.handle_frame(frame).unwrap();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].control.encode(), 0x44);
    }

    #[tokio::test]
    async fn integrity_poll_response() {
        let database = Database {
            binary_inputs: vec![BinaryInput {
                index: 0,
                flags: 0x81,
            }],
            binary_outputs: vec![],
            analog_inputs: vec![],
            analog_outputs: vec![],
            counters: vec![],
        };
        let mut outstation = OutstationSession::new(OutstationConfig::default(), database);
        let mut master = MasterSession::new(MasterConfig::default());
        let mut channel = Loopback::default();

        master.send_integrity_poll(&mut channel).await.unwrap();
        outstation.run_once(&mut channel).await.unwrap();
        let response = master.receive_application(&mut channel).await.unwrap();
        assert_eq!(response.function, FunctionCode::Response);
        assert_eq!(response.objects.len(), 1);
    }
}
