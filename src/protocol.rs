//! # Daly BMS Protocol Implementation
//!
//! This module provides the low-level implementation for the Daly BMS (Battery Management System)
//! communication protocol. It defines the structure of commands and the logic for encoding
//! requests and decoding responses.
//!
//! The module is organized around specific BMS commands, with each command typically represented
//! by a struct or enum. For example, the `Soc` struct is used for requesting and decoding
//! the State of Charge, voltage, and current.
//!
//! ## Key Features:
//!
//! - **Command-specific Structs/Enums**: Each BMS command (e.g., `get_soc`, `get_status`) has a
//!   corresponding type that encapsulates its request and response logic.
//! - **Request Generation**: Each command type provides a `request` function that constructs the
//!   byte frame to be sent to the BMS.
//! - **Response Decoding**: Each command type provides a `decode` function that parses the
//!   response byte frame from the BMS into a structured format.
//! - **Checksum Calculation**: Includes helpers for calculating and validating the checksum
//!   required by the Daly protocol.
//! - **Error Handling**: Defines protocol-specific error conditions, such as checksum mismatches
//!   or invalid response lengths.
//!
//! ## For End-Users:
//!
//! This module is intended for internal use by the higher-level client implementations
//! (e.g., `dalybms_lib::serialport` and `dalybms_lib::tokio_serial_async`). Most users should
//! not need to interact with this module directly. The clients provide a more ergonomic,
//! high-level API for interacting with the BMS.
//!
use crate::Error;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "serde")]
mod util {
    use serde::{Serializer, ser::SerializeSeq};

    pub fn f32_1_digits<S>(x: &f32, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_f64((*x as f64 * 10.0).round() / 10.0)
    }

    pub fn f32_3_digits<S>(x: &f32, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_f64((*x as f64 * 1000.0).round() / 1000.0)
    }

    pub fn vec_f32_3_digits<S>(vec: &Vec<f32>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = s.serialize_seq(Some(vec.len()))?;
        for e in vec {
            let val = (*e as f64 * 1000.0).round() / 1000.0;
            seq.serialize_element(&val)?;
        }
        seq.end()
    }
}

/// Represents the sender/receiver address in a BMS command.
/// Currently, only the Host address is defined, as the BMS address can vary.
#[derive(Debug)]
#[repr(u8)]
pub enum Address {
    /// Address of the host (e.g., your computer).
    Host = 0x40,
    // Note: BMS address (typically 0x80) is omitted here as it's the default
    // address for sending commands and not explicitly part of the `Address` enum
    // when constructing requests from the host perspective.
}

// https://minimalmodbus.readthedocs.io/en/stable/serialcommunication.html#timing-of-the-serial-communications
// minimum delay 4ms by baud rate 9600
/// Minimum delay required between sending commands to the BMS.
/// This is to prevent overwhelming the BMS with requests.
pub const MINIMUM_DELAY: std::time::Duration = std::time::Duration::from_millis(4);

/// The required length of a request sent to the BMS.
const TX_BUFFER_LENGTH: usize = 13;
/// The expected length of a standard response from the BMS.
const RX_BUFFER_LENGTH: usize = 13;
/// The start byte that begins every command.
const START_BYTE: u8 = 0xa5;
/// The length of the data payload in a standard command.
const DATA_LENGTH: u8 = 0x08;

/// Creates the basic structure of a request frame.
///
/// This function initializes a 13-byte vector and populates the header
/// with the start byte, address, command, and data length. The data
/// payload and checksum are left as zeros and must be populated later.
///
/// # Arguments
///
/// * `address` - The `Address` to which the command is being sent.
/// * `command` - The command code (e.g., 0x90 for SOC).
///
/// # Returns
///
/// A `Vec<u8>` of length `TX_BUFFER_LENGTH` with the header fields set.
fn create_request_header(address: Address, command: u8) -> Vec<u8> {
    let mut tx_buffer = vec![0; TX_BUFFER_LENGTH];
    tx_buffer[0] = START_BYTE;
    tx_buffer[1] = address as u8;
    tx_buffer[2] = command;
    tx_buffer[3] = DATA_LENGTH;
    tx_buffer
}

/// Calculates the checksum for a given buffer.
/// The checksum is the sum of all bytes in the buffer up to, but not including,
/// the last byte, which is reserved for the checksum itself.
fn calc_crc(buffer: &[u8]) -> u8 {
    let mut checksum: u8 = 0;
    let slice = &buffer[0..buffer.len() - 1];
    for b in slice {
        checksum = checksum.wrapping_add(*b);
    }
    checksum
}

/// Calculates and sets the checksum on the last byte of a mutable buffer.
fn calc_crc_and_set(buffer: &mut [u8]) {
    let len = buffer.len();
    buffer[len - 1] = calc_crc(buffer)
}

/// A macro to read a specific bit from a byte.
/// Returns `true` if the bit at `position` is 1, `false` otherwise.
macro_rules! read_bit {
    ($byte:expr,$position:expr) => {
        ($byte >> $position) & 1 != 0
    };
}

/// Validates that the received buffer has at least the expected length.
///
/// # Arguments
///
/// * `buffer` - The byte slice received from the BMS.
/// * `expected_size` - The minimum required length for the buffer.
///
/// # Returns
///
/// An empty `Result` on success, or an `Error::ReplySizeError` if validation fails.
fn validate_len(buffer: &[u8], expected_size: usize) -> std::result::Result<(), Error> {
    if buffer.len() < expected_size {
        log::warn!(
            "Invalid buffer size - required={} received={}",
            expected_size,
            buffer.len()
        );
        return Err(Error::ReplySizeError);
    }
    Ok(())
}

/// Validates that the checksum of the received buffer is correct.
///
/// # Arguments
///
/// * `buffer` - The byte slice received from the BMS.
///
/// # Returns
///
/// An empty `Result` on success, or an `Error::CheckSumError` if validation fails.
fn validate_checksum(buffer: &[u8]) -> std::result::Result<(), Error> {
    let checksum = calc_crc(buffer);
    if buffer[buffer.len() - 1] != checksum {
        log::warn!(
            "Invalid checksum - calculated={:02X?} received={:02X?} buffer={:?}",
            checksum,
            buffer[buffer.len() - 1],
            buffer
        );
        return Err(Error::CheckSumError);
    }
    Ok(())
}

/// Represents the State of Charge (SOC) and related battery metrics.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Soc {
    /// Total battery voltage in Volts.
    #[cfg_attr(feature = "serde", serde(serialize_with = "util::f32_3_digits"))]
    pub total_voltage: f32,
    /// Battery current in Amperes.
    /// Negative values indicate charging, positive values indicate discharging.
    #[cfg_attr(feature = "serde", serde(serialize_with = "util::f32_3_digits"))]
    pub current: f32,
    /// State of Charge percentage (0.0 - 100.0%).
    #[cfg_attr(feature = "serde", serde(serialize_with = "util::f32_1_digits"))]
    pub soc_percent: f32,
}

impl Soc {
    /// Creates a request frame to read the SOC from the BMS.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods like `DalyBMSSerial::get_soc`.
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x90);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Expected size of the reply frame for an SOC request.
    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    /// Decodes the SOC data from a response frame.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The response frame received from the BMS.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `Soc` data or an `Error` if decoding fails (e.g., checksum error, incorrect size).
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<Self, Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)?;
        Ok(Self {
            total_voltage: u16::from_be_bytes([rx_buffer[4], rx_buffer[5]]) as f32 / 10.0,
            // The current measurement is given with a 30000 unit offset (see /docs/)
            current: (((u16::from_be_bytes([rx_buffer[8], rx_buffer[9]]) as i32) - 30000) as f32)
                / 10.0,
            soc_percent: u16::from_be_bytes([rx_buffer[10], rx_buffer[11]]) as f32 / 10.0,
        })
    }
}

