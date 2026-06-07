# DNP3 Rust Library

This is an original Rust workspace for building DNP3 / IEEE 1815 protocol clients, servers, simulators, gateways, and embedded integrations.

The implementation separates deterministic protocol encoding and decoding from network and serial I/O. This makes the protocol crates easy to test without hardware while still providing asynchronous TCP and serial transports for applications.

## Requirements

- Rust 1.78 or newer with Cargo
- A supported desktop or server OS for TCP examples
- A serial port driver for serial examples

Install Rust from <https://rustup.rs/>.

## Build

```sh
cargo build --workspace
```

## Test

```sh
cargo test --workspace --all-features
```

The serial tests use mock transports, so CI and local test runs do not require physical RS-232 or RS-485 hardware.

## Run TCP Examples

Start an outstation (defaults: master `1`, outstation `10`):

```sh
cargo run --example tcp_outstation -- --bind 127.0.0.1:20000 --outstation 10
```

Configure points at startup (`INDEX=VALUE` or `INDEX VALUE`):

```sh
cargo run --example tcp_outstation -- --bind 127.0.0.1:20000 --binary-input 0=1 --analog-input 0=2500
```

While the outstation is running, use the interactive CLI in the same terminal (`bi`, `bo`, `ai`, `ao`, `ct`, `show`). Pass `--no-interactive` to disable it, or `--verbose` to log link frames.

Start a master in another terminal (addresses must match the outstation). Use `--connect`, not `--bind`:

```sh
cargo run --example tcp_master -- --connect 127.0.0.1:20000 --master 1 --outstation 10
```

If the master times out during link reset, stop any stale outstation on that port and start a fresh one first:

```sh
# Windows PowerShell
Stop-Process -Name tcp_outstation -Force -ErrorAction SilentlyContinue
```

The master performs link reset, test link, then polls continuously (default every 2s). Use `--poll-interval-ms 1000` to change the rate.

For third-party simulators connecting to `tcp_outstation`, set **master address = 1**, **outstation address = 10**, and map points at **index 0** (G1V2 binary input, G10V2 binary output, G30V1 analog input, G40V1 analog output, G20V1 counter).

## Run Serial Examples

Start a serial outstation:

```sh
cargo run --example serial_outstation -- --port COM3 --baud-rate 9600 --outstation 10 --binary-input 0=1
```

Start a serial master on the paired port with matching serial settings and addresses:

```sh
cargo run --example serial_master -- --port COM4 --baud-rate 9600 --master 1 --outstation 10
```

On Unix-like systems, serial port names are commonly `/dev/ttyUSB0`, `/dev/ttyS0`, or `/dev/tty.usbserial-*`. The master and outstation must use the same baud rate, data bits, parity, and stop bits.

## Configuration Example

```toml
[tcp]
host = "127.0.0.1"
port = 20000
connect_timeout_ms = 5000
read_timeout_ms = 5000
reconnect_delay_ms = 1000

[serial]
port = "COM3"
baud_rate = 9600
data_bits = 8
parity = "none"
stop_bits = 1
timeout_ms = 1000
```

## Troubleshooting

On Windows, serial ports are usually named `COM1`, `COM2`, and so on. If a port fails to open, check Device Manager, close other programs that may be using the port, and verify the port parameters match the connected device.

If Cargo cannot replace an executable on Windows, stop any running example, test process, terminal session, or debugger that may still be holding the binary open, then rerun the command.

## Workspace Layout

- `crates/dnp3-core`: protocol primitives, CRC, encoding helpers, time, and errors.
- `crates/dnp3-link`: link-layer frame encoding and decoding.
- `crates/dnp3-transport`: transport fragmentation, reassembly, and sequence handling.
- `crates/dnp3-app`: application-layer function codes, object headers, and common object data.
- `crates/dnp3-master`: master/client workflow helpers.
- `crates/dnp3-outstation`: outstation/server workflow helpers.
- `crates/dnp3-tcp`: Tokio TCP client/server transport.
- `crates/dnp3-serial`: cross-platform serial transport and mock serial transport.
- `examples`: runnable TCP and serial examples.
- `scripts`: test and coverage helper scripts.

## Questions

If you have any trouble running the examples or integrating the library, email [hamzamehboob103@gmail.com](mailto:hamzamehboob103@gmail.com).

## License

This project is source-available for evaluation, learning, internal development, and non-commercial use. Commercial use requires prior written permission from the copyright owner. See `LICENSE` and `COMMERCIAL_USE.md`.
