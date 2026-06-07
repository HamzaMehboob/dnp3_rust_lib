use std::time::Duration;

use dnp3_app::{AppObject, ApplicationFragment};
use dnp3_core::Address;
use dnp3_link::{LinkFrame, LinkFunction, SecondaryFunction};
use dnp3_master::{MasterChannel, MasterConfig, MasterError, MasterSession, Result};

pub fn binary_state(flags: u8) -> &'static str {
    match (flags >> 6) & 0x03 {
        0b10 => "ON",
        0b01 => "OFF",
        0b11 => "INDETERMINATE",
        _ => {
            // Freyr static group-1 often sends 0x01 for BI=0.
            if flags == 0x01 {
                "OFF"
            } else if flags & 0x01 != 0 {
                "ON"
            } else {
                "OFF"
            }
        }
    }
}

pub fn double_bit_state(flags: u8) -> &'static str {
    match (flags >> 6) & 0x03 {
        0b10 => "ON",
        0b01 => "OFF",
        0b11 => "INDETERMINATE",
        _ => "INTERMEDIATE",
    }
}

pub fn binary_quality(flags: u8) -> &'static str {
    if flags & 0x80 != 0 {
        "online"
    } else {
        "-"
    }
}

pub fn print_response(response: &ApplicationFragment) {
    println!("function: {:?}", response.function);
    println!("objects: {}", response.objects.len());

    for object in &response.objects {
        match object {
            AppObject::BinaryInputs(points) if !points.is_empty() => {
                for point in points {
                    println!(
                        "  binary input [{}] state={} quality={} flags=0x{:02x}",
                        point.index,
                        binary_state(point.flags),
                        binary_quality(point.flags),
                        point.flags
                    );
                }
            }
            AppObject::DoubleBitBinaryInputs(points) => {
                for point in points {
                    println!(
                        "  double-bit binary input [{}] state={} flags=0x{:02x}",
                        point.index,
                        double_bit_state(point.flags),
                        point.flags
                    );
                }
            }
            AppObject::BinaryOutputs(points) => {
                for point in points {
                    println!(
                        "  binary output [{}] state={} quality={} flags=0x{:02x}",
                        point.index,
                        binary_state(point.flags),
                        binary_quality(point.flags),
                        point.flags
                    );
                }
            }
            AppObject::AnalogInputs(points) => {
                for point in points {
                    println!(
                        "  analog input [{}] flags=0x{:02x} value={}",
                        point.index, point.flags, point.value
                    );
                }
            }
            AppObject::AnalogOutputs(points) => {
                for point in points {
                    println!(
                        "  analog output [{}] flags=0x{:02x} value={}",
                        point.index, point.flags, point.value
                    );
                }
            }
            AppObject::Counters(points) => {
                for point in points {
                    println!(
                        "  counter [{}] flags=0x{:02x} value={}",
                        point.index, point.flags, point.value
                    );
                }
            }
            other => println!("  {:?}", other),
        }
    }
}

async fn expect_link_ack<C>(channel: &mut C, step: &str) -> Result<()>
where
    C: MasterChannel + Send,
{
    match channel.receive_frame().await {
        Ok(ack) if matches!(
            ack.control.function,
            LinkFunction::Secondary(SecondaryFunction::Ack)
        ) => {
            println!("received link ACK");
            Ok(())
        }
        Ok(ack) => Err(MasterError::Transport(format!(
            "expected link ACK after {step}, got {:?}",
            ack.control.function
        ))),
        Err(MasterError::Transport(message)) if message.contains("timed out") => {
            Err(MasterError::Transport(format!(
                "no link-layer response after {step}; ensure the outstation is running, addresses match (--master/--outstation), and no stale process is holding the port"
            )))
        }
        Err(err) => Err(err),
    }
}

pub async fn run_link_startup<C>(
    channel: &mut C,
    outstation: Address,
    master: Address,
) -> Result<()>
where
    C: MasterChannel + Send,
{
    println!("sending reset link states (tool style 0xC0)");
    channel
        .send_frame(LinkFrame::tool_reset_link_states(outstation, master))
        .await?;
    expect_link_ack(channel, "reset link states").await?;
    println!("sending test link states (tool style 0xD2)");
    channel
        .send_frame(LinkFrame::tool_test_link_states(outstation, master, false))
        .await?;
    expect_link_ack(channel, "test link states").await
}

#[allow(dead_code)]
pub async fn run_poll_loop<C, Reconnect, ReconnectFut>(
    master_addr: Address,
    outstation_addr: Address,
    channel: &mut C,
    poll_interval: Duration,
    reconnect: &mut Reconnect,
) -> Result<()>
where
    C: MasterChannel + Send,
    Reconnect: FnMut() -> ReconnectFut,
    ReconnectFut: std::future::Future<Output = Result<()>>,
{
    let mut master = MasterSession::new(MasterConfig {
        master: master_addr,
        outstation: outstation_addr,
        ..Default::default()
    });
    let mut poll = 0_u64;

    loop {
        poll += 1;
        println!("integrity poll #{poll}");
        match master.integrity_poll(channel).await {
            Ok(response) => print_response(&response),
            Err(err) => {
                eprintln!("poll failed: {err}");
                reconnect().await?;
                run_link_startup(channel, outstation_addr, master_addr).await?;
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
