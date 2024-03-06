[![CI](https://github.com/acpiccolo/Daly-BMS/actions/workflows/check.yml/badge.svg)](https://github.com/acpiccolo/Daly-BMS/actions/workflows/check.yml)
[![dependency status](https://deps.rs/repo/github/acpiccolo/Daly-BMS/status.svg)](https://deps.rs/repo/github/acpiccolo/Daly-BMS)
[![CI](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/acpiccolo/Daly-BMS/blob/main/LICENSE-MIT)
[![CI](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://github.com/acpiccolo/Daly-BMS/blob/main/LICENSE-APACHE)
[![CI](https://img.shields.io/badge/Conventional%20Commits-1.0.0-yellow.svg)](https://conventionalcommits.org)

# Daly BMS
This RUST project can read and write a Daly BMS module from the command line.

## Hardware
The following hardware is required for this project:
* One or more R413D08 8 channel modules.
* One or more relay modules 1-8 channels.
* One USB-RS485 converter.

![R413D08 controller](/images/r413d08.png)

### Data sheet R413D08
* Operating Voltage: DC 5 Volt (5V version) or DC 6-24 Volt (12V version)
* Operating Current: 10-15 Milli-Ampere

## Compilation
1. Install Rust e.g. using [these instructions](https://www.rust-lang.org/learn/get-started).
2. Ensure that you have a C compiler and linker.
3. Clone `git clone https://github.com/acpiccolo/R413D08-Controller.git`
4. Run `cargo install --path .` to install the binary. Alternatively,
   check out the repository and run `cargo build --release`. This will compile
   the binary to `target/release/ch8ctl`.

## Getting started
To see all available commands:
```
ch8ctl --help
```
For TCP Modbus connected temperature collectors:
```
ch8ctl tcp 192.168.0.222:502 read
```

### Cargo Features
| Feature | Purpose | Default |
| :--- | :------ | :-----: |
| `tokio-rtu-sync` | Enable the implementation for the tokio modbus synchronous RTU client | ✅ |
| `tokio-rtu` | Enable the implementation for the tokio modbus asynchronous RTU client | ✅ |
| `tokio-tcp-sync` | Enable the implementation for the tokio modbus synchronous TCP client | - |
| `tokio-tcp` | Enable the implementation for the tokio modbus asynchronous TCP client | - |
| `bin-dependencies` | Enable all features required by the binary | ✅ |


## License
Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
