# Quickstart

## Install Rust

Install Rust with rustup:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

On Windows, install from <https://rustup.rs/> and use PowerShell or Windows Terminal.

## Build

```sh
cargo build --workspace
```

## Run Tests

```sh
cargo test --workspace --all-features
```

For the same checks used by CI:

```sh
sh scripts/test.sh
```

On Windows:

```powershell
.\scripts\test.ps1
```

## TCP Outstation

```sh
cargo run --example tcp_outstation -- --bind 127.0.0.1:20000
```

## TCP Master

```sh
cargo run --example tcp_master -- --connect 127.0.0.1:20000
```

## Serial Outstation

```sh
cargo run --example serial_outstation -- --port COM3 --baud-rate 9600
```

## Serial Master

```sh
cargo run --example serial_master -- --port COM4 --baud-rate 9600
```

## TCP Configuration

Use an address in `host:port` form:

```sh
cargo run --example tcp_outstation -- --bind 0.0.0.0:20000
cargo run --example tcp_master -- --connect 192.0.2.10:20000
```

## Serial Configuration

Common Windows names are `COM1`, `COM2`, and `COM3`. Common Unix-like names are `/dev/ttyUSB0`, `/dev/ttyS0`, and `/dev/tty.usbserial-*`.

```sh
cargo run --example serial_master -- --port /dev/ttyUSB0 --baud-rate 19200 --timeout-ms 1000
```

## Troubleshooting

- If a Windows serial port is busy, close vendor tools, terminal programs, services, and any running example that may have the port open.
- If an executable cannot be overwritten on Windows, stop the running process or debugger and rebuild.
- If TCP examples cannot connect, check the bind address, local firewall, and whether the outstation process is already listening.
- If serial traffic is silent, verify baud rate, parity, stop bits, data bits, cable wiring, and RS-485 direction control on the adapter.
