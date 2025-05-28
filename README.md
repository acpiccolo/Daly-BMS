[![CI](https://github.com/acpiccolo/Daly-BMS/actions/workflows/check.yml/badge.svg)](https://github.com/acpiccolo/Daly-BMS/actions/workflows/check.yml)
[![dependency status](https://deps.rs/repo/github/acpiccolo/Daly-BMS/status.svg)](https://deps.rs/repo/github/acpiccolo/Daly-BMS)
[![CI](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/acpiccolo/Daly-BMS/blob/main/LICENSE-MIT)
[![CI](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://github.com/acpiccolo/Daly-BMS/blob/main/LICENSE-APACHE)
[![CI](https://img.shields.io/badge/Conventional%20Commits-1.0.0-yellow.svg)](https://conventionalcommits.org)

# Daly BMS
This RUST project can read and write a Daly BMS module from the command line.

## Compilation
1. Install Rust e.g. using [these instructions](https://www.rust-lang.org/learn/get-started).
2. Ensure that you have a C compiler and linker.
3. Clone `git clone https://github.com/acpiccolo/Daly-BMS.git`
4. Run `cargo install --path .` to install the binary. Alternatively,
   check out the repository and run `cargo build --release`. This will compile
   the binary to `target/release/dalybms`.

## Getting started
To see all available commands:
```
dalybms --help
```

## Library Usage

This crate can also be used as a library (`dalybms_lib`) to interact with Daly BMS programmatically from your own Rust projects.

### Adding as a Dependency

To use `dalybms_lib`, add it to your `Cargo.toml`. Replace `"x.y.z"` with the desired version of `dalybms_lib`:

```toml
[dependencies]
# For the synchronous client:
dalybms_lib = { version = "x.y.z", features = ["serialport"] }

# For the asynchronous client:
# dalybms_lib = { version = "x.y.z", features = ["tokio-serial-async"] }

# For both clients:
# dalybms_lib = { version = "x.y.z", features = ["serialport", "tokio-serial-async"] }
```

You need to specify which client(s) you intend to use via feature flags:
- `serialport`: For the synchronous client.
- `tokio-serial-async`: For the asynchronous Tokio-based client.

You can enable both if needed: `features = ["serialport", "tokio-serial-async"]`

### Synchronous Client Example

The synchronous client uses the `serialport` crate.

**Feature flag required**: `serialport`

```rust
use dalybms_lib::serialport::DalyBMS; // Corrected path
use std::time::Duration;

fn main() {
    match DalyBMS::new("/dev/ttyUSB0") { // Replace with your serial port
        Ok(mut bms) => {
            bms.set_timeout(Duration::from_millis(500)).unwrap_or_else(|e| {
                eprintln!("Error setting timeout: {:?}", e);
            });

            match bms.get_soc() {
                Ok(soc) => {
                    println!("SOC: {:.1}%, Voltage: {:.1}V, Current: {:.1}A",
                             soc.soc_percent, soc.total_voltage, soc.current);
                }
                Err(e) => {
                    eprintln!("Error getting SOC: {:?}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to connect to BMS: {:?}", e);
        }
    }
}
```

### Asynchronous Client Example

The asynchronous client uses `tokio` and `tokio-serial`.

**Feature flag required**: `tokio-serial-async`

```rust
use dalybms_lib::tokio_serial_async::DalyBMS; // Corrected path
use std::time::Duration;

#[tokio::main]
async fn main() {
    match DalyBMS::new("/dev/ttyUSB0") { // Replace with your serial port
        Ok(mut bms) => {
            bms.set_timeout(Duration::from_millis(500)).unwrap_or_else(|e| {
                 // In async, set_timeout is sync, so direct error handling is fine
                eprintln!("Error setting timeout: {:?}", e);
            });

            match bms.get_soc().await {
                Ok(soc) => {
                    println!("SOC: {:.1}%, Voltage: {:.1}V, Current: {:.1}A",
                             soc.soc_percent, soc.total_voltage, soc.current);
                }
                Err(e) => {
                    eprintln!("Error getting SOC: {:?}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to connect to BMS: {:?}", e);
        }
    }
}
```

### Cargo Features

This crate (`dalybms_lib`) uses feature flags to manage optional dependencies and client implementations. This allows users to compile only the parts they need.

| Feature              | Purpose                                                                 | Client Enabled     | Default |
| :------------------- | :---------------------------------------------------------------------- | :----------------- | :-----: |
| `serialport`         | Enables the **synchronous** client using the `serialport` crate.        | Synchronous        | No      |
| `tokio-serial-async` | Enables the **asynchronous** client using `tokio` and `tokio-serial`. | Asynchronous       | No      |
| `serde`              | Enables `serde` support for serializing/deserializing data structures.  | Both (if enabled)  | No      |
| `bin-dependencies`   | Enables all features required by the `dalybms` binary executable.       | `serialport`       | Yes (for `dalybms` binary target) |

**Notes on Features:**
- When using `dalybms_lib` as a library, you should explicitly enable `serialport` and/or `tokio-serial-async` depending on your needs.
- The `serde` feature can be combined with either client feature if you need serialization capabilities (e.g., `features = ["serialport", "serde"]`).
- The `default` feature for the `dalybms` *crate as a whole* is `bin-dependencies`. However, for `dalybms_lib` when used as a dependency, no client features are enabled by default.
- The `bin-dependencies` feature enables `serialport` because the `dalybms` command-line tool currently uses the synchronous client.

## Protocol Details

For those interested in the low-level communication details, the Daly BMS UART/RS485 communication protocol specification (version 1.2) is available in the repository at `docs/Daly UART_485 Communications Protocol V1.2.pdf`.

## License
Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