/// Represents the range of cell voltages (highest and lowest) in the battery pack.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CellVoltageRange {
    /// Highest cell voltage in Volts.
    #[cfg_attr(feature = "serde", serde(serialize_with = "util::f32_3_digits"))]
    pub highest_voltage: f32,
    /// Cell number with the highest voltage.
    pub highest_cell: u8,
    /// Lowest cell voltage in Volts.
    #[cfg_attr(feature = "serde", serde(serialize_with = "util::f32_3_digits"))]
    pub lowest_voltage: f32,
    /// Cell number with the lowest voltage.
    pub lowest_cell: u8,
}

impl CellVoltageRange {
    /// Creates a request frame to read the cell voltage range from the BMS.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x91);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Expected size of the reply frame for a cell voltage range request.
    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    /// Decodes the cell voltage range data from a response frame.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The response frame received from the BMS.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `CellVoltageRange` data or an `Error` if decoding fails.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<Self, Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)?;
        Ok(Self {
            highest_voltage: u16::from_be_bytes([rx_buffer[4], rx_buffer[5]]) as f32 / 1000.0,
            highest_cell: rx_buffer[6],
            lowest_voltage: u16::from_be_bytes([rx_buffer[7], rx_buffer[8]]) as f32 / 1000.0,
            lowest_cell: rx_buffer[9],
        })
    }
}

/// Represents the range of temperatures (highest and lowest) measured by the BMS.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TemperatureRange {
    /// Highest temperature in degrees Celsius.
    pub highest_temperature: i8,
    /// Sensor number that detected the highest temperature.
    pub highest_sensor: u8,
    /// Lowest temperature in degrees Celsius.
    pub lowest_temperature: i8,
    /// Sensor number that detected the lowest temperature.
    pub lowest_sensor: u8,
}

impl TemperatureRange {
    /// Creates a request frame to read the temperature range from the BMS.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x92);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Expected size of the reply frame for a temperature range request.
    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    /// Decodes the temperature range data from a response frame.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The response frame received from the BMS.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `TemperatureRange` data or an `Error` if decoding fails.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<Self, Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)?;
        // An offset of 40 is added by the BMS to avoid having to deal with negative numbers, see protocol in /docs/
        Ok(Self {
            highest_temperature: ((rx_buffer[4] as i16) - 40) as i8,
            highest_sensor: rx_buffer[5],
            lowest_temperature: ((rx_buffer[6] as i16) - 40) as i8,
            lowest_sensor: rx_buffer[7],
        })
    }
}

/// Represents the operational mode of the MOSFETs.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MosfetMode {
    /// MOSFETs are stationary (neither charging nor discharging).
    Stationary,
    /// MOSFETs are in charging mode.
    Charging,
    /// MOSFETs are in discharging mode.
    Discharging,
}

/// Represents the status of the MOSFETs and battery capacity.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MosfetStatus {
    /// Current operational mode of the MOSFETs.
    pub mode: MosfetMode,
    /// True if the charging MOSFET is enabled.
    pub charging_mosfet: bool,
    /// True if the discharging MOSFET is enabled.
    pub discharging_mosfet: bool,
    /// Number of BMS cycles (e.g., charge/discharge cycles).
    pub bms_cycles: u8,
    /// Remaining battery capacity in Ampere-hours (Ah).
    #[cfg_attr(feature = "serde", serde(serialize_with = "util::f32_3_digits"))]
    pub capacity_ah: f32,
}

impl MosfetStatus {
    /// Creates a request frame to read the MOSFET status from the BMS.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x93);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Expected size of the reply frame for a MOSFET status request.
    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    /// Decodes the MOSFET status data from a response frame.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The response frame received from the BMS.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `MosfetStatus` data or an `Error` if decoding fails.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<Self, Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)?;
        let mode = match rx_buffer[4] {
            0 => MosfetMode::Stationary,
            1 => MosfetMode::Charging,
            2 => MosfetMode::Discharging,
            _ => unreachable!(),
        };
        Ok(Self {
            mode,
            charging_mosfet: rx_buffer[5] != 0,
            discharging_mosfet: rx_buffer[6] != 0,
            bms_cycles: rx_buffer[7],
            capacity_ah: u32::from_be_bytes([
                rx_buffer[8],
                rx_buffer[9],
                rx_buffer[10],
                rx_buffer[11],
            ]) as f32
                / 1000.0,
        })
    }
}

/// Represents the state of digital inputs (DI) and digital outputs (DO).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IOState {
    /// State of digital input 1.
    pub di1: bool,
    /// State of digital input 2.
    pub di2: bool,
    /// State of digital input 3.
    pub di3: bool,
    /// State of digital input 4.
    pub di4: bool,
    /// State of digital output 1.
    pub do1: bool,
    /// State of digital output 2.
    pub do2: bool,
    /// State of digital output 3.
    pub do3: bool,
    /// State of digital output 4.
    pub do4: bool,
}

/// Represents various status information of the BMS.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Status {
    /// Number of battery cells.
    pub cells: u8,
    /// Number of temperature sensors.
    pub temperature_sensors: u8,
    /// True if the charger is currently running.
    pub charger_running: bool,
    /// True if a load is currently connected and drawing power.
    pub load_running: bool,
    /// State of digital inputs and outputs.
    pub states: IOState,
    /// Number of charge/discharge cycles.
    pub cycles: u16,
}

impl Status {
    /// Creates a request frame to read the BMS status from the BMS.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x94);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Expected size of the reply frame for a BMS status request.
    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    /// Decodes the BMS status data from a response frame.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The response frame received from the BMS.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `Status` data or an `Error` if decoding fails.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<Self, Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)?;
        Ok(Self {
            cells: rx_buffer[4],
            temperature_sensors: rx_buffer[5],
            charger_running: rx_buffer[6] != 0,
            load_running: rx_buffer[7] != 0,
            states: IOState {
                di1: read_bit!(rx_buffer[8], 0),
                di2: read_bit!(rx_buffer[8], 1),
                di3: read_bit!(rx_buffer[8], 2),
                di4: read_bit!(rx_buffer[8], 3),
                do1: read_bit!(rx_buffer[8], 4),
                do2: read_bit!(rx_buffer[8], 5),
                do3: read_bit!(rx_buffer[8], 6),
                do4: read_bit!(rx_buffer[8], 7),
            },
            cycles: u16::from_be_bytes([rx_buffer[9], rx_buffer[10]]),
        })
    }
}

/// Represents a command to request individual cell voltages.
/// The BMS returns cell voltages in multiple frames.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CellVoltages(
    #[cfg_attr(feature = "serde", serde(serialize_with = "util::vec_f32_3_digits"))] Vec<f32>,
);

impl CellVoltages {
    /// Creates a request frame to read individual cell voltages from the BMS.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x95);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Calculates the number of frames expected for the given number of cells.
    fn n_frames(n_cells: u8) -> usize {
        (n_cells as f32 / 3.0).ceil() as usize
    }

    /// Calculates the total expected reply size in bytes for all frames for a given number of cells.
    ///
    /// # Arguments
    ///
    /// * `n_cells` - The number of cells in the battery pack.
    pub fn reply_size(n_cells: u8) -> usize {
        Self::n_frames(n_cells) * RX_BUFFER_LENGTH
    }

    /// Decodes the individual cell voltage data from a concatenated multi-frame response.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The concatenated response frames received from the BMS.
    /// * `n_cells` - The total number of cells expected.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec<f32>` of cell voltages or an `Error` if decoding fails.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8], n_cells: u8) -> std::result::Result<Self, Error> {
        validate_len(rx_buffer, Self::reply_size(n_cells))?;
        let mut voltages = Vec::with_capacity(n_cells as usize);
        let mut n_cell = 1;

