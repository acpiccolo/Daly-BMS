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
    #[error("Serialport error: {0}")]
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
    retries: u8,
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
            retries: 3,
        })
    }

    /// sets the number of retries for a failed send_bytes operation
    pub fn set_retry(&mut self, n_retries: u8) {
        self.retries = n_retries;
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
        // Before sending a new command, it's crucial to clear any lingering data
        // in the serial port's read buffer. This prevents a scenario where a previous,
        // timed-out response could be misinterpreted as the response to the current command.
        loop {
            log::trace!("read to see if there is any pending data");
            let pending = self.serial.bytes_to_read()?;
            log::trace!("got {pending} pending bytes");
            if pending > 0 {
                let mut buf: Vec<u8> = vec![0; 64]; // Temporary buffer to drain
                let received = self.serial.read(buf.as_mut_slice())?;
                log::trace!("{received} pending bytes consumed");
            } else {
                break;
            }
        }
        self.serial_await_delay();

        log::trace!("write bytes: {tx_buffer:02X?}");
        self.serial.write_all(tx_buffer)?;

        // Flushing is usually not necessary for USB serial devices and can sometimes cause issues.
        // If needed, it can be enabled here.
        if false {
            // Disabled by default
            log::trace!("flush connection");
            self.serial.flush()?;
        }
        Ok(())
    }

    /// Reads exactly one 13-byte frame, first resyncing to the `0xA5` start byte
    /// (dropping any bytes before it). Resyncing per frame means a preceding
    /// stray/late frame can't leave us reading from a mid-frame offset.
    fn receive_one_frame(&mut self) -> Result<[u8; RX_BUFFER_LENGTH]> {
        let mut byte = [0u8; 1];
        loop {
            self.serial.read_exact(&mut byte)?;
            if byte[0] == START_BYTE {
                break;
            }
        }
        let mut frame = [0u8; RX_BUFFER_LENGTH];
        frame[0] = START_BYTE;
        self.serial.read_exact(&mut frame[1..])?;
        log::trace!("frame: {frame:02X?}");
        Ok(frame)
    }

    /// Receives `reply_size` bytes (a whole number of 13-byte frames) of the
    /// BMS's reply to the command we just sent, frame by frame.
    ///
    /// Some Daly units carry an on-board Bluetooth/UART module that polls the BMS
    /// over the *same* internal serial bus; its request frames (`A5 40 ..`) and
    /// the BMS's answers to *them* (`A5 01 <other-cmd> ..`) leak onto our port
    /// and, read blindly, get mis-decoded as our reply. So we keep only genuine
    /// BMS replies to the command we asked for (see [`is_reply_for`]) and skip
    /// everything else. Per-frame checksum is validated downstream in each
    /// `decode`.
    fn receive_frames(&mut self, expected_cmd: u8, reply_size: usize) -> Result<Vec<u8>> {
        let n_frames = reply_size / RX_BUFFER_LENGTH;
        let mut out = Vec::with_capacity(reply_size);
        // Bound the resync so a line flooded with foreign frames fails the read
        // (→ retry/reconnect) instead of spinning forever.
        let mut skip_budget = (n_frames + 1) * MAX_STRAY_FRAMES_PER_FRAME;
        while out.len() < n_frames * RX_BUFFER_LENGTH {
            let frame = self.receive_one_frame()?;
            if is_reply_for(&frame, expected_cmd) {
                out.extend_from_slice(&frame);
            } else {
                log::trace!(
                    "skip stray frame addr=0x{:02X} cmd=0x{:02X} (want addr=0x{BMS_ADDRESS:02X} cmd=0x{expected_cmd:02X})",
                    frame[ADDRESS_INDEX],
                    frame[COMMAND_INDEX]
                );
                skip_budget = skip_budget
                    .checked_sub(1)
                    .ok_or(Error::DalyError(crate::Error::FrameNoError))?;
            }
        }
        self.last_execution = Instant::now(); // Update last execution time after successful read
        Ok(out)
    }

    fn send_and_receive(&mut self, tx_buffer: &[u8], reply_size: usize) -> Result<Vec<u8>> {
        self.send_bytes(tx_buffer)?;
        // The command we asked for — everything not addressed from the BMS for
        // this command (stray BT-module traffic on the shared bus) is skipped.
        let expected_cmd = tx_buffer[COMMAND_INDEX];
        self.receive_frames(expected_cmd, reply_size)
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
        log::trace!("set timeout to {timeout:?}");
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
            log::warn!("delay {delay:?} lower minimum {MINIMUM_DELAY:?}, use minimum");
            self.delay = MINIMUM_DELAY;
        } else {
            self.delay = delay;
        }
        log::trace!("set delay to {:?}", self.delay);
    }

    fn request_with_retry<F, T>(
        &mut self,
        tx_buffer: &[u8],
        reply_size: usize,
        request: F,
    ) -> Result<T>
    where
        F: Fn(&mut Self, &[u8], usize) -> Result<T>,
    {
        for t in 0..self.retries {
            match request(self, tx_buffer, reply_size) {
                Ok(result) => {
                    return Ok(result);
                }
                Err(err) => {
                    log::trace!(
                        "Failed try {} of {}, repeating ({err})",
                        t + 1,
                        self.retries
                    );
                }
            }
        }
        request(self, tx_buffer, reply_size)
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
        self.request_with_retry(
            &Soc::request(Address::Host),
            Soc::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(Soc::decode(&bms.send_and_receive(tx_buffer, reply_size)?)?)
            },
        )
    }

    /// Retrieves the highest and lowest cell voltages in the battery pack.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `CellVoltageRange` data or an `Error`.
    pub fn get_cell_voltage_range(&mut self) -> Result<CellVoltageRange> {
        log::trace!("get cell voltage range");
        self.request_with_retry(
            &CellVoltageRange::request(Address::Host),
            CellVoltageRange::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(CellVoltageRange::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                )?)
            },
        )
    }

    /// Retrieves the highest and lowest temperatures measured by the BMS.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `TemperatureRange` data or an `Error`.
    pub fn get_temperature_range(&mut self) -> Result<TemperatureRange> {
        log::trace!("get temperature range");
        self.request_with_retry(
            &TemperatureRange::request(Address::Host),
            TemperatureRange::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(TemperatureRange::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                )?)
            },
        )
    }

    /// Retrieves the status of the charging and discharging MOSFETs, and other related data.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `MosfetStatus` data or an `Error`.
    pub fn get_mosfet_status(&mut self) -> Result<MosfetStatus> {
        log::trace!("get mosfet status");
        self.request_with_retry(
            &MosfetStatus::request(Address::Host),
            MosfetStatus::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(MosfetStatus::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                )?)
            },
        )
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
        self.request_with_retry(
            &Status::request(Address::Host),
            Status::reply_size(),
            |bms, tx_buffer, reply_size| {
                let status = Status::decode(&bms.send_and_receive(tx_buffer, reply_size)?)?;
                bms.status = Some(status.clone()); // Cache the status
                Ok(status)
            },
        )
    }

    /// Retrieves the voltage of each individual cell in the battery pack.
    ///
    /// **Note:** `get_status()` must be called at least once before this method
    /// to determine the number of cells.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `CellVoltages` of cell voltages or an `Error`.
    /// Returns `Error::StatusError` if `get_status()` was not called previously.
    pub fn get_cell_voltages(&mut self) -> Result<CellVoltages> {
        log::trace!("get cell voltages");
        let n_cells = if let Some(status) = &self.status {
            status.cells
        } else {
            return Err(Error::StatusError);
        };
        self.request_with_retry(
            &CellVoltages::request(Address::Host),
            CellVoltages::reply_size(n_cells),
            |bms, tx_buffer, reply_size| {
                Ok(CellVoltages::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                    n_cells,
                )?)
            },
        )
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

        self.request_with_retry(
            &CellTemperatures::request(Address::Host),
            CellTemperatures::reply_size(n_sensors),
            |bms, tx_buffer, reply_size| {
                Ok(CellTemperatures::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                    n_sensors,
                )?)
            },
        )
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

        self.request_with_retry(
            &CellBalanceState::request(Address::Host),
            CellBalanceState::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(CellBalanceState::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                    n_cells,
                )?)
            },
        )
    }

    /// Retrieves a list of active error codes from the BMS.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec<ErrorCode>` of active errors or an `Error`.
    /// An empty vector means no errors are currently active.
    pub fn get_errors(&mut self) -> Result<Vec<ErrorCode>> {
        log::trace!("get errors");
        self.request_with_retry(
            &ErrorCode::request(Address::Host),
            ErrorCode::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(ErrorCode::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                )?)
            },
        )
    }

    /// Retrieves the rated (design) capacity and nominal cell voltage.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `RatedCapacity` data or an `Error`.
    pub fn get_rated_capacity(&mut self) -> Result<RatedCapacity> {
        log::trace!("get rated capacity");
        self.request_with_retry(
            &RatedCapacity::request(Address::Host),
            RatedCapacity::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(RatedCapacity::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                )?)
            },
        )
    }

    /// Retrieves battery info: production date plus raw operating-mode, type and
    /// auto-sleep bytes.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `BatteryInfo` data or an `Error`.
    pub fn get_battery_info(&mut self) -> Result<BatteryInfo> {
        log::trace!("get battery info");
        self.request_with_retry(
            &BatteryInfo::request(Address::Host),
            BatteryInfo::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(BatteryInfo::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                )?)
            },
        )
    }

    /// Retrieves the battery code / serial string (fixed 5-frame reply).
    ///
    /// # Returns
    ///
    /// A `Result` containing the `BatteryCode` (dereferences to `&str`) or an `Error`.
    pub fn get_battery_code(&mut self) -> Result<BatteryCode> {
        log::trace!("get battery code");
        self.request_with_retry(
            &BatteryCode::request(Address::Host),
            BatteryCode::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(BatteryCode::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                )?)
            },
        )
    }

    /// Retrieves the lifetime accumulated charge/discharge Ah (`0x52`).
    pub fn get_battery_stat(&mut self) -> Result<BatteryStat> {
        log::trace!("get battery stat");
        self.request_with_retry(
            &BatteryStat::request(Address::Host),
            BatteryStat::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(BatteryStat::decode(&bms.send_and_receive(tx_buffer, reply_size)?)?)
            },
        )
    }

    /// Retrieves the BMS config: board / cell / NTC counts (`0x51`).
    pub fn get_bms_config(&mut self) -> Result<BmsConfig> {
        log::trace!("get bms config");
        self.request_with_retry(
            &BmsConfig::request(Address::Host),
            BmsConfig::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(BmsConfig::decode(&bms.send_and_receive(tx_buffer, reply_size)?)?)
            },
        )
    }

    /// Retrieves the BMS real-time clock (`0x61`).
    pub fn get_rtc(&mut self) -> Result<Rtc> {
        log::trace!("get rtc");
        self.request_with_retry(
            &Rtc::request(Address::Host),
            Rtc::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(Rtc::decode(&bms.send_and_receive(tx_buffer, reply_size)?)?)
            },
        )
    }

    /// Retrieves the software/master version string (`0x62`).
    pub fn get_software_version(&mut self) -> Result<SoftwareVersion> {
        log::trace!("get software version");
        self.request_with_retry(
            &SoftwareVersion::request(Address::Host),
            SoftwareVersion::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(SoftwareVersion::decode(&bms.send_and_receive(tx_buffer, reply_size)?)?)
            },
        )
    }

    /// Retrieves the hardware version string (`0x63`).
    pub fn get_hardware_version(&mut self) -> Result<HardwareVersion> {
        log::trace!("get hardware version");
        self.request_with_retry(
            &HardwareVersion::request(Address::Host),
            HardwareVersion::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(HardwareVersion::decode(&bms.send_and_receive(tx_buffer, reply_size)?)?)
            },
        )
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
        log::trace!("set discharge mosfet to {enable}");
        self.request_with_retry(
            &SetDischargeMosfet::request(Address::Host, enable),
            SetDischargeMosfet::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(SetDischargeMosfet::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                )?)
            },
        )
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
        log::trace!("set charge mosfet to {enable}");
        self.request_with_retry(
            &SetChargeMosfet::request(Address::Host, enable),
            SetChargeMosfet::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(SetChargeMosfet::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                )?)
            },
        )
    }

    /// Sets the State of Charge (SOC) percentage on the BMS.
    ///
    /// # Arguments
    ///
    /// * `soc_percent`: The desired SOC percentage (0.0 to 100.0). Values outside this range will be clamped by the protocol.
    ///
    /// # Returns
    ///
    /// An empty `Result` indicating success or an `Error`.
    pub fn set_soc(&mut self, soc_percent: f32) -> Result<()> {
        log::trace!("set SOC to {soc_percent}");
        self.request_with_retry(
            &SetSoc::request(Address::Host, soc_percent),
            SetSoc::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(SetSoc::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                )?)
            },
        )
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
        self.request_with_retry(
            &BmsReset::request(Address::Host),
            BmsReset::reply_size(),
            |bms, tx_buffer, reply_size| {
                Ok(BmsReset::decode(
                    &bms.send_and_receive(tx_buffer, reply_size)?,
                )?)
            },
        )
    }
}
