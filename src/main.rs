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
    /// Show status
    Status,
    /// Show voltage, current, SOC
    Soc,
    /// Show mosfet status
    Mosfet,
    /// Show voltage range
    VoltageRange,
    /// Show temperature range
    TemperatureRange,
    /// Show cell voltages
    CellVoltages,
    /// Show temperature sensor values
    CellTemperatures,
    /// Show cell balancing status
    Balancing,
    /// Show BMS errors
    Errors,
    /// Show all
    All,
    /// Set SOC in percent from '0.0' to '100.0'
    SetSoc { soc_percent: f32 },
    /// Enable or disable discharge mosfet
    SetDischargeMosfet {
        #[clap(long, short, action)]
        enable: bool,
    },
    /// Enable or disable charge mosfet
    SetChargeMosfet {
        #[clap(long, short, action)]
        enable: bool,
    },
    /// Reset the BMS
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

    /// Device
    #[arg(short, long, default_value_t = default_device_name())]
    device: String,

    #[command(subcommand)]
    command: CliCommands,

    /// Serial Input/Output operations timeout
    #[arg(value_parser = humantime::parse_duration, long, default_value = "500ms")]
    timeout: Duration,

    // Some USB - RS485 dongles requires at least 10ms to switch between TX and RX, so use a save delay between frames
    /// Delay between multiple commands
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
