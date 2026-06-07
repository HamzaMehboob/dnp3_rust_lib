use std::time::Duration;

use async_trait::async_trait;
use dnp3_app::{
    AnalogInput, AnalogOutput, BinaryInput, BinaryOutput, Counter, FunctionCode,
};
use dnp3_link::LinkFrame;
use dnp3_master::{MasterChannel, MasterConfig, MasterError, MasterSession};
use dnp3_outstation::{
    Database, OutstationChannel, OutstationConfig, OutstationError, OutstationSession,
};
use dnp3_serial::MockSerialEndpoint;
use dnp3_tcp::{TcpClient, TcpClientConfig, TcpServer, TcpServerConfig};

fn test_database() -> Database {
    Database {
        binary_inputs: vec![BinaryInput {
            index: 0,
            flags: 0x81,
        }],
        binary_outputs: vec![BinaryOutput {
            index: 0,
            flags: 0x01,
        }],
        analog_inputs: vec![AnalogInput {
            index: 1,
            flags: 0x81,
            value: -12,
        }],
        analog_outputs: vec![AnalogOutput {
            index: 0,
            flags: 0x81,
            value: 100,
        }],
        counters: vec![Counter {
            index: 2,
            flags: 0x81,
            value: 99,
        }],
    }
}

struct TcpMasterChannel<'a> {
    client: &'a mut TcpClient,
}

#[async_trait]
impl MasterChannel for TcpMasterChannel<'_> {
    async fn send_frame(&mut self, frame: LinkFrame) -> dnp3_master::Result<()> {
        self.client
            .send_frame(&frame)
            .await
            .map_err(|err| MasterError::Transport(err.to_string()))
    }

    async fn receive_frame(&mut self) -> dnp3_master::Result<LinkFrame> {
        self.client
            .receive_frame()
            .await
            .map_err(|err| MasterError::Transport(err.to_string()))
    }
}

struct SerialMasterChannel {
    endpoint: MockSerialEndpoint,
}

#[async_trait]
impl MasterChannel for SerialMasterChannel {
    async fn send_frame(&mut self, frame: LinkFrame) -> dnp3_master::Result<()> {
        self.endpoint
            .write_frame(&frame)
            .await
            .map_err(|err| MasterError::Transport(err.to_string()))
    }

    async fn receive_frame(&mut self) -> dnp3_master::Result<LinkFrame> {
        self.endpoint
            .read_frame()
            .await
            .map_err(|err| MasterError::Transport(err.to_string()))
    }
}

struct SerialOutstationChannel {
    endpoint: MockSerialEndpoint,
}

#[async_trait]
impl OutstationChannel for SerialOutstationChannel {
    async fn send_frame(&mut self, frame: LinkFrame) -> dnp3_outstation::Result<()> {
        self.endpoint
            .write_frame(&frame)
            .await
            .map_err(|err| OutstationError::Transport(err.to_string()))
    }

    async fn receive_frame(&mut self) -> dnp3_outstation::Result<LinkFrame> {
        self.endpoint
            .read_frame()
            .await
            .map_err(|err| OutstationError::Transport(err.to_string()))
    }
}

#[tokio::test]
async fn tcp_localhost_master_outstation_integration() {
    let server = TcpServer::bind(TcpServerConfig {
        bind: "127.0.0.1:0".into(),
        read_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();
    let addr = server.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let mut outstation = OutstationSession::new(OutstationConfig::default(), test_database());
        let mut connection = server.accept().await.unwrap();
        loop {
            let frame = connection.read_frame().await.unwrap();
            let responses = outstation.handle_frame(frame).unwrap();
            if responses.is_empty() {
                continue;
            }
            for response in responses {
                connection.write_frame(&response).await.unwrap();
            }
            break;
        }
    });

    let mut client = TcpClient::new(TcpClientConfig {
        endpoint: addr.to_string(),
        connect_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        reconnect_delay: Duration::from_millis(10),
    });
    let mut master = MasterSession::new(MasterConfig::default());
    let mut channel = TcpMasterChannel {
        client: &mut client,
    };
    let response = master.integrity_poll(&mut channel).await.unwrap();
    assert_eq!(response.function, FunctionCode::Response);
    assert_eq!(response.objects.len(), 5);
    server_task.await.unwrap();
}

#[tokio::test]
async fn serial_mock_master_outstation_integration() {
    let (master_endpoint, outstation_endpoint) = MockSerialEndpoint::pair(Duration::from_secs(1));
    let mut master_channel = SerialMasterChannel {
        endpoint: master_endpoint,
    };
    let mut outstation_channel = SerialOutstationChannel {
        endpoint: outstation_endpoint,
    };
    let mut master = MasterSession::new(MasterConfig::default());
    let mut outstation = OutstationSession::new(OutstationConfig::default(), test_database());

    master
        .send_integrity_poll(&mut master_channel)
        .await
        .unwrap();
    outstation.run_once(&mut outstation_channel).await.unwrap();
    let response = master
        .receive_application(&mut master_channel)
        .await
        .unwrap();
    assert_eq!(response.function, FunctionCode::Response);
    assert_eq!(response.objects.len(), 5);
}

#[tokio::test]
async fn explicit_tcp_reconnect() {
    let server = TcpServer::bind(TcpServerConfig {
        bind: "127.0.0.1:0".into(),
        read_timeout: Duration::from_secs(1),
    })
    .await
    .unwrap();
    let addr = server.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let mut first = server.accept().await.unwrap();
        first.shutdown().await.unwrap();
        let mut second = server.accept().await.unwrap();
        second.shutdown().await.unwrap();
    });

    let mut client = TcpClient::new(TcpClientConfig {
        endpoint: addr.to_string(),
        connect_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        reconnect_delay: Duration::from_millis(10),
    });
    client.connect().await.unwrap();
    client.reconnect().await.unwrap();
    client.shutdown().await.unwrap();
    server_task.await.unwrap();
}
