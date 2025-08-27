//! Provides an asynchronous client for interacting with a Daly BMS (Battery Management System)
//! using Tokio and the `tokio-serial` crate for serial communication.
//!
//! This module is suitable for applications built on the Tokio runtime.
//!
//! # Example
//!
//! ```no_run
//! use dalybms_lib::tokio_serial_async::{DalyBMS, Error};
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Error> {
//!     let mut bms = DalyBMS::new("/dev/ttyUSB0")?;
//!     bms.set_timeout(Duration::from_millis(500))?;
//!
//!     let soc = bms.get_soc().await?;
//!     println!("SOC: {:?}", soc);
//!
//!     // It's recommended to call get_status() first to populate cell/sensor counts
//!     // for other methods like get_cell_voltages() or get_cell_temperatures().
//!     let status = bms.get_status().await?;
//!     println!("Status: {:?}", status);
//!
//!     let cell_voltages = bms.get_cell_voltages().await?;
//!     println!("Cell Voltages: {:?}", cell_voltages);
//!
//!     Ok(())
//! }
//! ```

use crate::protocol::*;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::{SerialPort, SerialPortBuilderExt};

/// Errors specific to the asynchronous Tokio serial port client.
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
    /// An error from the `tokio-serial` crate.
    #[error("Tokio serial error: {0}")]
    TokioSerial(#[from] tokio_serial::Error),
    /// An error indicating that a Tokio timeout elapsed during an I/O operation.
    #[error("Tokio timeout elapsed: {0}")]
    TokioElapsed(#[from] tokio::time::error::Elapsed),
}

/// A specialized `Result` type for operations within the `tokio_serial_async` module.
type Result<T> = std::result::Result<T, Error>;

/// The main struct for interacting asynchronously with a Daly BMS using Tokio.
///
/// It handles sending commands and receiving/decoding responses from the BMS
/// in an asynchronous manner, suitable for Tokio-based applications.
/// Most methods are `async` and require a mutable reference to `self`.
#[derive(Debug)]
pub struct DalyBMS {
    serial: tokio_serial::SerialStream,
    last_execution: Instant,
    io_timeout: Duration,   // Timeout for individual I/O operations
    delay: Duration,        // Delay between commands
    status: Option<Status>, // Stores the latest status
    retries: u8,
}

macro_rules! request_with_retry {
    ($self:ident, $X:ident, $request_bytes:expr, $reply_size:expr) => {{
        'retry_block: {
            for t in 0..$self.retries {
                match $self.send_and_receive($request_bytes, $reply_size).await {
                    Ok(reply_bytes) => match $X::decode(&reply_bytes) {
                        Ok(result) => break 'retry_block Ok(result),
                        Err(err) => {
                            log::trace!(
                                "Failed try {} of {}, repeating ({err})",
                                t + 1,
                                $self.retries
                            );
                        }
                    },
                    Err(err) => {
                        log::trace!(
                            "Failed try {} of {}, repeating ({err})",
                            t + 1,
                            $self.retries
                        );
                    }
                }
            }
            Ok($X::decode(
                &$self.send_and_receive($request_bytes, $reply_size).await?,
            )?)
        }
    }};

    ($self:ident, $X:ident, $request_bytes:expr, $reply_size:expr, $decode_arg:expr) => {{
        'retry_block: {
            for t in 0..$self.retries {
                match $self.send_and_receive($request_bytes, $reply_size).await {
                    Ok(reply_bytes) => match $X::decode(&reply_bytes, $decode_arg) {
                        Ok(result) => break 'retry_block Ok(result),
                        Err(err) => {
                            log::trace!(
                                "Failed try {} of {}, repeating ({err})",
                                t + 1,
                                $self.retries
                            );
                        }
                    },
                    Err(err) => {
                        log::trace!(
                            "Failed try {} of {}, repeating ({err})",
                            t + 1,
                            $self.retries
                        );
                    }
                }
            }
            Ok($X::decode(
                &$self.send_and_receive($request_bytes, $reply_size).await?,
                $decode_arg,
            )?)
        }
    }};
}

