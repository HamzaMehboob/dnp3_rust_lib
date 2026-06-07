use std::time::Duration;

use async_trait::async_trait;
use clap::Parser;
use dnp3_core::Address;
use dnp3_link::LinkFrame;
use dnp3_master::{MasterChannel, MasterConfig, MasterError, MasterSession};
use dnp3_tcp::{TcpClient, TcpClientConfig};

#[path = "common/master_shared.rs"]
mod shared;

use shared::{print_response, run_link_startup};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:20000", alias = "bind")]
    connect: String,
    #[arg(long, default_value_t = 1, help = "Local master address (must match outstation)")]
    master: u16,
    #[arg(long, default_value_t = 10, help = "Remote outstation address (must match outstation)")]
    outstation: u16,
    #[arg(long, default_value_t = 5000)]
    timeout_ms: u64,
    #[arg(long, default_value_t = 2000, help = "Milliseconds between integrity polls (0 = no delay)")]
    poll_interval_ms: u64,
    #[arg(long, default_value_t = 2000, help = "Milliseconds between connect retries")]
    retry_delay_ms: u64,
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

async fn establish_link(
    client: &mut TcpClient,
    outstation: Address,
    master: Address,
) -> dnp3_master::Result<()> {
    client
        .connect()
        .await
        .map_err(|err| MasterError::Transport(err.to_string()))?;
    let mut channel = TcpMasterChannel { client };
    run_link_startup(&mut channel, outstation, master).await
}

async fn wait_for_link(
    client: &mut TcpClient,
    outstation: Address,
    master: Address,
    retry_delay: Duration,
    label: &str,
) -> dnp3_master::Result<()> {
    loop {
        match establish_link(client, outstation, master).await {
            Ok(()) => {
                println!("{label} ok");
                return Ok(());
            }
            Err(err) => {
                eprintln!("{label} failed: {err}");
                eprintln!(
                    "retrying in {} ms (start: cargo run --example tcp_outstation -- --bind 127.0.0.1:20000)",
                    retry_delay.as_millis()
                );
                tokio::time::sleep(retry_delay).await;
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let master_addr = Address::new(args.master);
    let outstation_addr = Address::new(args.outstation);
    let retry_delay = Duration::from_millis(args.retry_delay_ms);
    let poll_interval = Duration::from_millis(args.poll_interval_ms);

    println!(
        "master {} connecting to outstation {} at {}",
        args.master, args.outstation, args.connect
    );
    println!("polling continuously; press Ctrl+C to stop");

    let mut client = TcpClient::new(TcpClientConfig {
        endpoint: args.connect,
        connect_timeout: Duration::from_millis(args.timeout_ms),
        read_timeout: Duration::from_millis(args.timeout_ms),
        reconnect_delay: Duration::from_secs(1),
    });

    wait_for_link(
        &mut client,
        outstation_addr,
        master_addr,
        retry_delay,
        "link startup",
    )
    .await?;

    let mut master = MasterSession::new(MasterConfig {
        master: master_addr,
        outstation: outstation_addr,
        ..Default::default()
    });
    let mut poll = 0_u64;

    loop {
        poll += 1;
        println!("integrity poll #{poll}");
        let poll_result = {
            let mut channel = TcpMasterChannel {
                client: &mut client,
            };
            master.integrity_poll(&mut channel).await
        };
        match poll_result {
            Ok(response) => print_response(&response),
            Err(err) => {
                eprintln!("poll failed: {err}");
                master.reset_transport();
                client.disconnect();
                wait_for_link(
                    &mut client,
                    outstation_addr,
                    master_addr,
                    retry_delay,
                    "reconnect",
                )
                .await?;
                master = MasterSession::new(MasterConfig {
                    master: master_addr,
                    outstation: outstation_addr,
                    ..Default::default()
                });
            }
        }
        tokio::time::sleep(poll_interval).await;
    }
}