        for n_frame in 1..=Self::n_frames(n_cells) {
            let part =
                &rx_buffer[((n_frame - 1) * RX_BUFFER_LENGTH)..((n_frame) * RX_BUFFER_LENGTH)];
            if n_frame != usize::from(part[4]) {
                log::warn!(
                    "Frame out of order - expected={} received={}",
                    n_frame,
                    part[4]
                );
                return Err(Error::FrameNoError);
            }
            validate_checksum(part)?;
            for i in 0..3 {
                let volt = u16::from_be_bytes([part[5 + i + i], part[6 + i + i]]) as f32 / 1000.0;
                log::trace!("Frame #{n_frame} cell #{n_cell} volt={volt}");
                voltages.push(volt);
                n_cell += 1;
                if n_cell > n_cells {
                    break;
                }
            }
        }
        Ok(Self(voltages))
    }
}

impl std::ops::Deref for CellVoltages {
    type Target = [f32];

    fn deref(&self) -> &[f32] {
        &self.0
    }
}

/// Represents a command to request individual cell temperatures.
/// The BMS returns cell temperatures in multiple frames.
pub struct CellTemperatures;

impl CellTemperatures {
    /// Creates a request frame to read individual cell temperatures from the BMS.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x96);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Calculates the number of frames expected for the given number of temperature sensors.
    fn n_frames(n_sensors: u8) -> usize {
        (n_sensors as f32 / 7.0).ceil() as usize
    }

    /// Calculates the total expected reply size in bytes for all frames for a given number of temperature sensors.
    ///
    /// # Arguments
    ///
    /// * `n_sensors` - The number of temperature sensors in the battery pack.
    pub fn reply_size(n_sensors: u8) -> usize {
        Self::n_frames(n_sensors) * RX_BUFFER_LENGTH
    }

    /// Decodes the individual cell temperature data from a concatenated multi-frame response.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The concatenated response frames received from the BMS.
    /// * `n_sensors` - The total number of temperature sensors expected.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec<i32>` of cell temperatures in degrees Celsius or an `Error` if decoding fails.
    /// Note that the BMS adds an offset of 40 to the temperature values, which is handled by this function.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8], n_sensors: u8) -> std::result::Result<Vec<i32>, Error> {
        validate_len(rx_buffer, Self::reply_size(n_sensors))?;
        let mut result = Vec::with_capacity(n_sensors as usize);
        let mut n_sensor = 1;

        for n_frame in 1..=Self::n_frames(n_sensors) {
            let part =
                &rx_buffer[((n_frame - 1) * RX_BUFFER_LENGTH)..((n_frame) * RX_BUFFER_LENGTH)];
            if n_frame != usize::from(part[4]) {
                log::warn!(
                    "Frame out of order - expected={} received={}",
                    n_frame,
                    part[4]
                );
                return Err(Error::FrameNoError);
            }
            validate_checksum(part)?;
            for i in 0..7 {
                let temperature = part[5 + i] as i32 - 40;
                log::trace!("Frame #{n_frame} sensor #{n_sensor} °C={temperature}");
                result.push(temperature);
                n_sensor += 1;
                if n_sensor > n_sensors {
                    break;
                }
            }
        }
        Ok(result)
    }
}

/// Represents a command to request the balance state of individual cells.
pub struct CellBalanceState;

impl CellBalanceState {
    /// Creates a request frame to read the cell balance states from the BMS.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x97);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Expected size of the reply frame for a cell balance state request.
    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    /// Decodes the cell balance state data from a response frame.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The response frame received from the BMS.
    /// * `n_cells` - The total number of cells in the battery pack.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec<bool>` where `true` indicates the cell is balancing,
    /// or an `Error` if decoding fails.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8], n_cells: u8) -> std::result::Result<Vec<bool>, Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)?;
        let mut result = Vec::with_capacity(n_cells as usize);
        let mut n_cell = 0;
        // We expect 6 bytes response for this command
        for i in 0..6 {
            // For each bit in the byte, pull out the cell balance state boolean
            for j in 0..8 {
                result.push(read_bit!(rx_buffer[4 + i], j));
                n_cell += 1;
                if n_cell >= n_cells {
                    return Ok(result);
                }
            }
        }
        Ok(result)
    }
}

/// Represents various error codes and alarm states reported by the BMS.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ErrorCode {
    /// Cell voltage is too high (Level 1)
    #[error("Cell voltage is too high (Level 1)")]
    CellVoltHighLevel1,
    /// Cell voltage is too high (Level 2)
    #[error("Cell voltage is too high (Level 2)")]
    CellVoltHighLevel2,
    /// Cell voltage is too low (Level 1)
    #[error("Cell voltage is too low (Level 1)")]
    CellVoltLowLevel1,
    /// Cell voltage is too low (Level 2)
    #[error("Cell voltage is too low (Level 2)")]
    CellVoltLowLevel2,
    /// Total voltage is too high (Level 1)
    #[error("Total voltage is too high (Level 1)")]
    SumVoltHighLevel1,
    /// Total voltage is too high (Level 2)
    #[error("Total voltage is too high (Level 2)")]
    SumVoltHighLevel2,
    /// Total voltage is too low (Level 1)
    #[error("Total voltage is too low (Level 1)")]
    SumVoltLowLevel1,
    /// Total voltage is too low (Level 2)
    #[error("Total voltage is too low (Level 2)")]
    SumVoltLowLevel2,
    /// Charging temperature too high (Level 1)
    #[error("Charging temperature too high (Level 1)")]
    ChargeTempHighLevel1,
    /// Charging temperature too high (Level 2)
    #[error("Charging temperature too high (Level 2)")]
    ChargeTempHighLevel2,
    /// Charging temperature too low (Level 1)
    #[error("Charging temperature too low (Level 1)")]
    ChargeTempLowLevel1,
    /// Charging temperature too low (Level 2)
    #[error("Charging temperature too low (Level 2)")]
    ChargeTempLowLevel2,
    /// Discharging temperature too high (Level 1)
    #[error("Discharging temperature too high (Level 1)")]
    DischargeTempHighLevel1,
    /// Discharging temperature too high (Level 2)
    #[error("Discharging temperature too high (Level 2)")]
    DischargeTempHighLevel2,
    /// Discharging temperature too low (Level 1)
    #[error("Discharging temperature too low (Level 1)")]
    DischargeTempLowLevel1,
    /// Discharging temperature too low (Level 2)
    #[error("Discharging temperature too low (Level 2)")]
    DischargeTempLowLevel2,
    /// Charge overcurrent (Level 1)
    #[error("Charge overcurrent (Level 1)")]
    ChargeOvercurrentLevel1,
    /// Charge overcurrent (Level 2)
    #[error("Charge overcurrent (Level 2)")]
    ChargeOvercurrentLevel2,
    /// Discharge overcurrent (Level 1)
    #[error("Discharge overcurrent (Level 1)")]
    DischargeOvercurrentLevel1,
    /// Discharge overcurrent (Level 2)
    #[error("Discharge overcurrent (Level 2)")]
    DischargeOvercurrentLevel2,
    /// State of Charge (SOC) too high (Level 1)
    #[error("SOC too high (Level 1)")]
    SocHighLevel1,
    /// State of Charge (SOC) too high (Level 2)
    #[error("SOC too high (Level 2)")]
    SocHighLevel2,
    /// State of Charge (SOC) too low (Level 1)
    #[error("SOC too low (Level 1)")]
    SocLowLevel1,
    /// State of Charge (SOC) too low (Level 2)
    #[error("SOC too low (Level 2)")]
    SocLowLevel2,
    /// Excessive voltage difference between cells (Level 1)
    #[error("Excessive voltage difference between cells (Level 1)")]
    DiffVoltLevel1,
    /// Excessive voltage difference between cells (Level 2)
    #[error("Excessive voltage difference between cells (Level 2)")]
    DiffVoltLevel2,
    /// Excessive temperature difference between sensors (Level 1)
    #[error("Excessive temperature difference between sensors (Level 1)")]
    DiffTempLevel1,
    /// Excessive temperature difference between sensors (Level 2)
    #[error("Excessive temperature difference between sensors (Level 2)")]
    DiffTempLevel2,
    /// Charging MOSFET over-temperature alarm.
    #[error("Charging MOSFET temperature too high")]
    ChargeMosTempHighAlarm,
    /// Discharging MOSFET over-temperature alarm.
    #[error("Discharging MOSFET temperature too high")]
    DischargeMosTempHighAlarm,
    /// Charging MOSFET temperature sensor failure.
    #[error("Charging MOSFET temperature sensor failure")]
    ChargeMosTempSensorErr,
    /// Discharging MOSFET temperature sensor failure.
    #[error("Discharging MOSFET temperature sensor failure")]
    DischargeMosTempSensorErr,
    /// Charging MOSFET adhesion failure (stuck closed).
    #[error("Charging MOSFET adhesion failure")]
    ChargeMosAdhesionErr,
    /// Discharging MOSFET adhesion failure (stuck closed).
    #[error("Discharging MOSFET adhesion failure")]
    DischargeMosAdhesionErr,
    /// Charging MOSFET breaker failure (stuck open).
    #[error("Charging MOSFET open circuit failure")]
    ChargeMosOpenCircuitErr,
    /// Discharging MOSFET breaker failure (stuck open).
    #[error("Discharging MOSFET open circuit failure")]
    DischargeMosOpenCircuitErr,
    /// AFE (Analog Front End) acquisition chip malfunction.
    #[error("AFE acquisition chip failure")]
    AfeCollectChipErr,
    /// Monomer (cell voltage) collection circuit drop off.
    #[error("Cell voltage collection circuit failure")]
    VoltageCollectDropped,
    /// Single temperature sensor failure.
    #[error("Cell temperature sensor failure")]
    CellTempSensorErr,
    /// EEPROM storage failure.
    #[error("EEPROM storage failure")]
    EepromErr,
    /// RTC (Real-Time Clock) malfunction.
    #[error("RTC clock failure")]
    RtcErr,
    /// Pre-charge failure.
    #[error("Pre-charge failure")]
    PrechangeFailure,
    /// General communication malfunction.
    #[error("Communication failure")]
    CommunicationFailure,
    /// Internal communication module malfunction.
    #[error("Internal communication failure")]
    InternalCommunicationFailure,
    /// Current detection module failure.
    #[error("Current detection module failure")]
    CurrentModuleFault,
    /// Total voltage detection module failure.
    #[error("Total voltage detection module failure")]
    SumVoltageDetectFault,
    /// Short circuit protection failure.
    #[error("Short circuit protection failure")]
    ShortCircuitProtectFault,
    /// Low voltage condition forbids charging.
    #[error("Low voltage forbids charging")]
    LowVoltForbiddenChargeFault,
}

