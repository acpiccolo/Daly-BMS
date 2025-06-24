use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use flexi_logger::{Logger, LoggerHandle};
use log::*;
use serde_json::json;
use std::{ops::Deref, panic, time::Duration}; // For MQTT payload construction

mod mqtt; // Added MQTT module

const MQTT_CONFIG_FILE: &str = "mqtt.yaml";

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
        /// Comma-separated list of metrics to fetch
        #[clap(
            long,
            short,
            use_value_delimiter = true,
            default_value = "soc,voltage,current"
        )]
        metrics: Vec<String>,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum DaemonOutput {
    /// Continuously read metrics and print them to the standard output (console).
    Console,
    /// Continuously read metrics and publish them to an MQTT broker.
    Mqtt {
        /// Path to an optional YAML file for MQTT configuration.
        /// If not provided, "mqtt.yaml" is loaded from the current directory.
        #[arg(long, value_name = "PATH")]
        mqtt_config_path: Option<String>,
    },
}

const fn about_text() -> &'static str {
    "daly bms command line tool"
}

#[derive(Parser, Debug)]
#[command(version, about=about_text(), long_about = None)]
struct CliArgs {
    #[command(flatten)]
    verbose: Verbosity<InfoLevel>,

    /// Serial port device path (e.g., /dev/ttyUSB0 on Linux, COM1 on Windows)
    #[arg(short, long, default_value_t = default_device_name())]
    device: String,

    #[command(subcommand)]
    command: CliCommands,

    /// Timeout for serial I/O operations (e.g., "500ms", "1s", "2s 500ms")
    #[arg(value_parser = humantime::parse_duration, long, default_value = "500ms")]
    timeout: Duration,

    // Some USB - RS485 dongles requires at least 10ms to switch between TX and RX, so use a save delay between frames
    /// Delay between sending multiple commands to the BMS (e.g., "50ms", "100ms")
    /// (useful for some serial adapters that need time to switch between TX/RX)
    #[arg(value_parser = humantime::parse_duration, long, default_value = "50ms")]
    delay: Duration,
}

fn logging_init(loglevel: LevelFilter) -> LoggerHandle {
    let log_handle = Logger::try_with_env_or_str(loglevel.as_str())
        .expect("Cannot init logging")
        .start()
        .expect("Cannot start logging");

    panic::set_hook(Box::new(|panic_info| {
        let (filename, line, column) = panic_info
            .location()
            .map(|loc| (loc.file(), loc.line(), loc.column()))
            .unwrap_or(("<unknown>", 0, 0));
        let cause = panic_info
            .payload()
            .downcast_ref::<String>()
            .map(String::deref);
        let cause = cause.unwrap_or_else(|| {
            panic_info
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .unwrap_or("<cause unknown>")
        });

        error!(
            "Thread '{}' panicked at {}:{}:{}: {}",
            std::thread::current().name().unwrap_or("<unknown>"),
            filename,
            line,
            column,
            cause
        );
    }));
    log_handle
}

macro_rules! print_status {
    ($bms:expr) => {
        println!(
            "Status: {:?}",
            $bms.get_status().with_context(|| "Cannot get status")?
        )
    };
}
macro_rules! print_soc {
    ($bms:expr) => {
        println!(
            "SOC: {:?}",
            $bms.get_soc().with_context(|| "Cannot get SOC")?
        )
    };
}
macro_rules! print_mosfet_status {
    ($bms:expr) => {
        println!(
            "Mosfet: {:?}",
            $bms.get_mosfet_status()
                .with_context(|| "Cannot get mosfet status")?
        )
    };
}
macro_rules! print_voltage_range {
    ($bms:expr) => {
        println!(
            "Voltage range: {:?}",
            $bms.get_cell_voltage_range()
                .with_context(|| "Cannot get voltage range")?
        )
    };
}
macro_rules! print_temperature_range {
    ($bms:expr) => {
        println!(
            "Temperature range: {:?}",
            $bms.get_temperature_range()
                .with_context(|| "Cannot get temperature range")?
        )
    };
}
macro_rules! print_cell_voltages {
    ($bms:expr) => {
        println!(
            "Cell Voltages: {:?}",
            $bms.get_cell_voltages()
                .with_context(|| "Cannot get cell voltages")?
        )
    };
}
macro_rules! print_cell_temperatures {
    ($bms:expr) => {
        println!(
            "Cell temperatures: {:?}",
            $bms.get_cell_temperatures()
                .with_context(|| "Cannot get cell temperatures")?
        )
    };
}
macro_rules! print_balancing_status {
    ($bms:expr) => {
        println!(
            "Balancing status: {:?}",
            $bms.get_balancing_status()
                .with_context(|| "Cannot get balancing stats")?
        )
    };
}
macro_rules! print_errors {
    ($bms:expr) => {
        println!(
            "Errors: {:?}",
            $bms.get_errors().with_context(|| "Cannot get errors")?
        )
    };
}

