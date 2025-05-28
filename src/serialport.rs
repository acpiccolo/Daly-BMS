//! Provides a synchronous client for interacting with a Daly BMS (Battery Management System)
//! using a serial port connection.
//!
//! This module relies on the `serialport` crate for serial communication.
//!
//! # Example
//!
//! ```no_run
//! use dalybms_lib::serialport::{DalyBMS, Error};
//! use std::time::Duration;
//!
//! fn main() -> Result<(), Error> {
//!     let mut bms = DalyBMS::new("/dev/ttyUSB0")?;
//!     bms.set_timeout(Duration::from_millis(500))?;
//!
//!     let soc = bms.get_soc()?;
//!     println!("SOC: {:?}", soc);
//!
//!     // It's recommended to call get_status() first to populate cell/sensor counts
//!     // for other methods like get_cell_voltages() or get_cell_temperatures().
//!     let status = bms.get_status()?;
//!     println!("Status: {:?}", status);
//!
//!     let cell_voltages = bms.get_cell_voltages()?;
//!     println!("Cell Voltages: {:?}", cell_voltages);
//!
//!     Ok(())
//! }
//! ```

use crate::protocol::*;
use std::time::{Duration, Instant};

/// Errors specific to the synchronous serial port client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error indicating that `get_status()` must be called before certain other methods
    /// that rely on information like cell count or temperature sensor count.
    #[error("get_status() has to be called at least once before")]
    StatusError,
    /// An error originating from the underlying Daly BMS protocol library.
    #[error("Daly error: {0}")]
    DalyError(#[from] crate::Error),
    /// An I/O error, typically from the serial port communication.
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    /// An error from the `serialport` crate.
    #[error("Tokio serial error: {0}")] // Note: Typo in original, should be "Serialport error"
    Serial(#[from] serialport::Error),
}

/// A specialized `Result` type for operations within the `serialport` module.
type Result<T> = std::result::Result<T, Error>;

/// The main struct for interacting with a Daly BMS over a serial port.
///
/// It handles sending commands and receiving/decoding responses from the BMS.
/// Most methods require a mutable reference to `self` as they involve serial communication
/// and may update internal state (like the last execution time or cached status).
#[derive(Debug)]
pub struct DalyBMS {
    serial: Box<dyn serialport::SerialPort>,
    last_execution: Instant,
    delay: Duration,
    status: Option<Status>, // Stores the latest status to provide cell/sensor counts
}

impl DalyBMS {
    /// Creates a new `DalyBMS` instance.
    ///
    /// # Arguments
    ///
    /// * `port`: The path to the serial port device (e.g., `/dev/ttyUSB0` on Linux, `COM3` on Windows).
    ///
    /// # Returns
    ///
    /// A `Result` containing the `DalyBMS` instance or an `Error` if the serial port
    /// cannot be opened or configured.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dalybms_lib::serialport::DalyBMS;
    ///
    /// let bms = DalyBMS::new("/dev/ttyUSB0");
    /// if let Ok(mut bms_instance) = bms {
    ///     // Use the BMS instance
    ///     if let Ok(soc) = bms_instance.get_soc() {
    ///         println!("SOC: {}%", soc.soc_percent);
    ///     }
    /// } else {
    ///     eprintln!("Failed to connect to BMS: {:?}", bms.err());
    /// }
    /// ```
    pub fn new(port: &str) -> Result<Self> {
        Ok(Self {
            serial: serialport::new(port, 9600)
                .data_bits(serialport::DataBits::Eight)
                .parity(serialport::Parity::None)
                .stop_bits(serialport::StopBits::One)
                .flow_control(serialport::FlowControl::None)
                .open()?,
            last_execution: Instant::now(),
            delay: MINIMUM_DELAY, // Default delay from protocol module
            status: None,
        })
    }

    /// Waits for the configured delay duration since the last command execution.
    /// This is a private helper to ensure commands are not sent too frequently.
    fn serial_await_delay(&self) {
        let last_exec_diff = Instant::now().duration_since(self.last_execution);
        if let Some(time_until_delay_reached) = self.delay.checked_sub(last_exec_diff) {
            std::thread::sleep(time_until_delay_reached);
        }
    }

    /// Private helper to send bytes to the serial port.
    /// It handles clearing pending data, awaiting delay, and writing the buffer.
    fn send_bytes(&mut self, tx_buffer: &[u8]) -> Result<()> {
        // clear all incoming serial to avoid data collision
        loop {
            log::trace!("read to see if there is any pending data");
            let pending = self.serial.bytes_to_read()?;
            log::trace!("got {} pending bytes", pending);
            if pending > 0 {
                let mut buf: Vec<u8> = vec![0; 64]; // Temporary buffer to drain
                let received = self.serial.read(buf.as_mut_slice())?;
                log::trace!("{} pending bytes consumed", received);
            } else {
                break;
            }
        }
        self.serial_await_delay();

        log::trace!("write bytes: {:02X?}", tx_buffer);
        self.serial.write_all(tx_buffer)?;

        // Flushing is usually not necessary for USB serial devices and can sometimes cause issues.
        // If needed, it can be enabled here.
        if false { // Disabled by default
            log::trace!("flush connection");
            self.serial.flush()?;
        }
        Ok(())
    }

    /// Private helper to receive a specified number of bytes from the serial port.
    fn receive_bytes(&mut self, size: usize) -> Result<Vec<u8>> {
        let mut rx_buffer = vec![0; size];

        log::trace!("read {} bytes", rx_buffer.len());
        self.serial.read_exact(&mut rx_buffer)?;

        self.last_execution = Instant::now(); // Update last execution time after successful read

        log::trace!("receive bytes: {:02X?}", rx_buffer);
        Ok(rx_buffer)
    }

    /// Sets the timeout for serial port I/O operations.
    ///
    /// # Arguments
    ///
    /// * `timeout`: The duration to wait for an operation to complete before timing out.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success or an `Error` if the timeout could not be set.
    pub fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
        log::trace!("set timeout to {:?}", timeout);
        self.serial.set_timeout(timeout).map_err(Error::from)
    }

    /// Sets the minimum delay between sending commands to the BMS.
    ///
    /// If the provided `delay` is less than `MINIMUM_DELAY` from the `protocol` module,
    /// `MINIMUM_DELAY` will be used.
    ///
    /// # Arguments
    ///
    /// * `delay`: The desired minimum delay between commands.
    pub fn set_delay(&mut self, delay: Duration) {
        if delay < MINIMUM_DELAY {
            log::warn!(
                "delay {:?} lower minimum {:?}, use minimum",
                delay,
                MINIMUM_DELAY
            );
            self.delay = MINIMUM_DELAY;
        } else {
            self.delay = delay;
        }
        log::trace!("set delay to {:?}", self.delay);
    }

    /// Retrieves the State of Charge (SOC) and other primary battery metrics.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `Soc` data or an `Error` if the command fails or decoding is unsuccessful.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dalybms_lib::serialport::{DalyBMS, Error};
    /// # use std::time::Duration;
    /// # fn main() -> Result<(), Error> {
    /// # let mut bms = DalyBMS::new("/dev/ttyUSB0")?;
    /// let soc_data = bms.get_soc()?;
    /// println!("Voltage: {:.1}V, Current: {:.1}A, SOC: {:.1}%",
    ///          soc_data.total_voltage, soc_data.current, soc_data.soc_percent);
    /// # Ok(())
    /// # }
    /// ```
    pub fn get_soc(&mut self) -> Result<Soc> {
        log::trace!("get SOC");
        self.send_bytes(&Soc::request(Address::Host))?;
        Ok(Soc::decode(&self.receive_bytes(Soc::reply_size())?)?)
    }

    /// Retrieves the highest and lowest cell voltages in the battery pack.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `CellVoltageRange` data or an `Error`.
    pub fn get_cell_voltage_range(&mut self) -> Result<CellVoltageRange> {
        log::trace!("get cell voltage range");
        self.send_bytes(&CellVoltageRange::request(Address::Host))?;
        Ok(CellVoltageRange::decode(
            &self.receive_bytes(CellVoltageRange::reply_size())?,
        )?)
    }

    /// Retrieves the highest and lowest temperatures measured by the BMS.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `TemperatureRange` data or an `Error`.
    pub fn get_temperature_range(&mut self) -> Result<TemperatureRange> {
        log::trace!("get temperature range");
        self.send_bytes(&TemperatureRange::request(Address::Host))?;
        Ok(TemperatureRange::decode(
            &self.receive_bytes(TemperatureRange::reply_size())?,
        )?)
    }

    /// Retrieves the status of the charging and discharging MOSFETs, and other related data.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `MosfetStatus` data or an `Error`.
    pub fn get_mosfet_status(&mut self) -> Result<MosfetStatus> {
        log::trace!("get mosfet status");
        self.send_bytes(&MosfetStatus::request(Address::Host))?;
        Ok(MosfetStatus::decode(
            &self.receive_bytes(MosfetStatus::reply_size())?,
        )?)
    }

    /// Retrieves general status information from the BMS, including cell count and temperature sensor count.
    ///
    /// This method also caches the retrieved status internally, as this information is
    /// required by other methods like `get_cell_voltages` and `get_cell_temperatures`.
    /// It's recommended to call this method at least once before calling those methods.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `Status` data or an `Error`.
    pub fn get_status(&mut self) -> Result<Status> {
        log::trace!("get status");
        self.send_bytes(&Status::request(Address::Host))?;
        let status = Status::decode(&self.receive_bytes(Status::reply_size())?)?;
        self.status = Some(status.clone()); // Cache the status
        Ok(status)
    }

    /// Retrieves the voltage of each individual cell in the battery pack.
    ///
    /// **Note:** `get_status()` must be called at least once before this method
    /// to determine the number of cells.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec<f32>` of cell voltages or an `Error`.
    /// Returns `Error::StatusError` if `get_status()` was not called previously.
    pub fn get_cell_voltages(&mut self) -> Result<Vec<f32>> {
        log::trace!("get cell voltages");
        let n_cells = if let Some(status) = &self.status {
            status.cells
        } else {
            return Err(Error::StatusError);
        };
        self.send_bytes(&CellVoltages::request(Address::Host))?;
        Ok(CellVoltages::decode(
            &self.receive_bytes(CellVoltages::reply_size(n_cells))?,
            n_cells,
        )?)
    }

    /// Retrieves the temperature from each individual temperature sensor.
    ///
    /// **Note:** `get_status()` must be called at least once before this method
    /// to determine the number of temperature sensors.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec<i32>` of temperatures in Celsius or an `Error`.
    /// Returns `Error::StatusError` if `get_status()` was not called previously.
    pub fn get_cell_temperatures(&mut self) -> Result<Vec<i32>> {
        log::trace!("get cell temperatures");
        let n_sensors = if let Some(status) = &self.status {
            status.temperature_sensors
        } else {
            return Err(Error::StatusError);
        };

        self.send_bytes(&CellTemperatures::request(Address::Host))?;
        Ok(CellTemperatures::decode(
            &self.receive_bytes(CellTemperatures::reply_size(n_sensors))?,
            n_sensors,
        )?)
    }

    /// Retrieves the balancing status of each individual cell.
    ///
    /// **Note:** `get_status()` must be called at least once before this method
    /// to determine the number of cells.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec<bool>` where `true` indicates the cell is currently balancing,
    /// or an `Error`. Returns `Error::StatusError` if `get_status()` was not called previously.
    pub fn get_balancing_status(&mut self) -> Result<Vec<bool>> {
        log::trace!("get balancing status");
        let n_cells = if let Some(status) = &self.status {
            status.cells
        } else {
            return Err(Error::StatusError);
        };

        self.send_bytes(&CellBalanceState::request(Address::Host))?;
        Ok(CellBalanceState::decode(
            &self.receive_bytes(CellBalanceState::reply_size())?,
            n_cells,
        )?)
    }

    /// Retrieves a list of active error codes from the BMS.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec<ErrorCode>` of active errors or an `Error`.
    /// An empty vector means no errors are currently active.
    pub fn get_errors(&mut self) -> Result<Vec<ErrorCode>> {
        log::trace!("get errors");
        self.send_bytes(&ErrorCode::request(Address::Host))?;
        Ok(ErrorCode::decode(
            &self.receive_bytes(ErrorCode::reply_size())?,
        )?)
    }

    /// Enables or disables the discharging MOSFET.
    ///
    /// # Arguments
    ///
    /// * `enable`: Set to `true` to enable the discharging MOSFET, `false` to disable it.
    ///
    /// # Returns
    ///
    /// An empty `Result` indicating success or an `Error`.
    pub fn set_discharge_mosfet(&mut self, enable: bool) -> Result<()> {
        log::trace!("set discharge mosfet to {}", enable);
        self.send_bytes(&SetDischargeMosfet::request(Address::Host, enable))?;
        Ok(SetDischargeMosfet::decode(
            &self.receive_bytes(SetDischargeMosfet::reply_size())?,
        )?)
    }

    /// Enables or disables the charging MOSFET.
    ///
    /// # Arguments
    ///
    /// * `enable`: Set to `true` to enable the charging MOSFET, `false` to disable it.
    ///
    /// # Returns
    ///
    /// An empty `Result` indicating success or an `Error`.
    pub fn set_charge_mosfet(&mut self, enable: bool) -> Result<()> {
        log::trace!("set charge mosfet to {}", enable);
        self.send_bytes(&SetChargeMosfet::request(Address::Host, enable))?;
        Ok(SetChargeMosfet::decode(
            &self.receive_bytes(SetChargeMosfet::reply_size())?,
        )?)
    }

    /// Sets the State of Charge (SOC) percentage on the BMS.
    ///
    /// # Arguments
    ///
    /// * `soc_percent`: The desired SOC percentage (0.0 to 100.0).
    ///                  Values outside this range will be clamped by the protocol.
    ///
    /// # Returns
    ///
    /// An empty `Result` indicating success or an `Error`.
    pub fn set_soc(&mut self, soc_percent: f32) -> Result<()> {
        log::trace!("set SOC to {}", soc_percent);
        self.send_bytes(&SetSoc::request(Address::Host, soc_percent))?;
        Ok(SetSoc::decode(&self.receive_bytes(SetSoc::reply_size())?)?)
    }

    /// Resets the BMS to its factory default settings.
    ///
    /// **Use with caution!**
    ///
    /// # Returns
    ///
    /// An empty `Result` indicating success or an `Error`.
    pub fn reset(&mut self) -> Result<()> {
        log::trace!("reset to factory default settings");
        self.send_bytes(&BmsReset::request(Address::Host))?;
        Ok(BmsReset::decode(
            &self.receive_bytes(BmsReset::reply_size())?,
        )?)
    }
}