impl ErrorCode {
    /// Creates a request frame to read the BMS error codes.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x98);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Expected size of the reply frame for an error codes request.
    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    /// Decodes the error codes from a response frame.
    /// The BMS returns a bitmask where each bit corresponds to an error.
    /// This function returns a `Vec<ErrorCode>` containing all active errors.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The response frame received from the BMS.
    ///
    /// # Returns
    ///
    /// A `Result` containing a `Vec<ErrorCode>` of active errors or an `Error` if decoding fails.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<Vec<Self>, Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)?;
        let mut result = Vec::new();

        macro_rules! ck_and_add {
            ($byte:expr,$position:expr,$enum_type:expr) => {
                if read_bit!(rx_buffer[$byte], $position) {
                    result.push($enum_type);
                }
            };
        }

        ck_and_add!(4, 0, ErrorCode::CellVoltHighLevel1);
        ck_and_add!(4, 1, ErrorCode::CellVoltHighLevel2);
        ck_and_add!(4, 2, ErrorCode::CellVoltLowLevel1);
        ck_and_add!(4, 3, ErrorCode::CellVoltLowLevel2);
        ck_and_add!(4, 4, ErrorCode::SumVoltHighLevel1);
        ck_and_add!(4, 5, ErrorCode::SumVoltHighLevel2);
        ck_and_add!(4, 6, ErrorCode::SumVoltLowLevel1);
        ck_and_add!(4, 7, ErrorCode::SumVoltLowLevel2);

        ck_and_add!(5, 0, ErrorCode::ChargeTempHighLevel1);
        ck_and_add!(5, 1, ErrorCode::ChargeTempHighLevel2);
        ck_and_add!(5, 2, ErrorCode::ChargeTempLowLevel1);
        ck_and_add!(5, 3, ErrorCode::ChargeTempLowLevel2);
        ck_and_add!(5, 4, ErrorCode::DischargeTempHighLevel1);
        ck_and_add!(5, 5, ErrorCode::DischargeTempHighLevel2);
        ck_and_add!(5, 6, ErrorCode::DischargeTempLowLevel1);
        ck_and_add!(5, 7, ErrorCode::DischargeTempLowLevel2);

        ck_and_add!(6, 1, ErrorCode::ChargeOvercurrentLevel2);
        ck_and_add!(6, 0, ErrorCode::ChargeOvercurrentLevel1);
        ck_and_add!(6, 2, ErrorCode::DischargeOvercurrentLevel1);
        ck_and_add!(6, 3, ErrorCode::DischargeOvercurrentLevel2);
        ck_and_add!(6, 4, ErrorCode::SocHighLevel1);
        ck_and_add!(6, 5, ErrorCode::SocHighLevel2);
        ck_and_add!(6, 6, ErrorCode::SocLowLevel1);
        ck_and_add!(6, 7, ErrorCode::SocLowLevel2);

        ck_and_add!(7, 0, ErrorCode::DiffVoltLevel1);
        ck_and_add!(7, 1, ErrorCode::DiffVoltLevel2);
        ck_and_add!(7, 2, ErrorCode::DiffTempLevel1);
        ck_and_add!(7, 3, ErrorCode::DiffTempLevel2);

        ck_and_add!(8, 0, ErrorCode::ChargeMosTempHighAlarm);
        ck_and_add!(8, 1, ErrorCode::DischargeMosTempHighAlarm);
        ck_and_add!(8, 2, ErrorCode::ChargeMosTempSensorErr);
        ck_and_add!(8, 3, ErrorCode::DischargeMosTempSensorErr);
        ck_and_add!(8, 4, ErrorCode::ChargeMosAdhesionErr);
        ck_and_add!(8, 5, ErrorCode::DischargeMosAdhesionErr);
        ck_and_add!(8, 6, ErrorCode::ChargeMosOpenCircuitErr);
        ck_and_add!(8, 7, ErrorCode::DischargeMosOpenCircuitErr);

        ck_and_add!(9, 0, ErrorCode::AfeCollectChipErr);
        ck_and_add!(9, 1, ErrorCode::VoltageCollectDropped);
        ck_and_add!(9, 2, ErrorCode::CellTempSensorErr);
        ck_and_add!(9, 3, ErrorCode::EepromErr);
        ck_and_add!(9, 4, ErrorCode::RtcErr);
        ck_and_add!(9, 5, ErrorCode::PrechangeFailure);
        ck_and_add!(9, 6, ErrorCode::CommunicationFailure);
        ck_and_add!(9, 7, ErrorCode::InternalCommunicationFailure);

        ck_and_add!(10, 0, ErrorCode::CurrentModuleFault);
        ck_and_add!(10, 1, ErrorCode::SumVoltageDetectFault);
        ck_and_add!(10, 2, ErrorCode::ShortCircuitProtectFault);
        ck_and_add!(10, 3, ErrorCode::LowVoltForbiddenChargeFault);

