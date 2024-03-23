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

### Cargo Features
| Feature | Purpose | Default |
| :--- | :------ | :-----: |
| `serialport` | Enable the implementation for the synchronous serialport client | - |
| `tokio-serial-async` | Enable the implementation for the tokio serial asynchronous client | - |
| `bin-dependencies` | Enable all features required by the binary | ✅ |


## License
Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