fn main() -> Result<()> {
    let args = CliArgs::parse();

    let _log_handle = logging_init(args.verbose.log_level_filter());

    let mut bms = dalybms_lib::serialport::DalyBMS::new(&args.device)
        .with_context(|| format!("Cannot open serial port '{}'", args.device))?;
    bms.set_timeout(args.timeout)?;
    bms.set_delay(args.delay);

    match args.command {
        CliCommands::Status => print_status!(bms),
        CliCommands::Soc => print_soc!(bms),
        CliCommands::VoltageRange => print_voltage_range!(bms),
        CliCommands::TemperatureRange => print_temperature_range!(bms),
        CliCommands::Mosfet => print_mosfet_status!(bms),
        CliCommands::CellVoltages => {
            let _ = bms.get_status().with_context(|| "Cannot get status")?;
            print_cell_voltages!(bms);
        }
        CliCommands::CellTemperatures => {
            let _ = bms.get_status().with_context(|| "Cannot get status")?;
            print_cell_temperatures!(bms);
        }
        CliCommands::Balancing => {
            let _ = bms.get_status().with_context(|| "Cannot get status")?;
            print_balancing_status!(bms);
        }
        CliCommands::Errors => print_errors!(bms),
        CliCommands::All => {
            print_status!(bms);
            print_soc!(bms);
            print_voltage_range!(bms);
            print_temperature_range!(bms);
            print_mosfet_status!(bms);
            print_cell_voltages!(bms);
            print_cell_temperatures!(bms);
            print_balancing_status!(bms);
            print_errors!(bms);
            print_soc!(bms);
        }
        CliCommands::SetSoc { soc_percent } => {
            bms.set_soc(soc_percent).with_context(|| "Cannot set SOC")?
        }
        CliCommands::SetChargeMosfet { enable } => bms
            .set_charge_mosfet(enable)
            .with_context(|| "Cannot set charge mosfet")?,
        CliCommands::SetDischargeMosfet { enable } => bms
            .set_discharge_mosfet(enable)
            .with_context(|| "Cannot set discharge mosfet")?,
        CliCommands::Reset => bms.reset()?,
        CliCommands::Daemon {
            output,
            interval,
            metrics,
        } => {
            info!(
                "Starting daemon mode: output={:?}, interval={:?}, metrics={:?}",
                output, interval, metrics
            );

            let mut mqtt_publisher: Option<mqtt::MqttPublisher> = None;

            if let DaemonOutput::Mqtt { mqtt_config_path } = &output {
                let config_path_str = mqtt_config_path
                    .clone()
                    .unwrap_or_else(|| MQTT_CONFIG_FILE.to_string());

                let config = mqtt::load_mqtt_config(&config_path_str).with_context(|| {
                    format!("Failed to open MQTT config file at '{config_path_str}'")
                })?;
                info!("Successfully loaded MQTT config from {config_path_str}: {config:?}");
                let publisher = mqtt::MqttPublisher::new(config)
                    .with_context(|| "Failed to create MQTT publisher")?;
                info!("MQTT Publisher created successfully.");
                mqtt_publisher = Some(publisher);
            }

            loop {
                let mut fetched_status = None;
                let mut fetched_soc = None;
                let mut fetched_voltages = None;
                let mut fetched_temperatures = None;

                let fetch_all = metrics.iter().any(|m| m == "all");

                if fetch_all {
                    info!("Fetching all metrics due to 'all' flag.");
                    // If "all" is present, ensure we attempt to fetch everything
                    // regardless of other specific metrics in the list for this iteration.
                    // We can clear metrics and add all individual ones, or just check fetch_all.
                    // For simplicity, we'll just use the fetch_all flag in subsequent checks.
                }

                for metric_name in &metrics {
                    if !fetch_all && metric_name == "all" {
                        // already handled above if "all" is primary
                        continue;
                    }

                    if fetch_all || metric_name == "status" {
                        info!("Fetching metric: status");
                        match bms.get_status() {
                            Ok(status) => fetched_status = Some(status),
                            Err(e) => error!("Error fetching status: {}", e),
                        }
                    }
                    if fetch_all || metric_name == "soc" {
                        info!("Fetching metric: soc");
                        match bms.get_soc() {
                            Ok(soc) => fetched_soc = Some(soc),
                            Err(e) => error!("Error fetching SOC: {}", e),
                        }
                    }
                    if fetch_all || metric_name == "voltages" {
                        info!("Fetching metric: voltages");
                        if fetched_status.is_none() && !fetch_all {
                            // Ensure status is fetched if not already
                            info!("Fetching status first for cell voltages");
                            match bms.get_status() {
                                Ok(status) => fetched_status = Some(status),
                                Err(e) => error!("Error fetching status for voltages: {}", e),
                            }
                        }
                        if fetched_status.is_some() || fetch_all {
                            // Proceed if status available or if fetching all
                            match bms.get_cell_voltages() {
                                Ok(voltages) => fetched_voltages = Some(voltages),
                                Err(e) => error!("Error fetching cell voltages: {}", e),
                            }
                        } else if !fetch_all {
                            // only log if not covered by 'all' already
                            error!("Skipping voltage fetch: status unavailable and not fetching all metrics.");
                        }
                    }
                    if fetch_all || metric_name == "temperatures" {
                        info!("Fetching metric: temperatures");
                        if fetched_status.is_none() && !fetch_all {
                            // Ensure status is fetched if not already
                            info!("Fetching status first for cell temperatures");
                            match bms.get_status() {
                                Ok(status) => fetched_status = Some(status),
                                Err(e) => error!("Error fetching status for temperatures: {}", e),
                            }
                        }
                        if fetched_status.is_some() || fetch_all {
                            // Proceed if status available or if fetching all
                            match bms.get_cell_temperatures() {
                                Ok(temps) => fetched_temperatures = Some(temps),
                                Err(e) => error!("Error fetching cell temperatures: {}", e),
                            }
                        } else if !fetch_all {
                            // only log if not covered by 'all' already
                            error!("Skipping temperature fetch: status unavailable and not fetching all metrics.");
                        }
                    }
                    // If only "all" was specified, no need to iterate further for this loop.
                    if fetch_all {
                        break;
                    }
                }

                match output {
                    DaemonOutput::Console => {
                        println!("--- Data at {} ---", chrono::Local::now().to_rfc3339());
                        if let Some(status) = fetched_status {
                            println!("Status: {status:?}");
                        }
                        if let Some(soc) = fetched_soc {
                            println!("SOC: {soc:?}");
                        }
                        if let Some(voltages) = fetched_voltages {
                            println!("Cell Voltages: {voltages:?}");
                        }
                        if let Some(temperatures) = fetched_temperatures {
                            println!("Cell Temperatures: {temperatures:?}");
                        }
                        println!("--------------------------");
                    }
                    DaemonOutput::Mqtt { .. } => {
                        if let Some(publisher) = &mqtt_publisher {
                            let mut data_to_publish = serde_json::Map::new();
                            data_to_publish.insert(
                                "timestamp".to_string(),
                                json!(chrono::Utc::now().to_rfc3339()),
                            );

                            if let Some(status) = &fetched_status {
                                match serde_json::to_value(status) {
                                    Ok(val) => {
                                        data_to_publish.insert("status".to_string(), val);
                                    }
                                    Err(e) => {
                                        error!("Failed to serialize status to JSON value: {}", e)
                                    }
                                }
                            }
                            if let Some(soc) = &fetched_soc {
                                match serde_json::to_value(soc) {
                                    Ok(val) => {
                                        data_to_publish.insert("soc".to_string(), val);
                                    }
                                    Err(e) => {
                                        error!("Failed to serialize soc to JSON value: {}", e)
                                    }
                                }
                            }
                            if let Some(voltages) = &fetched_voltages {
                                match serde_json::to_value(voltages) {
                                    Ok(val) => {
                                        data_to_publish.insert("cell_voltages".to_string(), val);
                                    }
                                    Err(e) => error!(
                                        "Failed to serialize cell_voltages to JSON value: {}",
                                        e
                                    ),
                                }
                            }
                            if let Some(temperatures) = &fetched_temperatures {
                                match serde_json::to_value(temperatures) {
                                    Ok(val) => {
                                        data_to_publish
                                            .insert("cell_temperatures".to_string(), val);
                                    }
                                    Err(e) => error!(
                                        "Failed to serialize cell_temperatures to JSON value: {}",
                                        e
                                    ),
                                }
                            }

                            // Only publish if there's more than just the timestamp
                            if data_to_publish.len() > 1 {
                                match serde_json::to_string(&data_to_publish) {
                                    Ok(json_payload) => {
                                        info!(
                                            "MQTT output: Attempting to publish data: {}",
                                            json_payload
                                        );
                                        if let Err(e) = publisher.publish(&json_payload) {
                                            error!("Failed to publish data to MQTT: {:?}", e);
                                        } else {
                                            info!("Successfully published data to MQTT.");
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to serialize data to JSON string: {}", e);
                                    }
                                }
                            } else {
                                info!("No data fetched in this cycle to publish via MQTT.");
                            }
                        } else {
                            warn!("MQTT output selected, but publisher is not initialized. Skipping publish.");
                        }
                    }
                }
                std::thread::sleep(interval);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // use super::*; // Removed as items like default_bms_status are local or directly imported
    use dalybms_lib::protocol::{IOState, Soc as BmsSoc, Status as BmsStatus};
    use serde_json::{json, Value as JsonValue};

    // Helper to create a default BmsStatus for tests
    fn default_bms_status() -> BmsStatus {
        BmsStatus {
            cells: 16,
            temperature_sensors: 2,
            charger_running: false,
            load_running: true,
            states: IOState {
                di1: false,
                di2: false,
                di3: false,
                di4: false,
                do1: true,
                do2: false,
                do3: false,
                do4: false,
            },
            cycles: 123,
        }
    }

    // Helper to create a default BmsSoc for tests
    fn default_bms_soc() -> BmsSoc {
        BmsSoc {
            total_voltage: 54.3,
            current: -1.2, // Charging
            soc_percent: 85.5,
        }
    }

    #[test]
    fn test_serialize_bms_data_to_json_full() {
        let bms_status = Some(default_bms_status());
        let bms_soc = Some(default_bms_soc());
        let cell_voltages = Some(vec![3.301, 3.302, 3.300, 3.303]); // Example voltages
        let cell_temperatures = Some(vec![25, 26]); // Example temperatures

        let mut data_to_publish_map = serde_json::Map::new();
        let timestamp = chrono::Utc::now().to_rfc3339();
        data_to_publish_map.insert("timestamp".to_string(), json!(timestamp));

        if let Some(status) = &bms_status {
            data_to_publish_map.insert("status".to_string(), serde_json::to_value(status).unwrap());
        }
        if let Some(soc) = &bms_soc {
            data_to_publish_map.insert("soc".to_string(), serde_json::to_value(soc).unwrap());
        }
        if let Some(voltages) = &cell_voltages {
            data_to_publish_map.insert(
                "cell_voltages".to_string(),
                serde_json::to_value(voltages).unwrap(),
            );
        }
        if let Some(temperatures) = &cell_temperatures {
            data_to_publish_map.insert(
                "cell_temperatures".to_string(),
                serde_json::to_value(temperatures).unwrap(),
            );
        }

        let json_payload_result = serde_json::to_string(&data_to_publish_map);
        assert!(json_payload_result.is_ok());
        let json_payload = json_payload_result.unwrap();

        let parsed_value: JsonValue = serde_json::from_str(&json_payload).unwrap();

        assert_eq!(parsed_value["timestamp"], timestamp);
        assert!(parsed_value["status"].is_object());
        assert_eq!(parsed_value["status"]["cells"], 16);
        assert!(parsed_value["soc"].is_object());
        assert_eq!(parsed_value["soc"]["soc_percent"], 85.5);
        assert!(parsed_value["cell_voltages"].is_array());
        assert_eq!(parsed_value["cell_voltages"].as_array().unwrap().len(), 4);
        assert_eq!(parsed_value["cell_voltages"][0], 3.301);
        assert!(parsed_value["cell_temperatures"].is_array());
        assert_eq!(
            parsed_value["cell_temperatures"].as_array().unwrap().len(),
            2
        );
        assert_eq!(parsed_value["cell_temperatures"][0], 25);
    }

    #[test]
    fn test_serialize_bms_data_partial() {
        let bms_status: Option<BmsStatus> = None; // Status not available
        let bms_soc = Some(default_bms_soc());
        let _cell_voltages: Option<Vec<f32>> = None;
        let _cell_temperatures: Option<Vec<i32>> = None;

        let mut data_to_publish_map = serde_json::Map::new();
        let timestamp = chrono::Utc::now().to_rfc3339();
        data_to_publish_map.insert("timestamp".to_string(), json!(timestamp));

        if let Some(status) = &bms_status {
            // This will be false
            data_to_publish_map.insert("status".to_string(), serde_json::to_value(status).unwrap());
        }
        if let Some(soc) = &bms_soc {
            data_to_publish_map.insert("soc".to_string(), serde_json::to_value(soc).unwrap());
        }
        // Voltages and temperatures are None

        let json_payload_result = serde_json::to_string(&data_to_publish_map);
        assert!(json_payload_result.is_ok());
        let json_payload = json_payload_result.unwrap();

        let parsed_value: JsonValue = serde_json::from_str(&json_payload).unwrap();

        assert_eq!(parsed_value["timestamp"], timestamp);
        assert!(parsed_value["status"].is_null());
        assert!(parsed_value["soc"].is_object());

        const TEST_EPSILON: f64 = 1e-5; // Using a larger epsilon
        let total_voltage_json = parsed_value["soc"]["total_voltage"].as_f64().unwrap();
        assert!((total_voltage_json - 54.3).abs() < TEST_EPSILON);
        assert!(parsed_value["cell_voltages"].is_null());
        assert!(parsed_value["cell_temperatures"].is_null());

        // Check number of keys (should be timestamp + soc)
        assert_eq!(parsed_value.as_object().unwrap().keys().count(), 2);
    }

    #[test]
    fn test_serialize_bms_data_empty() {
        // Test when no actual BMS data is available, only timestamp
        let bms_status: Option<BmsStatus> = None;
        let bms_soc: Option<BmsSoc> = None;
        let _cell_voltages: Option<Vec<f32>> = None; // Prefixed with underscore
        let _cell_temperatures: Option<Vec<i32>> = None; // Prefixed with underscore

        let mut data_to_publish_map = serde_json::Map::new();
        let timestamp = chrono::Utc::now().to_rfc3339();
        data_to_publish_map.insert("timestamp".to_string(), json!(timestamp));

        // No data is added beyond timestamp
        if let Some(status) = &bms_status {
            data_to_publish_map.insert("status".to_string(), serde_json::to_value(status).unwrap());
        }
        if let Some(soc) = &bms_soc {
            data_to_publish_map.insert("soc".to_string(), serde_json::to_value(soc).unwrap());
        }
        // etc for others

        // The main loop has: if data_to_publish.len() > 1 { ... }
        // So, if only timestamp is there, it won't try to serialize.
        // This test should reflect the structure IF it were to serialize just a timestamp,
        // or test the condition that prevents it.
        // Let's test the map before the conditional serialization.
        assert_eq!(data_to_publish_map.len(), 1); // Only timestamp
        assert!(data_to_publish_map.contains_key("timestamp"));

        // If we were to serialize it (though the main loop wouldn't for MQTT if len <=1):
        let json_payload_result = serde_json::to_string(&data_to_publish_map);
        assert!(json_payload_result.is_ok());
        let json_payload = json_payload_result.unwrap();
        let parsed_value: JsonValue = serde_json::from_str(&json_payload).unwrap();
        assert_eq!(parsed_value.as_object().unwrap().keys().count(), 1);
        assert_eq!(parsed_value["timestamp"], timestamp);
    }
}
