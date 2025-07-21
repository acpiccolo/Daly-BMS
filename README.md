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

The `dalybms` command-line tool allows you to interact with your Daly BMS from the terminal.

### Basic Help

To see all available commands and options:
```bash
dalybms --help
```

To get help for a specific subcommand, for example `set-soc`:
```bash
dalybms set-soc --help
```

### CLI Examples

Here are some examples of how to use the `dalybms` tool. Replace `/dev/ttyUSB0` with the actual serial port your BMS is connected to, if different.

**1. Fetching Basic Information**

*   Get State of Charge (SOC), total voltage, and current:
    ```bash
    dalybms --device /dev/ttyUSB0 soc
    ```
    (Output will be similar to: `SOC: Soc { total_voltage: 53.6, current: -0.0, soc_percent: 87.5 }`)

*   Get general status information (number of cells, temperature sensors, charger/load status, cycles):
    ```bash
    dalybms status
    ```
    (Assumes default device or you can specify `--device`)

*   Get MOSFET status (mode, charging/discharging MOSFET state, BMS cycles, capacity):
    ```bash
    dalybms mosfet
    ```

*   Get cell voltage range (highest/lowest cell voltage and which cell):
    ```bash
    dalybms voltage-range
    ```

*   Get temperature range (highest/lowest temperature and which sensor):
    ```bash
    dalybms temperature-range
    ```

*   Get current error codes:
    ```bash
    dalybms errors
    ```
    (Output will be like: `Errors: []` if no errors, or show a list of active errors)

**2. Fetching Detailed Information**

*Important*: For commands like `cell-voltages`, `cell-temperatures`, and `balancing`, the BMS needs to know the number of cells/sensors. The `dalybms` tool automatically calls `status` first if you haven't, but it's good practice to be aware of this.

*   Get individual cell voltages:
    ```bash
    dalybms cell-voltages
    ```

*   Get individual cell/temperature sensor readings:
    ```bash
    dalybms cell-temperatures
    ```

*   Get cell balancing status (shows which cells are currently being balanced):
    ```bash
    dalybms balancing
    ```

**3. Setting Values and Controlling MOSFETs**

*   Set the State of Charge (SOC) to 80.5%:
    ```bash
    dalybms set-soc 80.5
    ```

*   Enable the discharge MOSFET:
    ```bash
    dalybms set-discharge-mosfet --enable
    ```

*   Disable the discharge MOSFET:
    ```bash
    dalybms set-discharge-mosfet
    ```

*   Enable the charge MOSFET:
    ```bash
    dalybms set-charge-mosfet --enable
    ```

*   Disable the charge MOSFET:
    ```bash
    dalybms set-charge-mosfet 
    ```

**4. Fetching All Information**

*   Get all available information from the BMS (runs most of the read commands sequentially):
    ```bash
    dalybms all
    ```
    (This is very useful for a quick overview.)

**5. Specifying Connection Parameters**

*   Use a different serial device:
    ```bash
    dalybms --device /dev/ttyACM0 status
    ```

*   Change the communication timeout (e.g., to 1 second):
    ```bash
    dalybms --timeout 1s soc
    ```

*   Change the delay between commands (e.g., to 100 milliseconds):
    ```bash
    dalybms --delay 100ms all
    ```
    (Useful if you experience communication issues with the default delay.)

**6. Resetting the BMS**

*   Reset the BMS to factory defaults (Use with extreme caution!):
    ```bash
    dalybms reset
    ```

These examples should help you get started with using the `dalybms` command-line tool. Always refer to `dalybms --help` and `dalybms <subcommand> --help` for the most up-to-date options and parameters.

## Daemon Mode

The `dalybms` tool includes a daemon mode for continuous monitoring and data export, useful for logging BMS data over time or integrating with monitoring systems like Home Assistant via MQTT.

### Overview

Daemon mode runs persistently, fetching specified metrics from the BMS at regular intervals and outputting them to the console or an MQTT broker.

### Command-Line Usage

The basic command to start daemon mode is:
```bash
dalybms daemon [OPTIONS]
```

**Daemon Options:**

*   `--output <console|mqtt>`: (Required) Specifies where to send the data.
    *   `console`: Prints data to the standard output.
    *   `mqtt`: Publishes data to an MQTT broker. Requires `mqtt.yaml` for configuration.
*   `--interval <DURATION>`: Sets how often to fetch and report data. This is a duration string like "10s", "1m", "2h30m".
    *   Default: "10s" (10 seconds).
*   `--metrics <METRICS>`: A comma-separated list of specific metrics to collect.
    *   Available metrics: `status`, `soc`, `voltages` (individual cell voltages), `temperatures` (individual sensor readings), `all`.
    *   If `all` is included, all available metrics will be fetched.
    *   Default: "soc,status" (Note: The current implementation of daemon mode primarily supports `status`, `soc`, `voltages`, `temperatures`, and `all`. Other metrics specified in a custom list might only be logged as "Fetching metric: <name>" without specific data parsing in the output yet).

### MQTT Configuration (`mqtt.yaml`)

When using `--output mqtt`, the tool requires a configuration file named `mqtt.yaml` in the root directory where you run the `dalybms` command.

This file contains details for connecting to your MQTT broker:

*   `uri`: (String) MQTT broker server uri (e.g., mqtt://localhost:1883).
*   `username`: (String, Optional) Username for MQTT authentication.
*   `password`: (String, Optional) Password for MQTT authentication.
*   `topic`: (String, Optional) Base MQTT topic to publish data to. Defaults to "dalybms" if not set.
*   `qos` (Integer, Optional): MQTT Quality of Service level (0, 1, or 2). Defaults to 0 if not set.
*   `client_id`: (String, Optional) Custom client ID for this connection. If blank or omitted, a default ID (e.g., "dalybms-<random_suffix>") will be generated.

Please refer to the example `mqtt.yaml` file in the repository for exact formatting and more comments.

### Daemon Mode Examples

1.  **Console Output:** Fetch SOC and general status every 30 seconds and print to console.
    ```bash
    dalybms daemon --output console --interval 30s --metrics soc,status
    ```

2.  **MQTT Output:** Fetch all available metrics every 5 minutes and publish to an MQTT broker (ensure `mqtt.yaml` is configured).
    ```bash
    dalybms daemon --output mqtt --interval 5m --metrics all
    ```

    (You would also need an `mqtt.yaml` file in the same directory, for example:)
    ```yaml
    # mqtt.yaml
    uri: "mqtt://localhost:1883"
    username: "your_username" # Optional
    password: "your_password" # Optional
    topic: "dalybms" # Optional
    client_id: "dalybms_1" # Optional
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
use dalybms_lib::serialport::DalyBMS;
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
use dalybms_lib::tokio_serial_async::DalyBMS;
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
