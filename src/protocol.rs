use crate::Error;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

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

const TX_BUFFER_LENGTH: usize = 13;
const RX_BUFFER_LENGTH: usize = 13;
const START_BYTE: u8 = 0xa5;
const DATA_LENGTH: u8 = 0x08;

fn create_request_header(address: Address, command: u8) -> Vec<u8> {
    let mut tx_buffer = vec![0; TX_BUFFER_LENGTH];
    tx_buffer[0] = START_BYTE;
    tx_buffer[1] = address as u8;
    tx_buffer[2] = command;
    tx_buffer[3] = DATA_LENGTH;
    tx_buffer
}

fn calc_crc(buffer: &[u8]) -> u8 {
    let mut checksum: u8 = 0;
    let slice = &buffer[0..buffer.len() - 1];
    for b in slice {
        checksum = checksum.wrapping_add(*b);
    }
    checksum
}

fn calc_crc_and_set(buffer: &mut [u8]) {
    let len = buffer.len();
    buffer[len - 1] = calc_crc(buffer)
}

macro_rules! read_bit {
    ($byte:expr,$position:expr) => {
        ($byte >> $position) & 1 != 0
    };
}

fn validate_len(buffer: &[u8], reply_size: usize) -> std::result::Result<(), Error> {
    if buffer.len() < reply_size {
        log::warn!(
            "Invalid buffer size - required={} received={}",
            buffer.len(),
            reply_size
        );
        return Err(Error::ReplySizeError);
    }
    Ok(())
}

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
    pub total_voltage: f32,
    /// Battery current in Amperes.
    /// Negative values indicate charging, positive values indicate discharging.
    pub current: f32,
    /// State of Charge percentage (0.0 - 100.0%).
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
    pub highest_voltage: f32,
    /// Cell number with the highest voltage.
    pub highest_cell: u8,
    /// Lowest cell voltage in Volts.
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
pub struct CellVoltages;

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
    pub fn decode(rx_buffer: &[u8], n_cells: u8) -> std::result::Result<Vec<f32>, Error> {
        validate_len(rx_buffer, Self::reply_size(n_cells))?;
        let mut result = Vec::with_capacity(n_cells as usize);
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
                log::trace!("Frame #{} cell #{} volt={}", n_frame, n_cell, volt);
                result.push(volt);
                n_cell += 1;
                if n_cell > n_cells {
                    break;
                }
            }
        }
        Ok(result)
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
                log::trace!("Frame #{} sensor #{} °C={}", n_frame, n_sensor, temperature);
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
                    break;
                }
            }
        }
        Ok(result)
    }
}