impl DalyBMS {
    /// Creates a new `DalyBMS` instance for asynchronous communication.
    ///
    /// # Arguments
    ///
    /// * `port`: The path to the serial port device (e.g., `/dev/ttyUSB0` on Linux, `COM3` on Windows).
    ///
    /// # Returns
    ///
    /// A `Result` containing the `DalyBMS` instance or an `Error` if the serial port
    /// cannot be opened or configured for asynchronous operation.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use dalybms_lib::tokio_serial_async::DalyBMS;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let bms_result = DalyBMS::new("/dev/ttyUSB0");
    ///     if let Ok(mut bms_instance) = bms_result {
    ///         // Use the BMS instance
    ///         if let Ok(soc) = bms_instance.get_soc().await {
    ///             println!("SOC: {}%", soc.soc_percent);
    ///         }
    ///     } else {
    ///         eprintln!("Failed to connect to BMS: {:?}", bms_result.err());
    ///     }
    /// }
    /// ```
    pub fn new(port: &str) -> Result<Self> {
        Ok(Self {
            serial: tokio_serial::new(port, 9600)
                .data_bits(tokio_serial::DataBits::Eight)
                .parity(tokio_serial::Parity::None)
                .stop_bits(tokio_serial::StopBits::One)
                .flow_control(tokio_serial::FlowControl::None)
                .open_native_async()?,
            last_execution: Instant::now(),
            delay: MINIMUM_DELAY, // Default delay from protocol module
            io_timeout: Duration::from_secs(5), // Default I/O timeout
            status: None,
            retries: 3,
        })
    }

    /// sets the number of retries for a failed send_bytes operation
    pub fn set_retry(&mut self, n_retries: u8) {
        self.retries = n_retries;
    }

    /// Asynchronously waits for the configured delay duration since the last command execution.
    /// This is a private helper to ensure commands are not sent too frequently.
    async fn serial_await_delay(&self) {
        let last_exec_diff = Instant::now().duration_since(self.last_execution);
        if let Some(time_until_delay_reached) = self.delay.checked_sub(last_exec_diff) {
            tokio::time::sleep(time_until_delay_reached).await;
        }
    }

    /// Private async helper to send bytes to the serial port.
    /// It handles clearing pending data, awaiting delay, and writing the buffer with timeouts.
    async fn send_bytes(&mut self, tx_buffer: &[u8]) -> Result<()> {
        // Before sending a new command, it's crucial to clear any lingering data
        // in the serial port's read buffer. This prevents a scenario where a previous,
        // timed-out response could be misinterpreted as the response to the current command.
        loop {
            log::trace!("read to see if there is any pending data");
            let pending = self.serial.bytes_to_read()?;
            log::trace!("got {pending} pending bytes");
            if pending > 0 {
                let mut buf: Vec<u8> = vec![0; 64]; // Temporary buffer to drain
                let received =
                    tokio::time::timeout(self.io_timeout, self.serial.read(buf.as_mut_slice()))
                        .await??;
                log::trace!("{received} pending bytes consumed");
            } else {
                break;
            }
        }
        self.serial_await_delay().await;

        log::trace!("write bytes: {tx_buffer:02X?}");
        tokio::time::timeout(self.io_timeout, self.serial.write_all(tx_buffer)).await??;

        // Flushing is usually not necessary and can sometimes cause issues.
        if false {
            // Disabled by default
            log::trace!("flush connection");
            tokio::time::timeout(self.io_timeout, self.serial.flush()).await??;
        }
        Ok(())
    }

    /// Private async helper to receive a specified number of bytes from the serial port with timeouts.
    async fn receive_bytes(&mut self, size: usize) -> Result<Vec<u8>> {
        let mut rx_buffer = vec![0; size];

        log::trace!("read {size} bytes");
        tokio::time::timeout(self.io_timeout, self.serial.read_exact(&mut rx_buffer)).await??;

        self.last_execution = Instant::now(); // Update last execution time

        log::trace!("receive_bytes: {rx_buffer:02X?}");
        Ok(rx_buffer)
    }

    async fn send_and_receive(&mut self, tx_buffer: &[u8], reply_size: usize) -> Result<Vec<u8>> {
        self.send_bytes(tx_buffer).await?;
        self.receive_bytes(reply_size).await
    }