        Ok(result)
    }
}

/// Represents a command to enable or disable the discharging MOSFET.
pub struct SetDischargeMosfet;

impl SetDischargeMosfet {
    /// Creates a request frame to set the state of the discharging MOSFET.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    /// * `enable` - `true` to enable the discharging MOSFET, `false` to disable it.
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn request(address: Address, enable: bool) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0xD9);
        if enable {
            tx_buffer[4] = 0x01;
        }
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Expected size of the reply frame for a set discharge MOSFET command.
    /// The BMS typically echoes the command or sends a status.
    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    /// Decodes the response frame for a set discharge MOSFET command.
    /// This typically just validates the checksum and length, as the BMS response might not carry specific data.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The response frame received from the BMS.
    ///
    /// # Returns
    ///
    /// An empty `Result` if successful, or an `Error` if decoding fails.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<(), Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)
    }
}

/// Represents a command to enable or disable the charging MOSFET.
pub struct SetChargeMosfet;

impl SetChargeMosfet {
    /// Creates a request frame to set the state of the charging MOSFET.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    /// * `enable` - `true` to enable the charging MOSFET, `false` to disable it.
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn request(address: Address, enable: bool) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0xDA);
        if enable {
            tx_buffer[4] = 0x01;
        }
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Expected size of the reply frame for a set charge MOSFET command.
    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    /// Decodes the response frame for a set charge MOSFET command.
    /// This typically just validates the checksum and length.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The response frame received from the BMS.
    ///
    /// # Returns
    ///
    /// An empty `Result` if successful, or an `Error` if decoding fails.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<(), Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)
    }
}

/// Represents a command to set the State of Charge (SOC) percentage.
pub struct SetSoc;

impl SetSoc {
    /// Creates a request frame to set the SOC percentage on the BMS.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    /// * `soc_percent` - The desired SOC percentage (0.0 to 100.0). Values outside this range will be clamped.
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn request(address: Address, soc_percent: f32) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x21);
        let value = {
            let val = (soc_percent * 10.0).round();
            if val > 1000.0 {
                // BMS expects value * 10, so 100.0% is 1000
                1000
            } else if val < 0.0 {
                0
            } else {
                val as u16
            }
        }
        .to_be_bytes();
        tx_buffer[10] = value[0]; // SOC high byte
        tx_buffer[11] = value[1]; // SOC low byte
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Expected size of the reply frame for a set SOC command.
    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    /// Decodes the response frame for a set SOC command.
    /// This typically just validates the checksum and length.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The response frame received from the BMS.
    ///
    /// # Returns
    ///
    /// An empty `Result` if successful, or an `Error` if decoding fails.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<(), Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)
    }
}

/// Represents a command to reset the BMS.
pub struct BmsReset;

impl BmsReset {
    /// Creates a request frame to reset the BMS.
    ///
    /// # Arguments
    ///
    /// * `address` - The address of the BMS (should be `Address::Host` when sending from host).
    ///
    /// # Returns
    ///
    /// A byte vector representing the request frame.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x00);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    /// Expected size of the reply frame for a BMS reset command.
    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    /// Decodes the response frame for a BMS reset command.
    /// This typically just validates the checksum and length.
    ///
    /// # Arguments
    ///
    /// * `rx_buffer` - The response frame received from the BMS.
    ///
    /// # Returns
    ///
    /// An empty `Result` if successful, or an `Error` if decoding fails.
    ///
    /// This is a low-level function. Users might prefer client methods.
    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<(), Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to calculate checksum for test data
    fn calculate_test_checksum(data: &[u8]) -> u8 {
        let mut checksum: u8 = 0;
        for byte in data.iter().take(data.len() - 1) {
            checksum = checksum.wrapping_add(*byte);
        }
        checksum
    }

    #[test]
    fn test_calc_crc_valid() {
        let data1: [u8; 13] = [
            0xA5, 0x40, 0x90, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]; // Placeholder CRC
        let expected_crc1 = calculate_test_checksum(&data1);
        assert_eq!(calc_crc(&data1), expected_crc1);

        let data2: [u8; 13] = [
            0xA5, 0x40, 0x90, 0x08, 0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE, 0x00,
        ]; // Placeholder CRC
        let expected_crc2 = calculate_test_checksum(&data2);
        assert_eq!(calc_crc(&data2), expected_crc2);

        let data3: [u8; 13] = [
            0xA5, 0x01, 0x02, 0x08, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x00,
        ];
        let expected_crc3 = calculate_test_checksum(&data3);
        assert_eq!(calc_crc(&data3), expected_crc3);
    }

    #[test]
    fn test_validate_checksum_valid() {
        let mut data: [u8; 13] = [
            0xA5, 0x40, 0x90, 0x08, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x00,
        ];
        data[12] = calc_crc(&data); // Set correct CRC
        assert!(validate_checksum(&data).is_ok());
    }

    #[test]
    fn test_validate_checksum_invalid() {
        let mut data: [u8; 13] = [
            0xA5, 0x40, 0x90, 0x08, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x00,
        ];
        data[12] = calc_crc(&data).wrapping_add(1); // Set incorrect CRC
        let result = validate_checksum(&data);
        assert!(result.is_err());
        match result.err().unwrap() {
            Error::CheckSumError => {} // Expected error
            _ => panic!("Unexpected error type"),
        }
    }

    #[test]
    fn test_status_decode_valid() {
        // cells = 16 (0x10), temp_sensors = 4 (0x04), charger_running = true (0x01), load_running = false (0x00)
        // states (rx_buffer[8]): DI1=false, DI2=true, DI3=false, DI4=true, DO1=false, DO2=true, DO3=false, DO4=true => 0b10101010 = 0xAA
        // cycles = 1234 (0x04D2)
        // CRC: 0xA5+0x40+0x94+0x08+0x10+0x04+0x01+0x00+0xAA+0x04+0xD2+0x00 = 790 = 0x0316 => 0x16
        let bytes: [u8; 13] = [
            0xA5, 0x40, 0x94, 0x08, 0x10, 0x04, 0x01, 0x00, 0xAA, 0x04, 0xD2, 0x00, 0x16,
        ];
        let expected_status = Status {
            cells: 16,
            temperature_sensors: 4,
            charger_running: true,
            load_running: false,
            states: IOState {
                di1: false, // bit 0 of 0xAA
                di2: true,  // bit 1 of 0xAA
                di3: false, // bit 2 of 0xAA
                di4: true,  // bit 3 of 0xAA
                do1: false, // bit 4 of 0xAA
                do2: true,  // bit 5 of 0xAA
                do3: false, // bit 6 of 0xAA
                do4: true,  // bit 7 of 0xAA
            },
            cycles: 1234,
        };
        match Status::decode(&bytes) {
            Ok(decoded) => {
                assert_eq!(decoded.cells, expected_status.cells);
                assert_eq!(
                    decoded.temperature_sensors,
                    expected_status.temperature_sensors
                );
                assert_eq!(decoded.charger_running, expected_status.charger_running);
                assert_eq!(decoded.load_running, expected_status.load_running);
                assert_eq!(decoded.states.di1, expected_status.states.di1);
                assert_eq!(decoded.states.di2, expected_status.states.di2);
                assert_eq!(decoded.states.di3, expected_status.states.di3);
                assert_eq!(decoded.states.di4, expected_status.states.di4);
                assert_eq!(decoded.states.do1, expected_status.states.do1);
                assert_eq!(decoded.states.do2, expected_status.states.do2);
                assert_eq!(decoded.states.do3, expected_status.states.do3);
                assert_eq!(decoded.states.do4, expected_status.states.do4);
                assert_eq!(decoded.cycles, expected_status.cycles);
            }
            Err(e) => panic!("Decoding failed: {:?}", e),
        }
    }

