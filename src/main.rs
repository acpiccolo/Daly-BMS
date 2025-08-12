use anyhow::{Context, Result};
use clap::Parser;
use flexi_logger::{Logger, LoggerHandle};
use log::*;
use std::{ops::Deref, panic};

mod commandline;
mod daemon;
mod mqtt;

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

fn print_info<F, T>(label: &str, mut func: F) -> Result<()>
where
    F: FnMut() -> Result<T>,
    T: std::fmt::Debug,
{
    println!("{}: {:?}", label, func()?);
    Ok(())
}

fn main() -> Result<()> {
    let args = commandline::CliArgs::parse();

    let _log_handle = logging_init(args.verbose.log_level_filter());

    let mut bms = dalybms_lib::serialport::DalyBMS::new(&args.device)
        .with_context(|| format!("Cannot open serial port '{}'", args.device))?;
    bms.set_timeout(args.timeout)?;
    bms.set_delay(args.delay);
    bms.set_retry(args.retries);

    match args.command {
        commandline::CliCommands::Status => print_info("Status", || {
            bms.get_status().with_context(|| "Cannot get status")
        })?,
        commandline::CliCommands::Soc => {
            print_info("SOC", || bms.get_soc().with_context(|| "Cannot get SOC"))?
        }
        commandline::CliCommands::VoltageRange => print_info("Voltage range", || {
            bms.get_cell_voltage_range()
                .with_context(|| "Cannot get voltage range")
        })?,
        commandline::CliCommands::TemperatureRange => print_info("Temperature range", || {
            bms.get_temperature_range()
                .with_context(|| "Cannot get temperature range")
        })?,
        commandline::CliCommands::Mosfet => print_info("Mosfet", || {
            bms.get_mosfet_status()
                .with_context(|| "Cannot get mosfet status")
        })?,
        commandline::CliCommands::CellVoltages => {
            let _ = bms.get_status().with_context(|| "Cannot get status")?;
            print_info("Cell Voltages", || {
                bms.get_cell_voltages()
                    .with_context(|| "Cannot get cell voltages")
            })?
        }
        commandline::CliCommands::CellTemperatures => {
            let _ = bms.get_status().with_context(|| "Cannot get status")?;
            print_info("Cell temperatures", || {
                bms.get_cell_temperatures()
                    .with_context(|| "Cannot get cell temperatures")
            })?
        }
        commandline::CliCommands::Balancing => {
            let _ = bms.get_status().with_context(|| "Cannot get status")?;
            print_info("Balancing status", || {
                bms.get_balancing_status()
                    .with_context(|| "Cannot get balancing stats")
            })?
        }
        commandline::CliCommands::Errors => print_info("Errors", || {
            bms.get_errors().with_context(|| "Cannot get errors")
        })?,
        commandline::CliCommands::All => {
            print_info("Status", || {
                bms.get_status().with_context(|| "Cannot get status")
            })?;
            print_info("SOC", || bms.get_soc().with_context(|| "Cannot get SOC"))?;
            print_info("Voltage range", || {
                bms.get_cell_voltage_range()
                    .with_context(|| "Cannot get voltage range")
            })?;
            print_info("Temperature range", || {
                bms.get_temperature_range()
                    .with_context(|| "Cannot get temperature range")
            })?;
            print_info("Mosfet", || {
                bms.get_mosfet_status()
                    .with_context(|| "Cannot get mosfet status")
            })?;
            print_info("Cell Voltages", || {
                bms.get_cell_voltages()
                    .with_context(|| "Cannot get cell voltages")
            })?;
            print_info("Cell temperatures", || {
                bms.get_cell_temperatures()
                    .with_context(|| "Cannot get cell temperatures")
            })?;
            print_info("Balancing status", || {
                bms.get_balancing_status()
                    .with_context(|| "Cannot get balancing stats")
            })?;
            print_info("Errors", || {
                bms.get_errors().with_context(|| "Cannot get errors")
            })?;
            print_info("SOC", || bms.get_soc().with_context(|| "Cannot get SOC"))?;
        }
        commandline::CliCommands::SetSoc { soc_percent } => {
            bms.set_soc(soc_percent).with_context(|| "Cannot set SOC")?
        }
        commandline::CliCommands::SetChargeMosfet { enable } => bms
            .set_charge_mosfet(enable)
            .with_context(|| "Cannot set charge mosfet")?,
        commandline::CliCommands::SetDischargeMosfet { enable } => bms
            .set_discharge_mosfet(enable)
            .with_context(|| "Cannot set discharge mosfet")?,
        commandline::CliCommands::Reset => bms.reset()?,
        commandline::CliCommands::Daemon {
            output,
            interval,
            metrics,
        } => daemon::run(bms, output, interval, metrics)?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // use super::*; // Removed as items like default_bms_status are local or directly imported
    use dalybms_lib::protocol::{IOState, Soc as BmsSoc, Status as BmsStatus};
    use serde_json::{Value as JsonValue, json};

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
