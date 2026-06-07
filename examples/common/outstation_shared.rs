use std::io::{self, BufRead};
use std::sync::{Arc, RwLock};

use dnp3_app::{AnalogInput, AnalogOutput, BinaryInput, BinaryOutput, Counter};
use dnp3_link::{LinkFrame, LinkFunction};
use dnp3_outstation::Database;

#[derive(Debug, Clone)]
pub struct IndexedBool {
    pub index: u16,
    pub value: bool,
}

#[derive(Debug, Clone)]
pub struct IndexedI32 {
    pub index: u16,
    pub value: i32,
}

#[derive(Debug, Clone)]
pub struct IndexedU32 {
    pub index: u16,
    pub value: u32,
}

#[derive(Debug, Clone, Default)]
pub struct PointConfig {
    pub binary_inputs: Vec<IndexedBool>,
    pub binary_outputs: Vec<IndexedBool>,
    pub analog_inputs: Vec<IndexedI32>,
    pub analog_outputs: Vec<IndexedI32>,
    pub counters: Vec<IndexedU32>,
}

fn split_index_value(s: &str) -> Result<(&str, &str), String> {
    if let Some((index, value)) = s.split_once('=') {
        return Ok((index.trim(), value.trim()));
    }
    let mut parts = s.split_whitespace();
    let index = parts.next().ok_or("expected INDEX VALUE or INDEX=VALUE")?;
    let value = parts.next().ok_or("expected INDEX VALUE or INDEX=VALUE")?;
    if parts.next().is_some() {
        return Err("too many values; use INDEX=VALUE or INDEX VALUE".into());
    }
    Ok((index, value))
}

fn parse_index(s: &str) -> Result<u16, String> {
    s.parse::<u16>()
        .map_err(|_| format!("invalid index '{s}'; expected 0-65535"))
}

fn parse_binary_value(s: &str) -> Result<bool, String> {
    match s {
        "0" | "false" | "off" | "OFF" => Ok(false),
        "1" | "true" | "on" | "ON" => Ok(true),
        _ => Err(format!(
            "invalid binary value '{s}'; use 0/1, off/on, or false/true"
        )),
    }
}

pub fn parse_indexed_bool(s: &str) -> Result<IndexedBool, String> {
    let (index, value) = split_index_value(s)?;
    Ok(IndexedBool {
        index: parse_index(index)?,
        value: parse_binary_value(value)?,
    })
}

pub fn parse_indexed_i32(s: &str) -> Result<IndexedI32, String> {
    let (index, value) = split_index_value(s)?;
    Ok(IndexedI32 {
        index: parse_index(index)?,
        value: value
            .parse::<i32>()
            .map_err(|_| format!("invalid analog value '{value}'"))?,
    })
}

pub fn parse_indexed_u32(s: &str) -> Result<IndexedU32, String> {
    let (index, value) = split_index_value(s)?;
    Ok(IndexedU32 {
        index: parse_index(index)?,
        value: value
            .parse::<u32>()
            .map_err(|_| format!("invalid counter value '{value}'"))?,
    })
}

/// DNP3 binary flags: bit 0 = state, bit 7 = online.
/// OFF sends no flags; ON sets both state and online so masters display the value.
pub fn binary_flags(on: bool) -> u8 {
    if on {
        0x81
    } else {
        0x00
    }
}

pub fn analog_flags() -> u8 {
    0x81
}

pub fn on_off(on: bool) -> &'static str {
    if on {
        "ON"
    } else {
        "OFF"
    }
}