    #[test]
    fn test_mosfet_status_decode_valid() {
        // mode = Charging (1), charging_mosfet = true (1), discharging_mosfet = false (0), bms_cycles = 150 (0x96), capacity_ah = 50.123Ah (50123 -> 0x0000C3CB)
        // CRC: 0xA5+0x40+0x93+0x08+0x01+0x01+0x00+0x96+0x00+0x00+0xC3+0xCB = 934 = 0x03A6 => 0xA6
        let bytes: [u8; 13] = [
            0xA5, 0x40, 0x93, 0x08, 0x01, 0x01, 0x00, 0x96, 0x00, 0x00, 0xC3, 0xCB, 0xA6,
        ];
        let expected_status = MosfetStatus {
            mode: MosfetMode::Charging,
            charging_mosfet: true,
            discharging_mosfet: false,
            bms_cycles: 150,
            capacity_ah: 50.123,
        };
        match MosfetStatus::decode(&bytes) {
            Ok(decoded) => {
                assert!(matches!(decoded.mode, MosfetMode::Charging));
                assert_eq!(decoded.charging_mosfet, expected_status.charging_mosfet);
                assert_eq!(
                    decoded.discharging_mosfet,
                    expected_status.discharging_mosfet
                );
                assert_eq!(decoded.bms_cycles, expected_status.bms_cycles);
                assert!((decoded.capacity_ah - expected_status.capacity_ah).abs() < f32::EPSILON);
            }
            Err(e) => panic!("Decoding failed: {:?}", e),
        }
    }

    #[test]
    fn test_temperature_range_decode_valid() {
        // highest_temperature = 25C (0x41 with 40 offset), highest_sensor = 1, lowest_temperature = 10C (0x32 with 40 offset), lowest_sensor = 2
        // CRC: 0xA5+0x40+0x92+0x08+0x41+0x01+0x32+0x02+0x00+0x00+0x00+0x00 = 501 = 0x01F5 => 0xF5
        let bytes: [u8; 13] = [
            0xA5, 0x40, 0x92, 0x08, 0x41, 0x01, 0x32, 0x02, 0x00, 0x00, 0x00, 0x00, 0xF5,
        ];
        let expected_range = TemperatureRange {
            highest_temperature: 25,
            highest_sensor: 1,
            lowest_temperature: 10,
            lowest_sensor: 2,
        };
        match TemperatureRange::decode(&bytes) {
            Ok(decoded) => {
                assert_eq!(
                    decoded.highest_temperature,
                    expected_range.highest_temperature
                );
                assert_eq!(decoded.highest_sensor, expected_range.highest_sensor);
                assert_eq!(
                    decoded.lowest_temperature,
                    expected_range.lowest_temperature
                );
                assert_eq!(decoded.lowest_sensor, expected_range.lowest_sensor);
            }
            Err(e) => panic!("Decoding failed: {:?}", e),
        }
    }

    #[test]
    fn test_cell_voltage_range_decode_valid() {
        // highest_voltage = 3.456V (0x0D80), highest_cell = 1, lowest_voltage = 3.123V (0x0C33), lowest_cell = 5
        // CRC: 0xA5+0x40+0x91+0x08+0x0D+0x80+0x01+0x0C+0x33+0x05+0x00+0x00 = 592 = 0x0250 => 0x50
        let bytes: [u8; 13] = [
            0xA5, 0x40, 0x91, 0x08, 0x0D, 0x80, 0x01, 0x0C, 0x33, 0x05, 0x00, 0x00, 0x50,
        ];
        let expected_range = CellVoltageRange {
            highest_voltage: 3.456,
            highest_cell: 1,
            lowest_voltage: 3.123,
            lowest_cell: 5,
        };
        match CellVoltageRange::decode(&bytes) {
            Ok(decoded) => {
                assert!(
                    (decoded.highest_voltage - expected_range.highest_voltage).abs() < f32::EPSILON
                );
                assert_eq!(decoded.highest_cell, expected_range.highest_cell);
                assert!(
                    (decoded.lowest_voltage - expected_range.lowest_voltage).abs() < f32::EPSILON
                );
                assert_eq!(decoded.lowest_cell, expected_range.lowest_cell);
            }
            Err(e) => panic!("Decoding failed: {:?}", e),
        }
    }

    #[test]
    fn test_validate_len_valid() {
        let data: [u8; 13] = [0; 13];
        assert!(validate_len(&data, 13).is_ok());
    }

    #[test]
    fn test_validate_len_valid_larger_buffer() {
        let data: [u8; 15] = [0; 15]; // Buffer is larger than required size
        assert!(validate_len(&data, 13).is_ok());
    }

    #[test]
    fn test_validate_len_invalid() {
        let data: [u8; 12] = [0; 12];
        let result = validate_len(&data, 13);
        assert!(result.is_err());
        match result.err().unwrap() {
            Error::ReplySizeError => {} // Expected error
            _ => panic!("Unexpected error type"),
        }
    }

    #[test]
    fn test_validate_len_empty() {
        let data: [u8; 0] = [];
        let result = validate_len(&data, 13);
        assert!(result.is_err());
        match result.err().unwrap() {
            Error::ReplySizeError => {} // Expected error
            _ => panic!("Unexpected error type"),
        }
    }

    // Decode tests
    #[test]
    fn test_soc_decode_valid() {
        // total_voltage = 54.3V (0x021F), current = 2.5A (0x7549 with 30000 offset), soc_percent = 75.5% (0x02F3)
        // CRC: 0xA5+0x40+0x90+0x08+0x02+0x1F+0x00+0x00+0x75+0x49+0x02+0xF3 = 849 = 0x0351 => 0x51
        let bytes: [u8; 13] = [
            0xA5, 0x40, 0x90, 0x08, 0x02, 0x1F, 0x00, 0x00, 0x75, 0x49, 0x02, 0xF3, 0x51,
        ];
        let expected_soc = Soc {
            total_voltage: 54.3,
            current: 2.5,
            soc_percent: 75.5,
        };
        match Soc::decode(&bytes) {
            Ok(decoded) => {
                assert!((decoded.total_voltage - expected_soc.total_voltage).abs() < f32::EPSILON);
                assert!((decoded.current - expected_soc.current).abs() < f32::EPSILON);
                assert!((decoded.soc_percent - expected_soc.soc_percent).abs() < f32::EPSILON);
            }
            Err(e) => panic!("Decoding failed: {:?}", e),
        }
    }

    #[test]
    fn test_soc_decode_negative_current() {
        // total_voltage = 50.0V (0x01F4), current = -10.0A (0x74CC from 29900, since (29900-30000)/10 = -10), soc_percent = 50.0% (0x01F4)
        // CRC: 0xA5+0x40+0x90+0x08+0x01+0xF4+0x00+0x00+0x74+0x9C+0x01+0xF4 = 801 = 0x0321 => 0xA7
        let bytes: [u8; 13] = [
            0xA5, 0x40, 0x90, 0x08, 0x01, 0xF4, 0x00, 0x00, 0x74, 0xCC, 0x01, 0xF4, 0xA7,
        ];
        let expected_soc = Soc {
            total_voltage: 50.0,
            current: -10.0,
            soc_percent: 50.0,
        };
        match Soc::decode(&bytes) {
            Ok(decoded) => {
                assert!((decoded.total_voltage - expected_soc.total_voltage).abs() < f32::EPSILON);
                assert!((decoded.current - expected_soc.current).abs() < f32::EPSILON);
                assert!((decoded.soc_percent - expected_soc.soc_percent).abs() < f32::EPSILON);
            }
            Err(e) => panic!("Decoding failed: {:?}", e),
        }
    }