    /// Sets the timeout for individual I/O operations (read/write) on the serial port.
    ///
    /// # Arguments
    ///
    /// * `timeout`: The duration to wait for an I/O operation before timing out.
    ///
    /// # Returns
    ///
    /// A `Result` indicating success. This operation currently always succeeds.
    pub fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
        log::trace!("set timeout to {timeout:?}");
        self.io_timeout = timeout;
        Ok(())
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
            log::warn!("delay {delay:?} lower minimum {MINIMUM_DELAY:?}, use minimum");
            self.delay = MINIMUM_DELAY;
        } else {
            self.delay = delay;
        }
        log::trace!("set delay to {:?}", self.delay);
    }

    /// Asynchronously retrieves the State of Charge (SOC) and other primary battery metrics.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `Soc` data or an `Error` if the command fails,
    /// decoding is unsuccessful, or a timeout occurs.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use dalybms_lib::tokio_serial_async::{DalyBMS, Error};
    /// # use std::time::Duration;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Error> {
    /// # let mut bms = DalyBMS::new("/dev/ttyUSB0")?;
    /// let soc_data = bms.get_soc().await?;
    /// println!("Voltage: {:.1}V, Current: {:.1}A, SOC: {:.1}%",
    ///          soc_data.total_voltage, soc_data.current, soc_data.soc_percent);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_soc(&mut self) -> Result<Soc> {
        log::trace!("get SOC");
        request_with_retry!(self, Soc, &Soc::request(Address::Host), Soc::reply_size())
    }

    /// Asynchronously retrieves the highest and lowest cell voltages in the battery pack.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `CellVoltageRange` data or an `Error`.
    pub async fn get_cell_voltage_range(&mut self) -> Result<CellVoltageRange> {
        log::trace!("get cell voltage range");
        request_with_retry!(
            self,
            CellVoltageRange,
            &CellVoltageRange::request(Address::Host),
            CellVoltageRange::reply_size()
        )
    }

    /// Asynchronously retrieves the highest and lowest temperatures measured by the BMS.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `TemperatureRange` data or an `Error`.
    pub async fn get_temperature_range(&mut self) -> Result<TemperatureRange> {
        log::trace!("get temperature range");
        request_with_retry!(
            self,
            TemperatureRange,
            &TemperatureRange::request(Address::Host),
            TemperatureRange::reply_size()
        )
    }

    /// Asynchronously retrieves the status of the charging and discharging MOSFETs, and other related data.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `MosfetStatus` data or an `Error`.
    pub async fn get_mosfet_status(&mut self) -> Result<MosfetStatus> {
        log::trace!("get mosfet status");
        request_with_retry!(
            self,
            MosfetStatus,
            &MosfetStatus::request(Address::Host),
            MosfetStatus::reply_size()
        )
    }

    /// Asynchronously retrieves general status information from the BMS, including cell count and temperature sensor count.
    ///
    /// This method also caches the retrieved status internally, as this information is
    /// required by other methods like `get_cell_voltages` and `get_cell_temperatures`.
    /// It's recommended to call this method at least once before calling those methods.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `Status` data or an `Error`.
    pub async fn get_status(&mut self) -> Result<Status> {
        log::trace!("get status");
        match request_with_retry!(
            self,
            Status,
            &Status::request(Address::Host),
            Status::reply_size()
        ) {
            Ok(status) => {
                self.status = Some(status.clone()); // Cache the status
                Ok(status)
            }
            Err(err) => Err(err),
        }
    }

    /// Asynchronously retrieves the voltage of each individual cell in the battery pack.
    ///
    /// **Note:** `get_status().await` must be called at least once before this method
    /// to determine the number of cells.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `CellVoltages` of cell voltages or an `Error`.
    /// Returns `Error::StatusError` if `get_status().await` was not called previously.
    pub async fn get_cell_voltages(&mut self) -> Result<CellVoltages> {
        log::trace!("get cell voltages");
        let n_cells = if let Some(status) = &self.status {
            status.cells
        } else {
            return Err(Error::StatusError);
        };
        request_with_retry!(
            self,
            CellVoltages,
            &CellVoltages::request(Address::Host),
            CellVoltages::reply_size(n_cells),
            n_cells
        )
    }

    /// Asynchronously retrieves the temperature from each individual temperature sensor.
    ///
    /// **Note:** `get_status().await` must be called at least once before this method
    /// to determine the number of temperature sensors.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec<i32>` of temperatures in Celsius or an `Error`.
    /// Returns `Error::StatusError` if `get_status().await` was not called previously.
    pub async fn get_cell_temperatures(&mut self) -> Result<Vec<i32>> {
        log::trace!("get cell temperatures");
        let n_sensors = if let Some(status) = &self.status {
            status.temperature_sensors
        } else {
            return Err(Error::StatusError);
        };
        request_with_retry!(
            self,
            CellTemperatures,
            &CellTemperatures::request(Address::Host),
            CellTemperatures::reply_size(n_sensors),
            n_sensors
        )
    }

    /// Asynchronously retrieves the balancing status of each individual cell.
    ///
    /// **Note:** `get_status().await` must be called at least once before this method
    /// to determine the number of cells.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec<bool>` where `true` indicates the cell is currently balancing,
    /// or an `Error`. Returns `Error::StatusError` if `get_status().await` was not called previously.
    pub async fn get_balancing_status(&mut self) -> Result<Vec<bool>> {
        log::trace!("get balancing status");
        let n_cells = if let Some(status) = &self.status {
            status.cells
        } else {
            return Err(Error::StatusError);
        };
        request_with_retry!(
            self,
            CellBalanceState,
            &CellBalanceState::request(Address::Host),
            CellBalanceState::reply_size(),
            n_cells
        )
    }

    /// Asynchronously retrieves a list of active error codes from the BMS.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec<ErrorCode>` of active errors or an `Error`.
    /// An empty vector means no errors are currently active.
    pub async fn get_errors(&mut self) -> Result<Vec<ErrorCode>> {
        log::trace!("get errors");
        request_with_retry!(
            self,
            ErrorCode,
            &ErrorCode::request(Address::Host),
            ErrorCode::reply_size()
        )
    }

    /// Asynchronously enables or disables the discharging MOSFET.
    ///
    /// # Arguments
    ///
    /// * `enable`: Set to `true` to enable the discharging MOSFET, `false` to disable it.
    ///
    /// # Returns
    ///
    /// An empty `Result` indicating success or an `Error`.
    pub async fn set_discharge_mosfet(&mut self, enable: bool) -> Result<()> {
        log::trace!("set discharge mosfet to {enable}");
        request_with_retry!(
            self,
            SetDischargeMosfet,
            &SetDischargeMosfet::request(Address::Host, enable),
            SetDischargeMosfet::reply_size()
        )
    }

    /// Asynchronously enables or disables the charging MOSFET.
    ///
    /// # Arguments
    ///
    /// * `enable`: Set to `true` to enable the charging MOSFET, `false` to disable it.
    ///
    /// # Returns
    ///
    /// An empty `Result` indicating success or an `Error`.
    pub async fn set_charge_mosfet(&mut self, enable: bool) -> Result<()> {
        log::trace!("set charge mosfet to {enable}");
        request_with_retry!(
            self,
            SetChargeMosfet,
            &SetChargeMosfet::request(Address::Host, enable),
            SetChargeMosfet::reply_size()
        )
    }

    /// Asynchronously sets the State of Charge (SOC) percentage on the BMS.
    ///
    /// # Arguments
    ///
    /// * `soc_percent`: The desired SOC percentage (0.0 to 100.0). Values outside this range will be clamped by the protocol.
    ///
    /// # Returns
    ///
    /// An empty `Result` indicating success or an `Error`.
    pub async fn set_soc(&mut self, soc_percent: f32) -> Result<()> {
        log::trace!("set SOC to {soc_percent}");
        request_with_retry!(
            self,
            SetSoc,
            &SetSoc::request(Address::Host, soc_percent),
            SetSoc::reply_size()
        )
    }

    /// Asynchronously resets the BMS to its factory default settings.
    ///
    /// **Use with caution!**
    ///
    /// # Returns
    ///
    /// An empty `Result` indicating success or an `Error`.
    pub async fn reset(&mut self) -> Result<()> {
        log::trace!("reset to factory default settings");
        request_with_retry!(
            self,
            BmsReset,
            &BmsReset::request(Address::Host),
            BmsReset::reply_size()
        )
    }
}