pub fn database_from_points(points: &PointConfig) -> Database {
    Database {
        binary_inputs: if points.binary_inputs.is_empty() {
            vec![BinaryInput {
                index: 0,
                flags: binary_flags(true),
            }]
        } else {
            points
                .binary_inputs
                .iter()
                .map(|point| BinaryInput {
                    index: point.index,
                    flags: binary_flags(point.value),
                })
                .collect()
        },
        binary_outputs: if points.binary_outputs.is_empty() {
            vec![BinaryOutput {
                index: 0,
                flags: binary_flags(true),
            }]
        } else {
            points
                .binary_outputs
                .iter()
                .map(|point| BinaryOutput {
                    index: point.index,
                    flags: binary_flags(point.value),
                })
                .collect()
        },
        analog_inputs: if points.analog_inputs.is_empty() {
            vec![AnalogInput {
                index: 0,
                flags: analog_flags(),
                value: 1234,
            }]
        } else {
            points
                .analog_inputs
                .iter()
                .map(|point| AnalogInput {
                    index: point.index,
                    flags: analog_flags(),
                    value: point.value,
                })
                .collect()
        },
        analog_outputs: if points.analog_outputs.is_empty() {
            vec![AnalogOutput {
                index: 0,
                flags: analog_flags(),
                value: 5678,
            }]
        } else {
            points
                .analog_outputs
                .iter()
                .map(|point| AnalogOutput {
                    index: point.index,
                    flags: analog_flags(),
                    value: point.value,
                })
                .collect()
        },
        counters: if points.counters.is_empty() {
            vec![Counter {
                index: 0,
                flags: analog_flags(),
                value: 42,
            }]
        } else {
            points
                .counters
                .iter()
                .map(|point| Counter {
                    index: point.index,
                    flags: analog_flags(),
                    value: point.value,
                })
                .collect()
        },
    }
}

pub fn set_binary_input(database: &mut Database, index: u16, on: bool) {
    upsert(
        &mut database.binary_inputs,
        index,
        BinaryInput {
            index,
            flags: binary_flags(on),
        },
    );
}

pub fn set_binary_output(database: &mut Database, index: u16, on: bool) {
    upsert(
        &mut database.binary_outputs,
        index,
        BinaryOutput {
            index,
            flags: binary_flags(on),
        },
    );
}

pub fn set_analog_input(database: &mut Database, index: u16, value: i32) {
    upsert(
        &mut database.analog_inputs,
        index,
        AnalogInput {
            index,
            flags: analog_flags(),
            value,
        },
    );
}

pub fn set_analog_output(database: &mut Database, index: u16, value: i32) {
    upsert(
        &mut database.analog_outputs,
        index,
        AnalogOutput {
            index,
            flags: analog_flags(),
            value,
        },
    );
}

pub fn set_counter(database: &mut Database, index: u16, value: u32) {
    upsert(
        &mut database.counters,
        index,
        Counter {
            index,
            flags: analog_flags(),
            value,
        },
    );
}

fn upsert<T>(points: &mut Vec<T>, index: u16, point: T)
where
    T: PointIndex,
{
    if let Some(existing) = points.iter_mut().find(|entry| entry.index() == index) {
        *existing = point;
    } else {
        points.push(point);
    }
}

trait PointIndex {
    fn index(&self) -> u16;
}

impl PointIndex for BinaryInput {
    fn index(&self) -> u16 {
        self.index
    }
}

impl PointIndex for BinaryOutput {
    fn index(&self) -> u16 {
        self.index
    }
}

impl PointIndex for AnalogInput {
    fn index(&self) -> u16 {
        self.index
    }
}

impl PointIndex for AnalogOutput {
    fn index(&self) -> u16 {
        self.index
    }
}

impl PointIndex for Counter {
    fn index(&self) -> u16 {
        self.index
    }
}

pub fn print_cli_examples() {
    println!("examples:");
    println!("  bi 0 1      # turn ON");
    println!("  bi 0 0      # turn OFF (empty status)");
    println!("  ai 0 2500   # change analog");
    println!("  show        # verify");
}

pub fn start_interactive_cli_if_enabled(no_interactive: bool, database: &Arc<RwLock<Database>>) {
    if no_interactive {
        return;
    }
    println!("interactive CLI enabled:");
    print_cli_help();
    spawn_interactive_cli(Arc::clone(database));
}

pub fn print_outstation_startup(outstation: u16, master: u16, database: &Database) {
    println!(
        "outstation address {} (expects master {})",
        outstation, master
    );
    println!(
        "simulator/client config: master={master}, outstation={outstation}, points at index 0"
    );
    println!("database:");
    print_database_summary(database);
}

pub fn print_cli_help() {
    println!("interactive commands (INDEX VALUE or INDEX=VALUE):");
    println!("  bi <index> <value>   set binary input (0/1)");
    println!("  bo <index> <value>   set binary output (0/1)");
    println!("  ai <index> <value>   set analog input");
    println!("  ao <index> <value>   set analog output");
    println!("  ct <index> <value>   set counter");
    println!("  show                 print current database");
    println!("  help                 print this help");
    print_cli_examples();
}