    #[test]
    fn test_soc_decode_invalid_checksum() {
        let bytes: [u8; 13] = [
            0xA5, 0x40, 0x90, 0x08, 0x02, 0x1F, 0x00, 0x00, 0x75, 0x49, 0x02, 0xF3, 0x52,
        ]; // Incorrect CRC (0x51 is correct)
        let result = Soc::decode(&bytes);
        assert!(result.is_err());
        match result.err().unwrap() {
            Error::CheckSumError => {} // Expected
            e => panic!("Unexpected error type: {:?}", e),
        }
    }

    #[test]
    fn test_soc_decode_invalid_len() {
        let bytes: [u8; 12] = [
            0xA5, 0x40, 0x90, 0x08, 0x02, 0x1F, 0x00, 0x00, 0x75, 0x49, 0x02, 0xF3,
        ]; // Missing CRC byte
        let result = Soc::decode(&bytes);
        assert!(result.is_err());
        match result.err().unwrap() {
            Error::ReplySizeError => {} // Expected
            e => panic!("Unexpected error type: {:?}", e),
        }
    }

    #[test]
    fn test_cell_voltages_decode_valid_multi_frame() {
        // n_cells = 4, so 2 frames.
        // Frame 1: Cell1=3.300V (0x0CE4), Cell2=3.301V (0x0CE5), Cell3=3.302V (0x0CE6)
        // Frame 1 Bytes: [0xA5, 0x40, 0x95, 0x08, 0x01, 0x0C, 0xE4, 0x0C, 0xE5, 0x0C, 0xE6, 0x00, CRC1]
        // CRC1: 0xA5+0x40+0x95+0x08+0x01+0x0C+0xE4+0x0C+0xE5+0x0C+0xE6+0x00 = 1110 = 0x0456 => 0x56
        let frame1: [u8; 13] = [
            0xA5, 0x40, 0x95, 0x08, 0x01, 0x0C, 0xE4, 0x0C, 0xE5, 0x0C, 0xE6, 0x00, 0x56,
        ];

        // Frame 2: Cell4=3.303V (0x0CE7)
        // Frame 2 Bytes: [0xA5, 0x40, 0x95, 0x08, 0x02, 0x0C, 0xE7, 0x00, 0x00, 0x00, 0x00, 0x00, CRC2]
        // CRC2: 0xA5+0x40+0x95+0x08+0x02+0x0C+0xE7+0x00+0x00+0x00+0x00+0x00 = 631 = 0x0277 => 0x77
        let frame2: [u8; 13] = [
            0xA5, 0x40, 0x95, 0x08, 0x02, 0x0C, 0xE7, 0x00, 0x00, 0x00, 0x00, 0x00, 0x77,
        ];

        let mut combined_bytes = Vec::new();
        combined_bytes.extend_from_slice(&frame1);
        combined_bytes.extend_from_slice(&frame2);

        let expected_voltages = vec![3.300, 3.301, 3.302, 3.303];

        match CellVoltages::decode(&combined_bytes, 4) {
            Ok(decoded) => {
                assert_eq!((*decoded).len(), expected_voltages.len());
                for (d, e) in (*decoded).iter().zip(expected_voltages.iter()) {
                    assert!((d - e).abs() < f32::EPSILON);
                }
            }
            Err(e) => panic!("Decoding failed: {:?}", e),
        }
    }

    #[test]
    fn test_cell_voltages_decode_frame_out_of_order() {
        let frame1: [u8; 13] = [
            0xA5, 0x40, 0x95, 0x08, 0x01, 0x0C, 0xE4, 0x0C, 0xE5, 0x0C, 0xE6, 0x00, 0x56,
        ];
        let frame2_wrong_order: [u8; 13] = [
            0xA5, 0x40, 0x95, 0x08, 0x01, 0x0C, 0xE7, 0x00, 0x00, 0x00, 0x00, 0x00, 0x75,
        ]; // Frame num 0x01, CRC for this data

        let mut combined_bytes = Vec::new();
        combined_bytes.extend_from_slice(&frame1);
        combined_bytes.extend_from_slice(&frame2_wrong_order);

        let result = CellVoltages::decode(&combined_bytes, 4);
        assert!(result.is_err());
        match result.err().unwrap() {
            Error::FrameNoError => {} // Expected
            e => panic!("Unexpected error type: {:?}", e),
        }
    }

    #[test]
    fn test_cell_temperatures_decode_valid_multi_frame() {
        // n_sensors = 8, so 2 frames.
        // Frame 1: Sens1=20C(raw 60,0x3C), Sens2=21C(61,0x3D), Sens3=22C(62,0x3E), Sens4=23C(63,0x3F), Sens5=24C(64,0x40), Sens6=25C(65,0x41), Sens7=26C(66,0x42)
        // Frame 1 Bytes: [0xA5, 0x40, 0x96, 0x08, 0x01, 0x3C, 0x3D, 0x3E, 0x3F, 0x40, 0x41, 0x42, CRC1]
        // CRC1: 0xA5+0x40+0x96+0x08+0x01+0x3C+0x3D+0x3E+0x3F+0x40+0x41+0x42 = 829 = 0x033D => 0x3D
        let frame1: [u8; 13] = [
            0xA5, 0x40, 0x96, 0x08, 0x01, 0x3C, 0x3D, 0x3E, 0x3F, 0x40, 0x41, 0x42, 0x3D,
        ];

        // Frame 2: Sens8=27C(raw 67,0x43)
        // Frame 2 Bytes: [0xA5, 0x40, 0x96, 0x08, 0x02, 0x43, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, CRC2]
        // CRC2: 0xA5+0x40+0x96+0x08+0x02+0x43+0x00+0x00+0x00+0x00+0x00+0x00 = 456 = 0x01C8 => 0xC8
        let frame2: [u8; 13] = [
            0xA5, 0x40, 0x96, 0x08, 0x02, 0x43, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC8,
        ];

        let mut combined_bytes = Vec::new();
        combined_bytes.extend_from_slice(&frame1);
        combined_bytes.extend_from_slice(&frame2);

        let expected_temperatures = vec![20, 21, 22, 23, 24, 25, 26, 27];

        match CellTemperatures::decode(&combined_bytes, 8) {
            Ok(decoded) => {
                assert_eq!(decoded, expected_temperatures);
            }
            Err(e) => panic!("Decoding failed: {:?}", e),
        }
    }

    #[test]
    fn test_cell_balance_state_decode_valid() {
        // n_cells = 16. Cells 0, 8, 15 are balancing.
        // Data bytes: [0x01, 0x81, 0x00, 0x00, 0x00, 0x00]
        // Frame: [0xA5, 0x40, 0x97, 0x08, 0x01, 0x81, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, CRC]
        // CRC: 0xA5+0x40+0x97+0x08+0x01+0x81+0x00+0x00+0x00+0x00+0x00+0x00 = 518 = 0x0206 => 0x06
        let bytes: [u8; 13] = [
            0xA5, 0x40, 0x97, 0x08, 0x01, 0x81, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06,
        ];

        let mut expected_state = vec![false; 16];
        expected_state[0] = true; // Cell 0
        expected_state[8] = true; // Cell 8
        expected_state[15] = true; // Cell 15

        match CellBalanceState::decode(&bytes, 16) {
            Ok(decoded) => {
                assert_eq!(decoded, expected_state);
            }
            Err(e) => panic!("Decoding failed: {:?}", e),
        }
    }

    #[test]
    fn test_cell_balance_state_decode_n_cells_less_than_byte_boundary() {
        // n_cells = 5. Cell 0 and Cell 4 are balancing.
        // Data byte 0: 0b00010001 = 0x11
        // Frame: [0xA5, 0x40, 0x97, 0x08, 0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, CRC]
        // CRC: 0xA5+0x40+0x97+0x08+0x11+0x00+0x00+0x00+0x00+0x00+0x00+0x00 = 463 = 0x01CF => 0x95
        let bytes: [u8; 13] = [
            0xA5, 0x40, 0x97, 0x08, 0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x95,
        ];

        let mut expected_state = vec![false; 5];
        expected_state[0] = true; // Cell 0
        expected_state[4] = true; // Cell 4

        match CellBalanceState::decode(&bytes, 5) {
            Ok(decoded) => {
                assert_eq!(decoded, expected_state);
            }
            Err(e) => panic!("Decoding failed: {:?}", e),
        }
    }

