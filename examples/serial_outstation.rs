use std::sync::{Arc, RwLock};
use std::time::Duration;

use clap::{Parser, ValueEnum};
use dnp3_core::Address;
use dnp3_outstation::{OutstationConfig, OutstationSession};
use dnp3_serial::{DataBits, Parity, SerialConfig, SerialTransport, StopBits};

#[path = "common/outstation_shared.rs"]
mod shared;

use shared::{
    database_from_points, describe_frame, hex_line, parse_indexed_bool, parse_indexed_i32,
    parse_indexed_u32, print_outstation_startup, start_interactive_cli_if_enabled, PointConfig,
};

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
    #[arg(
        long,
        default_value_t = 300_000,
        help = "Per-read idle timeout in ms; keep high for simulator clients"
    )]
    timeout_ms: u64,
    #[arg(long, default_value_t = 1)]
    master: u16,
    #[arg(long, default_value_t = 10)]
    outstation: u16,
    #[arg(
        long,
        value_parser = parse_indexed_bool,
        help = "Binary input as INDEX=VALUE or INDEX VALUE (0/off, 1/on); repeatable"
    )]
    binary_input: Vec<shared::IndexedBool>,
    #[arg(
        long,
        value_parser = parse_indexed_bool,
        help = "Binary output as INDEX=VALUE or INDEX VALUE (0/off, 1/on); repeatable"
    )]
    binary_output: Vec<shared::IndexedBool>,
    #[arg(
        long,
        value_parser = parse_indexed_i32,
        help = "Analog input as INDEX=VALUE or INDEX VALUE; repeatable"
    )]
    analog_input: Vec<shared::IndexedI32>,
    #[arg(
        long,
        value_parser = parse_indexed_i32,
        help = "Analog output as INDEX=VALUE or INDEX VALUE; repeatable"
    )]
    analog_output: Vec<shared::IndexedI32>,
    #[arg(
        long,
        value_parser = parse_indexed_u32,
        help = "Counter as INDEX=VALUE or INDEX VALUE; repeatable"
    )]
    counter: Vec<shared::IndexedU32>,
    #[arg(long, help = "Log received and transmitted link frames")]
    verbose: bool,
    #[arg(long, help = "Disable the interactive stdin command thread")]
    no_interactive: bool,
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let database = Arc::new(RwLock::new(database_from_points(&PointConfig {
        binary_inputs: args.binary_input,
        binary_outputs: args.binary_output,
        analog_inputs: args.analog_input,
        analog_outputs: args.analog_output,
        counters: args.counter,
    })));
    start_interactive_cli_if_enabled(args.no_interactive, &database);
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
    let mut serial = SerialTransport::open(&config)?;
    println!(
        "serial listening on {} @ {} baud ({}-{}-{})",
        config.port_name,
        config.baud_rate,
        data_bits_label(args.data_bits),
        parity_label(args.parity),
        stop_bits_label(args.stop_bits),
    );
    print_outstation_startup(
        args.outstation,
        args.master,
        &database.read().expect("database lock poisoned"),
    );
    let mut outstation = OutstationSession::with_shared_database(
        OutstationConfig {
            outstation: Address::new(args.outstation),
        },
        Arc::clone(&database),
    );

    loop {
        let frame = match serial.read_frame() {
            Ok(frame) => frame,
            Err(err) => {
                eprintln!("read failed: {err}");
                continue;
            }
        };
        if args.verbose {
            let rx_bytes = frame.encode();
            println!("rx: {} | {}", describe_frame(&frame), hex_line(&rx_bytes));
        }
        match outstation.handle_frame(frame) {
            Ok(responses) => {
                for response in responses {
                    if args.verbose {
                        let tx_bytes = response.encode();
                        println!(
                            "tx: {} | {}",
                            describe_frame(&response),
                            hex_line(&tx_bytes)
                        );
                    }
                    if let Err(err) = serial.write_frame(&response) {
                        eprintln!("write failed: {err}");
                    }
                }
            }
            Err(err) => eprintln!("protocol error: {err}"),
        }
    }
}

fn data_bits_label(bits: DataBitsArg) -> &'static str {
    match bits {
        DataBitsArg::Five => "5",
        DataBitsArg::Six => "6",
        DataBitsArg::Seven => "7",
        DataBitsArg::Eight => "8",
    }
}

fn parity_label(parity: ParityArg) -> &'static str {
    match parity {
        ParityArg::None => "N",
        ParityArg::Odd => "O",
        ParityArg::Even => "E",
    }
}

fn stop_bits_label(bits: StopBitsArg) -> &'static str {
    match bits {
        StopBitsArg::One => "1",
        StopBitsArg::Two => "2",
    }
}
