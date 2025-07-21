use anyhow::{bail, Context, Result};
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
        /// Comma-separated list of metrics to fetch (e.g., status,soc,voltages,temperatures or all)
        #[clap(long, short, use_value_delimiter = true, default_value = "status,soc")]
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

    /// Number of retries for failed commands
    #[arg(long, default_value = "3")]
    retries: u8,
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
    bms.set_retry(args.retries);

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
                let mut fetched_mosfet = None;
                let mut fetched_voltage_range = None;
                let mut fetched_temperature_range = None;
                let mut fetched_cell_voltages = None;
                let mut fetched_cell_temperatures = None;
                let mut fetched_balancing = None;
                let mut fetched_errors = None;

                let fetch_all = metrics.iter().any(|m| m == "all");

                if fetch_all {
                    info!("Fetching all metrics due to 'all' flag.");
                    // If "all" is present, ensure we attempt to fetch everything
                    // regardless of other specific metrics in the list for this iteration.
                    // We can clear metrics and add all individual ones, or just check fetch_all.
                    // For simplicity, we'll just use the fetch_all flag in subsequent checks.
                }

                let mut found_metric_name = false;
                for metric_name in &metrics {
                    if !fetch_all && metric_name == "all" {
                        // already handled above if "all" is primary
                        continue;
                    }

                    if fetch_all || metric_name == "status" {
                        info!("Fetching metric: status");
                        found_metric_name = true;
                        match bms.get_status() {
                            Ok(status) => fetched_status = Some(status),
                            Err(e) => error!("Error fetching status: {}", e),
                        }
                    }
                    if fetch_all || metric_name == "soc" {
                        info!("Fetching metric: soc");
                        found_metric_name = true;
                        match bms.get_soc() {
                            Ok(soc) => fetched_soc = Some(soc),
                            Err(e) => error!("Error fetching SOC: {}", e),
                        }
                    }
                    if fetch_all || metric_name == "mosfet" {
                        info!("Fetching metric: mosfet");
                        found_metric_name = true;
                        match bms.get_mosfet_status() {
                            Ok(range) => fetched_mosfet = Some(range),
                            Err(e) => error!("Error fetching mosfet: {}", e),
                        }
                    }
                    if fetch_all || metric_name == "voltage-range" {
                        info!("Fetching metric: voltage-range");
                        found_metric_name = true;
                        match bms.get_cell_voltage_range() {
                            Ok(range) => fetched_voltage_range = Some(range),
                            Err(e) => error!("Error fetching voltage-range: {}", e),
                        }
                    }
                    if fetch_all || metric_name == "temperature-range" {
                        info!("Fetching metric: temperature-range");
                        found_metric_name = true;
                        match bms.get_temperature_range() {
                            Ok(range) => fetched_temperature_range = Some(range),
                            Err(e) => error!("Error fetching temperature-range: {}", e),
                        }
                    }
                    if fetch_all || metric_name == "cell-voltages" {
                        info!("Fetching metric: cell-voltages");
                        found_metric_name = true;
                        if fetched_status.is_none() && !fetch_all {
                            // Ensure status is fetched if not already
                            info!("Fetching status first for cell-voltages");
                            match bms.get_status() {
                                Ok(status) => fetched_status = Some(status),
                                Err(e) => error!("Error fetching status for cell-voltages: {}", e),
                            }
                        }
                        if fetched_status.is_some() || fetch_all {
                            // Proceed if status available or if fetching all
                            match bms.get_cell_voltages() {
                                Ok(voltages) => fetched_cell_voltages = Some(voltages),
                                Err(e) => error!("Error fetching cell-voltages: {}", e),
                            }
                        } else if !fetch_all {
                            // only log if not covered by 'all' already
                            error!("Skipping voltage fetch: status unavailable and not fetching all metrics.");
                        }
                    }
                    if fetch_all || metric_name == "cell-temperatures" {
                        info!("Fetching metric: cell-temperatures");
                        found_metric_name = true;
                        if fetched_status.is_none() && !fetch_all {
                            // Ensure status is fetched if not already
                            info!("Fetching status first for cell-temperatures");
                            match bms.get_status() {
                                Ok(status) => fetched_status = Some(status),
                                Err(e) => {
                                    error!("Error fetching status for cell-temperatures: {}", e)
                                }
                            }
                        }
                        if fetched_status.is_some() || fetch_all {
                            // Proceed if status available or if fetching all
                            match bms.get_cell_temperatures() {
                                Ok(temps) => fetched_cell_temperatures = Some(temps),
                                Err(e) => error!("Error fetching cell-temperatures: {}", e),
                            }
                        } else if !fetch_all {
                            // only log if not covered by 'all' already
                            error!("Skipping temperature fetch: status unavailable and not fetching all metrics.");
                        }
                    }
                    if fetch_all || metric_name == "balancing" {
                        info!("Fetching metric: balancing");
                        found_metric_name = true;
                        match bms.get_balancing_status() {
                            Ok(range) => fetched_balancing = Some(range),
                            Err(e) => error!("Error fetching balancing: {}", e),
                        }
                    }
                    if fetch_all || metric_name == "errors" {
                        info!("Fetching metric: errors");
                        found_metric_name = true;
                        match bms.get_errors() {
                            Ok(range) => fetched_errors = Some(range),
                            Err(e) => error!("Error fetching errors: {}", e),
                        }
                    }
                    // If only "all" was specified, no need to iterate further for this loop.
                    if fetch_all {
                        break;
                    }
                    if !found_metric_name {
                        bail!("Unknown metric name '{metric_name}'");
                    }
                }

                match output {
                    DaemonOutput::Console => {
                        println!("--- Data at {} ---", chrono::Local::now().to_rfc3339());
                        if let Some(status) = fetched_status {
                            println!("{status:?}");
                        }
                        if let Some(soc) = fetched_soc {
                            println!("{soc:?}");
                        }
                        if let Some(mosfet) = fetched_mosfet {
                            println!("{mosfet:?}");
                        }
                        if let Some(voltage_range) = fetched_voltage_range {
                            println!("{voltage_range:?}");
                        }
                        if let Some(temperature_range) = fetched_temperature_range {
                            println!("{temperature_range:?}");
                        }
                        if let Some(cell_voltages) = fetched_cell_voltages {
                            println!("{cell_voltages:?}");
                        }
                        if let Some(cell_temperatures) = fetched_cell_temperatures {
                            println!("{cell_temperatures:?}");
                        }
                        if let Some(balancing) = fetched_balancing {
                            println!("{balancing:?}");
                        }
                        if let Some(errors) = fetched_errors {
                            println!("{errors:?}");
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
                                let val = serde_json::to_value(status)
                                    .with_context(|| "Failed to serialize status to JSON value")?;
                                data_to_publish.insert("status".to_string(), val);
                            }
                            if let Some(soc) = &fetched_soc {
                                let val = serde_json::to_value(soc)
                                    .with_context(|| "Failed to serialize soc to JSON value")?;
                                data_to_publish.insert("soc".to_string(), val);
                            }
                            if let Some(mosfet) = &fetched_mosfet {
                                let val = serde_json::to_value(mosfet)
                                    .with_context(|| "Failed to serialize mosfet to JSON value")?;
                                data_to_publish.insert("mosfet".to_string(), val);
                            }
                            if let Some(voltage_range) = &fetched_voltage_range {
                                let val =
                                    serde_json::to_value(voltage_range).with_context(|| {
                                        "Failed to serialize voltage_range to JSON value"
                                    })?;
                                data_to_publish.insert("voltage_range".to_string(), val);
                            }
                            if let Some(temperature_range) = &fetched_temperature_range {
                                let val =
                                    serde_json::to_value(temperature_range).with_context(|| {
                                        "Failed to serialize temperature_range to JSON value"
                                    })?;
                                data_to_publish.insert("temperature_range".to_string(), val);
                            }
                            if let Some(cell_voltages) = &fetched_cell_voltages {
                                let val =
                                    serde_json::to_value(cell_voltages).with_context(|| {
                                        "Failed to serialize cell_voltages to JSON value"
                                    })?;
                                data_to_publish.insert("cell_voltages".to_string(), val);
                            }
                            if let Some(cell_temperatures) = &fetched_cell_temperatures {
                                let val =
                                    serde_json::to_value(cell_temperatures).with_context(|| {
                                        "Failed to serialize cell_temperatures to JSON value"
                                    })?;
                                data_to_publish.insert("cell_temperatures".to_string(), val);
                            }
                            if let Some(balancing) = &fetched_balancing {
                                let val = serde_json::to_value(balancing).with_context(|| {
                                    "Failed to serialize balancing to JSON value"
                                })?;
                                data_to_publish.insert("balancing".to_string(), val);
                            }
                            if let Some(errors) = &fetched_errors {
                                let val = serde_json::to_value(errors)
                                    .with_context(|| "Failed to serialize errors to JSON value")?;
                                data_to_publish.insert("errors".to_string(), val);
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