/// Represents various error codes and alarm states reported by the BMS.
#[derive(Debug, Clone, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ErrorCode {
    /// Cell voltage too high (Level 1 Alarm).
    #[error("Cell voltage is too high level one alarm")]
    CellVoltHighLevel1,
    /// Cell voltage too high (Level 2 Alarm).
    #[error("Cell voltage is too high level two alarm")]
    CellVoltHighLevel2,
    /// Cell voltage too low (Level 1 Alarm).
    #[error("Cell voltage is too low level one alarm")]
    CellVoltLowLevel1,
    /// Cell voltage too low (Level 2 Alarm).
    #[error("Cell voltage is too low level two alarm")]
    CellVoltLowLevel2,
    /// Total battery voltage too high (Level 1 Alarm).
    #[error("Total voltage is too high level one alarm")]
    SumVoltHighLevel1,
    /// Total battery voltage too high (Level 2 Alarm).
    #[error("Total voltage is too high level two alarm")]
    SumVoltHighLevel2,
    /// Total battery voltage too low (Level 1 Alarm).
    #[error("Total voltage is too low level one alarm")]
    SumVoltLowLevel1,
    /// Total battery voltage too low (Level 2 Alarm).
    #[error("Total voltage is too low level two alarm")]
    SumVoltLowLevel2,
    /// Charging temperature too high (Level 1 Alarm).
    #[error("Charging temperature too high level one alarm")]
    ChargeTempHighLevel1,
    /// Charging temperature too high (Level 2 Alarm).
    #[error("Charging temperature too high level two alarm")]
    ChargeTempHighLevel2,
    /// Charging temperature too low (Level 1 Alarm).
    #[error("Charging temperature too low level one alarm")]
    ChargeTempLowLevel1,
    /// Charging temperature too low (Level 2 Alarm).
    #[error("Charging temperature too low level two alarm")]
    ChargeTempLowLevel2,
    /// Discharging temperature too high (Level 1 Alarm).
    #[error("Discharging temperature too high level one alarm")]
    DischargeTempHighLevel1,
    /// Discharging temperature too high (Level 2 Alarm).
    #[error("Discharging temperature too high level two alarm")]
    DischargeTempHighLevel2,
    /// Discharging temperature too low (Level 1 Alarm).
    #[error("Discharging temperature too low level one alarm")]
    DischargeTempLowLevel1,
    /// Discharging temperature too low (Level 2 Alarm).
    #[error("Discharging temperature too low level two alarm")]
    DischargeTempLowLevel2,
    /// Charge overcurrent (Level 1 Alarm).
    #[error("Charge over current level one alarm")]
    ChargeOvercurrentLevel1,
    /// Charge overcurrent (Level 2 Alarm).
    #[error("Charge over current level two alarm")]
    ChargeOvercurrentLevel2,
    /// Discharge overcurrent (Level 1 Alarm).
    #[error("Discharge over current level one alarm")]
    DischargeOvercurrentLevel1,
    /// Discharge overcurrent (Level 2 Alarm).
    #[error("Discharge over current level two alarm")]
    DischargeOvercurrentLevel2,
    /// State of Charge (SOC) too high (Level 1 Alarm).
    #[error("SOC is too high level one alarm")]
    SocHighLevel1,
    /// State of Charge (SOC) too high (Level 2 Alarm).
    #[error("SOC is too high level two alarm")]
    SocHighLevel2,
    /// State of Charge (SOC) too low (Level 1 Alarm).
    #[error("SOC is too low level one alarm")]
    SocLowLevel1,
    /// State of Charge (SOC) too low (Level 2 Alarm).
    #[error("SOC is too low level two alarm")]
    SocLowLevel2,
    /// Excessive cell voltage difference (Level 1 Alarm).
    #[error("Excessive differential pressure level one alarm")]
    DiffVoltLevel1,
    /// Excessive cell voltage difference (Level 2 Alarm).
    #[error("Excessive differential pressure level two alarm")]
    DiffVoltLevel2,
    /// Excessive temperature difference between sensors (Level 1 Alarm).
    #[error("Excessive temperature difference level one alarm")]
    DiffTempLevel1,
    /// Excessive temperature difference between sensors (Level 2 Alarm).
    #[error("Excessive temperature difference level two alarm")]
    DiffTempLevel2,
    /// Charging MOSFET overtemperature alarm.
    #[error("Charging MOS overtemperature alarm")]
    ChargeMosTempHighAlarm,
    /// Discharging MOSFET overtemperature alarm.
    #[error("Discharging MOS overtemperature alarm")]
    DischargeMosTempHighAlarm,
    /// Charging MOSFET temperature sensor failure.
    #[error("Charging MOS temperature detection sensor failure")]
    ChargeMosTempSensorErr,
    /// Discharging MOSFET temperature sensor failure.
    #[error("Disharging MOS temperature detection sensor failure")]
    DischargeMosTempSensorErr,
    /// Charging MOSFET adhesion failure (stuck closed).
    #[error("Charging MOS adhesion failure")]
    ChargeMosAdhesionErr,
    /// Discharging MOSFET adhesion failure (stuck closed).
    #[error("Discharging MOS adhesion failure")]
    DischargeMosAdhesionErr,
    /// Charging MOSFET breaker failure (stuck open).
    #[error("Charging MOS breaker failure")]
    ChargeMosOpenCircuitErr,
    /// Discharging MOSFET breaker failure (stuck open).
    #[error("Discharging MOS breaker failure")]
    DischargeMosOpenCircuitErr,
    /// AFE (Analog Front End) acquisition chip malfunction.
    #[error("AFE acquisition chip malfunction")]
    AfeCollectChipErr,
    /// Monomer (cell voltage) collection circuit drop off.
    #[error("monomer collect drop off")]
    VoltageCollectDropped,
    /// Single temperature sensor failure.
    #[error("Single temperature sensor failure")]
    CellTempSensorErr,
    /// EEPROM storage failure.
    #[error("EEPROM storage failures")]
    EepromErr,
    /// RTC (Real-Time Clock) malfunction.
    #[error("RTC clock malfunction")]
    RtcErr,
    /// Precharge failure.
    #[error("Precharge failure")]
    PrechangeFailure,
    /// General communication malfunction.
    #[error("Communication malfunction")]
    CommunicationFailure,
    /// Internal communication module malfunction.
    #[error("Internal communication module malfunction")]
    InternalCommunicationFailure,
    /// Current detection module failure.
    #[error("Current module failure")]
    CurrentModuleFault,
    /// Total voltage detection module failure.
    #[error("Total voltage detection failure")]
    SumVoltageDetectFault,
    /// Short circuit protection failure.
    #[error("Short circuit protection failure")]
    ShortCircuitProtectFault,
    /// Low voltage condition forbids charging.
    #[error("Low voltage forbidden charging")]
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
            if val > 1000.0 { // BMS expects value * 10, so 100.0% is 1000
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
