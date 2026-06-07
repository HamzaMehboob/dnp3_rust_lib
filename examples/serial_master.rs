use std::time::Duration;

use async_trait::async_trait;
use clap::{Parser, ValueEnum};
use dnp3_core::Address;
use dnp3_link::LinkFrame;
use dnp3_master::{MasterChannel, MasterError};
use dnp3_serial::{DataBits, Parity, SerialConfig, SerialTransport, StopBits};

#[path = "common/master_shared.rs"]
mod shared;

use shared::{run_link_startup, run_poll_loop};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    port: String,
    #[arg(long, default_value_t = 9600)]
    baud_rate: u32,
    #[arg(long, value_enum, default_value_t = DataBitsArg::Eight)]
    data_bits: DataBitsArg,
    #[arg(long, value_enum, default_value_t = ParityArg::None)]
    parity: ParityArg,
    #[arg(long, value_enum, default_value_t = StopBitsArg::One)]
    stop_bits: StopBitsArg,
    #[arg(long, default_value_t = 1000)]
    timeout_ms: u64,
    #[arg(
        long,
        default_value_t = 1,
        help = "Local master address (must match outstation)"
    )]
    master: u16,
    #[arg(
        long,
        default_value_t = 10,
        help = "Remote outstation address (must match outstation)"
    )]
    outstation: u16,
    #[arg(
        long,
        default_value_t = 2000,
        help = "Milliseconds between integrity polls (0 = no delay)"
    )]
    poll_interval_ms: u64,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DataBitsArg {
    Five,
    Six,
    Seven,
    Eight,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ParityArg {
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StopBitsArg {
    One,
    Two,
}

struct SerialMasterChannel {
    serial: SerialTransport,
}

#[async_trait]
impl MasterChannel for SerialMasterChannel {
    async fn send_frame(&mut self, frame: LinkFrame) -> dnp3_master::Result<()> {
        self.serial
            .write_frame(&frame)
            .map_err(|err| MasterError::Transport(err.to_string()))
    }

    async fn receive_frame(&mut self) -> dnp3_master::Result<LinkFrame> {
        self.serial
            .read_frame()
            .map_err(|err| MasterError::Transport(err.to_string()))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let master_addr = Address::new(args.master);
    let outstation_addr = Address::new(args.outstation);
    let config = SerialConfig {
        port_name: args.port,
        baud_rate: args.baud_rate,
        data_bits: match args.data_bits {
            DataBitsArg::Five => DataBits::Five,
            DataBitsArg::Six => DataBits::Six,
            DataBitsArg::Seven => DataBits::Seven,
            DataBitsArg::Eight => DataBits::Eight,
        },
        parity: match args.parity {
            ParityArg::None => Parity::None,
            ParityArg::Odd => Parity::Odd,
            ParityArg::Even => Parity::Even,
        },
        stop_bits: match args.stop_bits {
            StopBitsArg::One => StopBits::One,
            StopBitsArg::Two => StopBits::Two,
        },
        timeout: Duration::from_millis(args.timeout_ms),
    };
    let serial = SerialTransport::open(&config)?;
    println!(
        "master {} polling outstation {} on {} @ {} baud",
        args.master, args.outstation, config.port_name, config.baud_rate
    );
    println!("polling continuously; press Ctrl+C to stop");

    let mut channel = SerialMasterChannel { serial };
    run_link_startup(&mut channel, outstation_addr, master_addr)
        .await
        .map_err(|err| -> Box<dyn std::error::Error> { err.into() })?;

    let poll_interval = Duration::from_millis(args.poll_interval_ms);
    let mut reconnect = || async { Ok(()) };

    run_poll_loop(
        master_addr,
        outstation_addr,
        &mut channel,
        poll_interval,
        &mut reconnect,
    )
    .await
    .map_err(|err| -> Box<dyn std::error::Error> { err.into() })
}