    #[test]
    fn test_error_code_decode_valid() {
        // Errors: CellVoltHighLevel1 (byte 4, bit 0), ChargeTempLowLevel2 (byte 5, bit 3), SocHighLevel1 (byte 6, bit 4), DiffTempLevel2 (byte 7, bit 3), AfeCollectChipErr (byte 9, bit 0)
        // rx_buffer[4] = 0x01
        // rx_buffer[5] = 0x08
        // rx_buffer[6] = 0x10
        // rx_buffer[7] = 0x08
        // rx_buffer[8] = 0x00
        // rx_buffer[9] = 0x01
        // rx_buffer[10]= 0x00
        // rx_buffer[11]= 0x00
        // Frame: [0xA5, 0x40, 0x98, 0x08, 0x01, 0x08, 0x10, 0x08, 0x00, 0x01, 0x00, 0x00, CRC]
        // CRC: 0xA5+0x40+0x98+0x08+0x01+0x08+0x10+0x08+0x00+0x01+0x00+0x00 = 423 = 0x01A7 => 0xA7
        let bytes: [u8; 13] = [
            0xA5, 0x40, 0x98, 0x08, 0x01, 0x08, 0x10, 0x08, 0x00, 0x01, 0x00, 0x00, 0xA7,
        ];

        let expected_errors = vec![
            ErrorCode::CellVoltHighLevel1,
            ErrorCode::ChargeTempLowLevel2,
            ErrorCode::SocHighLevel1,
            ErrorCode::DiffTempLevel2,
            ErrorCode::AfeCollectChipErr,
        ];

        match ErrorCode::decode(&bytes) {
            Ok(decoded) => {
                assert_eq!(decoded.len(), expected_errors.len());
                for err in expected_errors {
                    assert!(decoded.contains(&err), "Missing error: {:?}", err);
                }
            }
            Err(e) => panic!("Decoding failed: {:?}", e),
        }
    }

    #[test]
    fn test_error_code_decode_no_errors() {
        // Frame: [0xA5, 0x40, 0x98, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, CRC]
        // CRC: 0xA5+0x40+0x98+0x08+0x00+0x00+0x00+0x00+0x00+0x00+0x00+0x00 = 415 = 0x019F => 0x9F
        let bytes: [u8; 13] = [
            0xA5, 0x40, 0x98, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x85,
        ];

        match ErrorCode::decode(&bytes) {
            Ok(decoded) => {
                assert!(decoded.is_empty());
            }
            Err(e) => panic!("Decoding failed: {:?}", e),
        }
    }

    // Request encoding tests
    #[test]
    fn test_soc_request() {
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0x90, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7D,
        ];
        assert_eq!(Soc::request(Address::Host), expected_frame);
    }

    #[test]
    fn test_cell_voltage_range_request() {
        // CMD = 0x91
        // CRC = 0xA5+0x40+0x91+0x08 = 382 = 0x017E => 0x7E
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0x91, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7E,
        ];
        assert_eq!(CellVoltageRange::request(Address::Host), expected_frame);
    }

    #[test]
    fn test_temperature_range_request() {
        // CMD = 0x92
        // CRC = 0xA5+0x40+0x92+0x08 = 383 = 0x017F => 0x7F
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0x92, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F,
        ];
        assert_eq!(TemperatureRange::request(Address::Host), expected_frame);
    }

    #[test]
    fn test_mosfet_status_request() {
        // CMD = 0x93
        // CRC = 0xA5+0x40+0x93+0x08 = 384 = 0x0180 => 0x80
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0x93, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80,
        ];
        assert_eq!(MosfetStatus::request(Address::Host), expected_frame);
    }

    #[test]
    fn test_status_request() {
        // CMD = 0x94
        // CRC = 0xA5+0x40+0x94+0x08 = 385 = 0x0181 => 0x81
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0x94, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x81,
        ];
        assert_eq!(Status::request(Address::Host), expected_frame);
    }

    #[test]
    fn test_cell_voltages_request() {
        // CMD = 0x95
        // CRC = 0xA5+0x40+0x95+0x08 = 386 = 0x0182 => 0x82
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0x95, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x82,
        ];
        assert_eq!(CellVoltages::request(Address::Host), expected_frame);
    }

    #[test]
    fn test_cell_temperatures_request() {
        // CMD = 0x96
        // CRC = 0xA5+0x40+0x96+0x08 = 387 = 0x0183 => 0x83
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0x96, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x83,
        ];
        assert_eq!(CellTemperatures::request(Address::Host), expected_frame);
    }

    #[test]
    fn test_cell_balance_state_request() {
        // CMD = 0x97
        // CRC = 0xA5+0x40+0x97+0x08 = 388 = 0x0184 => 0x84
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0x97, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x84,
        ];
        assert_eq!(CellBalanceState::request(Address::Host), expected_frame);
    }

    #[test]
    fn test_error_code_request() {
        // CMD = 0x98
        // CRC = 0xA5+0x40+0x98+0x08 = 389 = 0x0185 => 0x85
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0x98, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x85,
        ];
        assert_eq!(ErrorCode::request(Address::Host), expected_frame);
    }

    #[test]
    fn test_set_discharge_mosfet_request_enable() {
        // CMD = 0xD9, Data[0] = 0x01
        // CRC = 0xA5+0x40+0xD9+0x08+0x01 = 165+64+217+8+1 = 455 = 0x01C7 => 0xC7
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0xD9, 0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC7,
        ];
        assert_eq!(
            SetDischargeMosfet::request(Address::Host, true),
            expected_frame
        );
    }

    #[test]
    fn test_set_discharge_mosfet_request_disable() {
        // CMD = 0xD9, Data[0] = 0x00
        // CRC = 0xA5+0x40+0xD9+0x08+0x00 = 165+64+217+8+0 = 454 = 0x01C6 => 0xC6
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0xD9, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC6,
        ];
        assert_eq!(
            SetDischargeMosfet::request(Address::Host, false),
            expected_frame
        );
    }

    #[test]
    fn test_set_charge_mosfet_request_enable() {
        // CMD = 0xDA, Data[0] = 0x01
        // CRC = 0xA5+0x40+0xDA+0x08+0x01 = 165+64+218+8+1 = 456 = 0x01C8 => 0xC8
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0xDA, 0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC8,
        ];
        assert_eq!(
            SetChargeMosfet::request(Address::Host, true),
            expected_frame
        );
    }

    #[test]
    fn test_set_soc_request() {
        // CMD = 0x21, SOC = 80.5% => 805 => 0x0325. Data[6]=0x03, Data[7]=0x25
        // Frame: A5 40 21 08 00 00 00 00 00 00 03 25 CRC
        // CRC: 0xA5+0x40+0x21+0x08+0x00+0x00+0x00+0x00+0x00+0x00+0x03+0x25 = 165+64+33+8+0+0+0+0+0+0+3+37 = 310 = 0x0136 => 0x36
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0x21, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x25, 0x36,
        ];
        assert_eq!(SetSoc::request(Address::Host, 80.5), expected_frame);
    }

    #[test]
    fn test_bms_reset_request() {
        // CMD = 0x00
        // CRC = 0xA5+0x40+0x00+0x08 = 165+64+0+8 = 237 = 0xED
        let expected_frame: [u8; 13] = [
            0xA5, 0x40, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xED,
        ];
        assert_eq!(BmsReset::request(Address::Host), expected_frame);
    }
}
