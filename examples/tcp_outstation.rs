use std::sync::{Arc, RwLock};
use std::time::Duration;

use clap::Parser;
use dnp3_core::Address;
use dnp3_outstation::{OutstationConfig, OutstationSession};
use dnp3_tcp::{TcpServer, TcpServerConfig};

#[path = "common/outstation_shared.rs"]
mod shared;

use shared::{
    database_from_points, describe_frame, hex_line, parse_indexed_bool, parse_indexed_i32,
    parse_indexed_u32, print_outstation_startup, start_interactive_cli_if_enabled, PointConfig,
};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:20000")]
    bind: String,
    #[arg(long, default_value_t = 1)]
    master: u16,
    #[arg(long, default_value_t = 10)]
    outstation: u16,
    #[arg(
        long,
        default_value_t = 300_000,
        help = "Per-read idle timeout in ms; keep high for simulator clients"
    )]
    timeout_ms: u64,
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let database = Arc::new(RwLock::new(database_from_points(&PointConfig {
        binary_inputs: args.binary_input,
        binary_outputs: args.binary_output,
        analog_inputs: args.analog_input,
        analog_outputs: args.analog_output,
        counters: args.counter,
    })));
    start_interactive_cli_if_enabled(args.no_interactive, &database);
    let server = TcpServer::bind(TcpServerConfig {
        bind: args.bind,
        read_timeout: Duration::from_millis(args.timeout_ms),
    })
    .await?;
    println!("listening on {}", server.local_addr()?);
    print_outstation_startup(
        args.outstation,
        args.master,
        &database.read().expect("database lock poisoned"),
    );
    let outstation_config = OutstationConfig {
        outstation: Address::new(args.outstation),
    };

    loop {
        let mut connection = server.accept().await?;
        println!("accepted TCP connection");
        let database = Arc::clone(&database);
        let verbose = args.verbose;
        tokio::spawn(async move {
            let mut outstation =
                OutstationSession::with_shared_database(outstation_config, database);
            loop {
                let frame = match connection.read_frame().await {
                    Ok(frame) => frame,
                    Err(err) => {
                        eprintln!("connection ended: {err}");
                        break;
                    }
                };
                if verbose {
                    let rx_bytes = frame.encode();
                    println!(
                        "rx: {} | {}",
                        describe_frame(&frame),
                        hex_line(&rx_bytes)
                    );
                }
                match outstation.handle_frame(frame) {
                    Ok(responses) => {
                        for response in responses {
                            if verbose {
                                let tx_bytes = response.encode();
                                println!(
                                    "tx: {} | {}",
                                    describe_frame(&response),
                                    hex_line(&tx_bytes)
                                );
                            }
                            if let Err(err) = connection.write_frame(&response).await {
                                eprintln!("write failed: {err}");
                                break;
                            }
                        }
                    }
                    Err(err) => eprintln!("protocol error: {err}"),
                }
            }
        });
    }
}
