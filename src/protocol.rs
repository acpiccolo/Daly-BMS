use crate::Error;
use std::fmt;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug)]
#[repr(u8)]
pub enum Address {
    Host = 0x40,
}

// https://minimalmodbus.readthedocs.io/en/stable/serialcommunication.html#timing-of-the-serial-communications
// minimum delay 4ms by baud rate 9600
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

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Soc {
    pub total_voltage: f32,
    pub current: f32, // negative=charging, positive=discharging
    pub soc_percent: f32,
}

impl Soc {
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x90);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

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

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CellVoltageRange {
    pub highest_voltage: f32,
    pub highest_cell: u8,
    pub lowest_voltage: f32,
    pub lowest_cell: u8,
}

impl CellVoltageRange {
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x91);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

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

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TemperatureRange {
    pub highest_temperature: i8,
    pub highest_sensor: u8,
    pub lowest_temperature: i8,
    pub lowest_sensor: u8,
}

impl TemperatureRange {
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x92);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

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

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MosfetMode {
    Stationary,
    Charging,
    Discharging,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MosfetStatus {
    pub mode: MosfetMode,
    pub charging_mosfet: bool,
    pub discharging_mosfet: bool,
    pub bms_cycles: u8,
    pub capacity_ah: f32,
}

impl MosfetStatus {
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x93);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

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

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IOState {
    pub di1: bool,
    pub di2: bool,
    pub di3: bool,
    pub di4: bool,
    pub do1: bool,
    pub do2: bool,
    pub do3: bool,
    pub do4: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Status {
    pub cells: u8,
    pub temperature_sensors: u8,
    pub charger_running: bool,
    pub load_running: bool,
    pub states: IOState,
    pub cycles: u16,
}

impl Status {
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x94);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

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

pub struct CellVoltages;

impl CellVoltages {
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x95);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    fn n_frames(n_cells: u8) -> usize {
        (n_cells as f32 / 3.0).ceil() as usize
    }

    pub fn reply_size(n_cells: u8) -> usize {
        Self::n_frames(n_cells) * RX_BUFFER_LENGTH
    }

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

pub struct CellTemperatures;

impl CellTemperatures {
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x96);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    fn n_frames(n_sensors: u8) -> usize {
        (n_sensors as f32 / 7.0).ceil() as usize
    }

    pub fn reply_size(n_sensors: u8) -> usize {
        Self::n_frames(n_sensors) * RX_BUFFER_LENGTH
    }

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

pub struct CellBalanceState;

impl CellBalanceState {
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x97);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

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

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ErrorCode {
    CellVoltHighLevel1,
    CellVoltHighLevel2,
    CellVoltLowLevel1,
    CellVoltLowLevel2,
    SumVoltHighLevel1,
    SumVoltHighLevel2,
    SumVoltLowLevel1,
    SumVoltLowLevel2,
    ChargeTempHighLevel1,
    ChargeTempHighLevel2,
    ChargeTempLowLevel1,
    ChargeTempLowLevel2,
    DischargeTempHighLevel1,
    DischargeTempHighLevel2,
    DischargeTempLowLevel1,
    DischargeTempLowLevel2,
    ChargeOvercurrentLevel1,
    ChargeOvercurrentLevel2,
    DischargeOvercurrentLevel1,
    DischargeOvercurrentLevel2,
    SocHighLevel1,
    SocHighLevel2,
    SocLowLevel1,
    SocLowLevel2,
    DiffVoltLevel1,
    DiffVoltLevel2,
    DiffTempLevel1,
    DiffTempLevel2,
    ChargeMosTempHighAlarm,
    DischargeMosTempHighAlarm,
    ChargeMosTempSensorErr,
    DischargeMosTempSensorErr,
    ChargeMosAdhesionErr,
    DischargeMosAdhesionErr,
    ChargeMosOpenCircuitErr,
    DischargeMosOpenCircuitErr,
    AfeCollectChipErr,
    VoltageCollectDropped,
    CellTempSensorErr,
    EepromErr,
    RtcErr,
    PrechangeFailure,
    CommunicationFailure,
    InternalCommunicationFailure,
    CurrentModuleFault,
    SumVoltageDetectFault,
    ShortCircuitProtectFault,
    LowVoltForbiddenChargeFault,
}

impl ErrorCode {
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x98);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

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

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ErrorCode::CellVoltHighLevel1 => write!(f, "Cell voltage is too high level one alarm"),
            ErrorCode::CellVoltHighLevel2 => write!(f, "Cell voltage is too high level two alarm"),
            ErrorCode::CellVoltLowLevel1 => write!(f, "Cell voltage is too low level one alarm"),
            ErrorCode::CellVoltLowLevel2 => write!(f, "Cell voltage is too low level two alarm"),
            ErrorCode::SumVoltHighLevel1 => write!(f, "Total voltage is too high level one alarm"),
            ErrorCode::SumVoltHighLevel2 => write!(f, "Total voltage is too high level two alarm"),
            ErrorCode::SumVoltLowLevel1 => write!(f, "Total voltage is too low level one alarm"),
            ErrorCode::SumVoltLowLevel2 => write!(f, "Total voltage is too low level two alarm"),
            ErrorCode::ChargeTempHighLevel1 => {
                write!(f, "Charging temperature too high level one alarm")
            }
            ErrorCode::ChargeTempHighLevel2 => {
                write!(f, "Charging temperature too high level two alarm")
            }
            ErrorCode::ChargeTempLowLevel1 => {
                write!(f, "Charging temperature too low level one alarm")
            }
            ErrorCode::ChargeTempLowLevel2 => {
                write!(f, "Charging temperature too low level two alarm")
            }
            ErrorCode::DischargeTempHighLevel1 => {
                write!(f, "Discharging temperature too high level one alarm")
            }
            ErrorCode::DischargeTempHighLevel2 => {
                write!(f, "Discharging temperature too high level two alarm")
            }
            ErrorCode::DischargeTempLowLevel1 => {
                write!(f, "Discharging temperature too low level one alarm")
            }
            ErrorCode::DischargeTempLowLevel2 => {
                write!(f, "Discharging temperature too low level two alarm")
            }
            ErrorCode::ChargeOvercurrentLevel1 => write!(f, "Charge over current level one alarm"),
            ErrorCode::ChargeOvercurrentLevel2 => write!(f, "Charge over current level two alarm"),
            ErrorCode::DischargeOvercurrentLevel1 => {
                write!(f, "Discharge over current level one alarm")
            }
            ErrorCode::DischargeOvercurrentLevel2 => {
                write!(f, "Discharge over current level two alarm")
            }
            ErrorCode::SocHighLevel1 => write!(f, "SOC is too high level one alarm"),
            ErrorCode::SocHighLevel2 => write!(f, "SOC is too high level two alarm"),
            ErrorCode::SocLowLevel1 => write!(f, "SOC is too low level one alarm"),
            ErrorCode::SocLowLevel2 => write!(f, "SOC is too low level two alarm"),
            ErrorCode::DiffVoltLevel1 => {
                write!(f, "Excessive differential pressure level one alarm")
            }
            ErrorCode::DiffVoltLevel2 => {
                write!(f, "Excessive differential pressure level two alarm")
            }
            ErrorCode::DiffTempLevel1 => {
                write!(f, "Excessive temperature difference level one alarm")
            }
            ErrorCode::DiffTempLevel2 => {
                write!(f, "Excessive temperature difference level two alarm")
            }
            ErrorCode::ChargeMosTempHighAlarm => write!(f, "Charging MOS overtemperature alarm"),
            ErrorCode::DischargeMosTempHighAlarm => {
                write!(f, "Discharging MOS overtemperature alarm")
            }
            ErrorCode::ChargeMosTempSensorErr => {
                write!(f, "Charging MOS temperature detection sensor failure")
            }
            ErrorCode::DischargeMosTempSensorErr => {
                write!(f, "Disharging MOS temperature detection sensor failure")
            }
            ErrorCode::ChargeMosAdhesionErr => write!(f, "Charging MOS adhesion failure"),
            ErrorCode::DischargeMosAdhesionErr => write!(f, "Discharging MOS adhesion failure"),
            ErrorCode::ChargeMosOpenCircuitErr => write!(f, "Charging MOS breaker failure"),
            ErrorCode::DischargeMosOpenCircuitErr => write!(f, "Discharging MOS breaker failure"),
            ErrorCode::AfeCollectChipErr => write!(f, "AFE acquisition chip malfunction"),
            ErrorCode::VoltageCollectDropped => write!(f, "monomer collect drop off"),
            ErrorCode::CellTempSensorErr => write!(f, "Single Temperature Sensor Fault"),
            ErrorCode::EepromErr => write!(f, "EEPROM storage failures"),
            ErrorCode::RtcErr => write!(f, "RTC clock malfunction"),
            ErrorCode::PrechangeFailure => write!(f, "Precharge Failure"),
            ErrorCode::CommunicationFailure => write!(f, "vehicle communications malfunction"),
            ErrorCode::InternalCommunicationFailure => {
                write!(f, "intranet communication module malfunction")
            }
            ErrorCode::CurrentModuleFault => write!(f, "Current Module Failure"),
            ErrorCode::SumVoltageDetectFault => write!(f, "main pressure detection module"),
            ErrorCode::ShortCircuitProtectFault => write!(f, "Short circuit protection failure"),
            ErrorCode::LowVoltForbiddenChargeFault => write!(f, "Low Voltage No Charging"),
        }
    }
}

