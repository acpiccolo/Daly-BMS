use anyhow::Result;
use clap::{Parser, Subcommand};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use dalybms_lib;
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
    /// Show cell voltages
    CellVoltages,
    /// Show temperature sensor values
    Temperatures,
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

    /// Modbus timeout in milliseconds
    #[arg(value_parser = humantime::parse_duration, long, default_value = "500ms")]
    timeout: Duration,

    // Some USB - RS485 dongles requires at least 10ms to switch between TX and RX, so use a save delay between frames
    /// Delay between multiple modbus commands in milliseconds
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

fn main() -> Result<()> {
    let args = CliArgs::parse();

    let _log_handle = logging_init(args.verbose.log_level_filter());

    // https://minimalmodbus.readthedocs.io/en/stable/serialcommunication.html#timing-of-the-serial-communications
    // minimum delay 4ms by baud rate 9600
    let delay = Duration::max(args.delay, Duration::from_millis(4));

    let mut bms = dalybms_lib::Bms::new(&args.device, args.timeout)?;

    match args.command {
        CliCommands::Status => {
            println!("Status: {:?}", bms.get_status()?);
        }
        CliCommands::Soc => {
            println!("SOC: {:?}", bms.get_soc()?);
        }
        CliCommands::Mosfet => {
            println!("Mosfet: {:?}", bms.get_mosfet_status()?);
        }
        CliCommands::CellVoltages => {
            let _ = bms.get_status()?;
            std::thread::sleep(delay);
            println!("CellVoltages: {:?}", bms.get_cell_voltages()?);
        }
        CliCommands::Temperatures => {
            let _ = bms.get_status()?;
            std::thread::sleep(delay);
            println!("Temperatures: {:?}", bms.get_cell_temperatures()?);
        }
        CliCommands::Balancing => {
            let _ = bms.get_status()?;
            std::thread::sleep(delay);
            println!("Balancing: {:?}", bms.get_balancing_status()?);
        }
        CliCommands::Errors => {
            println!("Errors: {:?}", bms.get_errors()?);
        }
        CliCommands::All => {
            println!("Status: {:?}", bms.get_status()?);
            std::thread::sleep(delay);
            println!("SOC: {:?}", bms.get_soc()?);
            std::thread::sleep(delay);
            println!("CellVoltageRange: {:?}", bms.get_cell_voltage_range()?);
            std::thread::sleep(delay);
            println!("TemperatureRange: {:?}", bms.get_temperature_range()?);
            std::thread::sleep(delay);
            println!("Mosfet: {:?}", bms.get_mosfet_status()?);
            std::thread::sleep(delay);
            println!("CellVoltages: {:?}", bms.get_cell_voltages()?);
            std::thread::sleep(delay);
            println!("CellTemperatures: {:?}", bms.get_cell_temperatures()?);
            std::thread::sleep(delay);
            println!("Balancing: {:?}", bms.get_balancing_status()?);
            std::thread::sleep(delay);
            println!("Errors: {:?}", bms.get_errors()?);
        }
        CliCommands::SetSoc { soc_percent } => bms.set_soc(soc_percent)?,
        CliCommands::SetChargeMosfet { enable } => bms.set_charge_mosfet(enable)?,
        CliCommands::SetDischargeMosfet { enable } => bms.set_discharge_mosfet(enable)?,
        CliCommands::Reset => bms.reset()?,
    }

    Ok(())
}
