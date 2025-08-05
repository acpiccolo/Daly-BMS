use crate::mqtt;
use clap::{Parser, Subcommand};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use std::time::Duration;

fn default_device_name() -> String {
    if cfg!(target_os = "windows") {
        String::from("COM1")
    } else {
        String::from("/dev/ttyUSB0")
    }
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum CliCommands {
    /// Show general BMS status: cell count, temperature sensors, charger/load status, cycles
    Status,
    /// Show total voltage, current, and State of Charge (SOC)
    Soc,
    /// Show MOSFET status: mode, charge/discharge state, capacity, and BMS cycles
    Mosfet,
    /// Show highest/lowest cell voltage and corresponding cell number
    VoltageRange,
    /// Show highest/lowest temperature and corresponding sensor number
    TemperatureRange,
    /// Show individual cell voltages (requires BMS status to be fetched first or will fetch it)
    CellVoltages,
    /// Show individual temperature sensor readings (requires BMS status to be fetched first or will fetch it)
    CellTemperatures,
    /// Show cell balancing status (requires BMS status to be fetched first or will fetch it)
    Balancing,
    /// Show current BMS error codes
    Errors,
    /// Show all available BMS information by running most read commands
    All,
    /// Set State of Charge (SOC) in percent
    SetSoc {
        /// The desired SOC value as a percentage (e.g., 75.5 for 75.5%)
        soc_percent: f32,
    },
    /// Enable or disable the discharge MOSFET
    SetDischargeMosfet {
        /// Enable the discharge MOSFET. If this flag is not present, it will be disabled.
        #[clap(long, short, action)]
        enable: bool,
    },
    /// Enable or disable the charge MOSFET
    SetChargeMosfet {
        /// Enable the charge MOSFET. If this flag is not present, it will be disabled.
        #[clap(long, short, action)]
        enable: bool,
    },
    /// Reset the BMS to factory settings (Use with caution!)
    Reset,
    /// Run in daemon mode, periodically fetching and outputting metrics
    Daemon {
        /// Output destination for metrics
        #[command(subcommand)]
        output: DaemonOutput,
        /// Interval for fetching metrics (e.g., "10s", "1m")
        #[clap(long, short, value_parser = humantime::parse_duration, default_value = "10s")]
        interval: Duration,
        /// Comma-separated list of metrics to fetch (e.g., status,soc,voltages,temperatures or all)
        #[clap(long, short, use_value_delimiter = true, default_value = "status,soc")]
        metrics: Vec<String>,
    },
}

#[derive(clap::ValueEnum, Debug, Clone, PartialEq)]
pub enum MqttFormat {
    Simple,
    Json,
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum DaemonOutput {
    /// Continuously read metrics and print them to the standard output (console).
    Console,
    /// Continuously read metrics and publish them to an MQTT broker.
    Mqtt {
        /// The configuration file for the MQTT broker
        #[arg(long, default_value_t = mqtt::MqttConfig::DEFAULT_CONFIG_FILE.to_string())]
        config_file: String,
        /// Output format for MQTT messages
        #[arg(long, value_enum, default_value_t = MqttFormat::Simple)]
        format: MqttFormat,
    },
}

const fn about_text() -> &'static str {
    "daly bms command line tool"
}

#[derive(Parser, Debug)]
#[command(version, about=about_text(), long_about = None)]
pub struct CliArgs {
    #[command(flatten)]
    pub verbose: Verbosity<InfoLevel>,

    /// Serial port device path (e.g., /dev/ttyUSB0 on Linux, COM1 on Windows)
    #[arg(short, long, default_value_t = default_device_name())]
    pub device: String,

    #[command(subcommand)]
    pub command: CliCommands,

    /// Timeout for serial I/O operations (e.g., "100ms", "1s", "2s 500ms")
    #[arg(value_parser = humantime::parse_duration, long, default_value = "100ms")]
    pub timeout: Duration,

    // Some USB - RS485 dongles requires at least 10ms to switch between TX and RX, so use a save delay between frames
    /// Delay between sending multiple commands to the BMS (e.g., "50ms", "100ms")
    /// (useful for some serial adapters that need time to switch between TX/RX)
    #[arg(value_parser = humantime::parse_duration, long, default_value = "15ms")]
    pub delay: Duration,

    /// Number of retries for failed commands
    #[arg(long, default_value = "3")]
    pub retries: u8,
}