pub fn handle_cli_line(database: &RwLock<Database>, line: &str) -> Result<(), String> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }

    let mut parts = line.split_whitespace();
    let command = parts.next().unwrap_or_default().to_ascii_lowercase();
    let rest: Vec<&str> = parts.collect();

    match command.as_str() {
        "help" | "?" => print_cli_help(),
        "show" | "list" => {
            let database = database
                .read()
                .map_err(|_| "database lock poisoned".to_string())?;
            print_database_summary(&database);
        }
        "bi" | "binary-input" => {
            let point = parse_indexed_bool(&join_args(&rest))?;
            let mut database = database
                .write()
                .map_err(|_| "database lock poisoned".to_string())?;
            set_binary_input(&mut database, point.index, point.value);
            println!("updated BI[{}]={}", point.index, on_off(point.value));
        }
        "bo" | "binary-output" => {
            let point = parse_indexed_bool(&join_args(&rest))?;
            let mut database = database
                .write()
                .map_err(|_| "database lock poisoned".to_string())?;
            set_binary_output(&mut database, point.index, point.value);
            println!("updated BO[{}]={}", point.index, on_off(point.value));
        }
        "ai" | "analog-input" => {
            let point = parse_indexed_i32(&join_args(&rest))?;
            let mut database = database
                .write()
                .map_err(|_| "database lock poisoned".to_string())?;
            set_analog_input(&mut database, point.index, point.value);
            println!("updated AI[{}]={}", point.index, point.value);
        }
        "ao" | "analog-output" => {
            let point = parse_indexed_i32(&join_args(&rest))?;
            let mut database = database
                .write()
                .map_err(|_| "database lock poisoned".to_string())?;
            set_analog_output(&mut database, point.index, point.value);
            println!("updated AO[{}]={}", point.index, point.value);
        }
        "ct" | "counter" => {
            let point = parse_indexed_u32(&join_args(&rest))?;
            let mut database = database
                .write()
                .map_err(|_| "database lock poisoned".to_string())?;
            set_counter(&mut database, point.index, point.value);
            println!("updated counter[{}]={}", point.index, point.value);
        }
        _ => return Err(format!("unknown command '{command}'; type 'help'")),
    }

    if command != "help" && command != "?" {
        print_cli_examples();
    }
    Ok(())
}

fn join_args(parts: &[&str]) -> String {
    parts.join(" ")
}

pub fn spawn_interactive_cli(database: Arc<RwLock<Database>>) {
    std::thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if let Err(err) = handle_cli_line(&database, &line) {
                        eprintln!("{err}");
                    }
                }
                Err(err) => {
                    eprintln!("stdin closed: {err}");
                    break;
                }
            }
        }
    });
}

pub fn print_database_summary(database: &Database) {
    for point in &database.binary_inputs {
        println!("BI[{}]={}", point.index, on_off(point.flags & 0x01 != 0));
    }
    for point in &database.binary_outputs {
        println!("BO[{}]={}", point.index, on_off(point.flags & 0x01 != 0));
    }
    for point in &database.analog_inputs {
        println!("AI[{}]={}", point.index, point.value);
    }
    for point in &database.analog_outputs {
        println!("AO[{}]={}", point.index, point.value);
    }
    for point in &database.counters {
        println!("counter[{}]={}", point.index, point.value);
    }
}

pub fn hex_line(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn describe_frame(frame: &LinkFrame) -> String {
    let control = frame.control.encode();
    let function = match frame.control.function {
        LinkFunction::Primary(primary) => format!("primary {primary:?}"),
        LinkFunction::Secondary(secondary) => format!("secondary {secondary:?}"),
    };
    format!(
        "ctrl=0x{control:02x} {function} dest={} src={} payload={}",
        frame.destination.value(),
        frame.source.value(),
        frame.payload.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_index_equals_value() {
        let point = parse_indexed_bool("0=0").unwrap();
        assert_eq!(point.index, 0);
        assert!(!point.value);
    }

    #[test]
    fn parses_index_space_value() {
        let point = parse_indexed_bool("1 0").unwrap();
        assert_eq!(point.index, 1);
        assert!(!point.value);
    }

    #[test]
    fn binary_flags_encoding() {
        assert_eq!(binary_flags(false), 0x00);
        assert_eq!(binary_flags(true), 0x81);
    }
}
