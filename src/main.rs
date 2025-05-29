use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use flexi_logger::{Logger, LoggerHandle};
use log::*;
use std::{ops::Deref, panic, time::Duration};

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
    }

    Ok(())
}