pub struct SetDischargeMosfet;

impl SetDischargeMosfet {
    pub fn request(address: Address, enable: bool) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0xD9);
        if enable {
            tx_buffer[4] = 0x01;
        }
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<(), Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)
    }
}
pub struct SetChargeMosfet;

impl SetChargeMosfet {
    pub fn request(address: Address, enable: bool) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0xDA);
        if enable {
            tx_buffer[4] = 0x01;
        }
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<(), Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)
    }
}

pub struct SetSoc;

impl SetSoc {
    pub fn request(address: Address, soc_percent: f32) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x21);
        let value = {
            let val = (soc_percent * 10.0).round();
            if val > 1000.0 {
                1000
            } else if val < 0.0 {
                0
            } else {
                val as u16
            }
        }
        .to_be_bytes();
        tx_buffer[10] = value[0];
        tx_buffer[11] = value[1];
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<(), Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)
    }
}
pub struct BmsReset;

impl BmsReset {
    pub fn request(address: Address) -> Vec<u8> {
        let mut tx_buffer = create_request_header(address, 0x00);
        calc_crc_and_set(&mut tx_buffer);
        tx_buffer
    }

    pub fn reply_size() -> usize {
        RX_BUFFER_LENGTH
    }

    pub fn decode(rx_buffer: &[u8]) -> std::result::Result<(), Error> {
        validate_len(rx_buffer, Self::reply_size())?;
        validate_checksum(rx_buffer)
    }
}
