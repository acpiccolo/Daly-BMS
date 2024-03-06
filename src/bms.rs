use std::ops::{Deref, DerefMut};
use std::{fmt, time::Duration};

use anyhow::{bail, Context, Result};

const TX_BUFFER_LENGTH: usize = 13;
const RX_BUFFER_LENGTH: usize = 13;

const START_BYTE: u8 = 0xa5;
const DATA_LENGTH: u8 = 0x08;

pub struct TxBuffer([u8; TX_BUFFER_LENGTH]);

impl TxBuffer {
    fn new() -> Self {
        Self([0; TX_BUFFER_LENGTH])
    }
}

impl Deref for TxBuffer {
    type Target = [u8; TX_BUFFER_LENGTH];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for TxBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl fmt::Debug for TxBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02X?}", self.0)
    }
}

pub struct RxBuffer([u8; RX_BUFFER_LENGTH]);

impl RxBuffer {
    fn new() -> Self {
        Self([0; RX_BUFFER_LENGTH])
    }
}

impl Deref for RxBuffer {
    type Target = [u8; RX_BUFFER_LENGTH];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for RxBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl fmt::Debug for RxBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02X?}", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Soc {
    pub total_voltage: f32,
    pub current: f32, // negative=charging, positive=discharging
    pub soc_percent: f32,
}

impl From<RxBuffer> for Soc {
    fn from(rx_buffer: RxBuffer) -> Self {
        Self {
            total_voltage: u16::from_be_bytes([rx_buffer[4], rx_buffer[5]]) as f32 / 10.0,
            // The current measurement is given with a 30000 unit offset (see /docs/)
            current: (((u16::from_be_bytes([rx_buffer[8], rx_buffer[9]]) as i32) - 30000) as f32)
                / 10.0,
            soc_percent: u16::from_be_bytes([rx_buffer[10], rx_buffer[11]]) as f32 / 10.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CellVoltageRange {
    pub highest_voltage: f32,
    pub highest_cell: u8,
    pub lowest_voltage: f32,
    pub lowest_cell: u8,
}

impl From<RxBuffer> for CellVoltageRange {
    fn from(rx_buffer: RxBuffer) -> Self {
        Self {
            highest_voltage: u16::from_be_bytes([rx_buffer[4], rx_buffer[5]]) as f32 / 1000.0,
            highest_cell: rx_buffer[6],
            lowest_voltage: u16::from_be_bytes([rx_buffer[7], rx_buffer[8]]) as f32 / 1000.0,
            lowest_cell: rx_buffer[9],
        }
    }
}

#[derive(Debug, Clone)]
pub struct TemperatureRange {
    pub highest_temperature: i8,
    pub highest_sensor: u8,
    pub lowest_temperature: i8,
    pub lowest_sensor: u8,
}

impl From<RxBuffer> for TemperatureRange {
    fn from(rx_buffer: RxBuffer) -> Self {
        // An offset of 40 is added by the BMS to avoid having to deal with negative numbers, see protocol in /docs/
        Self {
            highest_temperature: ((rx_buffer[4] as i16) - 40) as i8,
            highest_sensor: rx_buffer[5],
            lowest_temperature: ((rx_buffer[6] as i16) - 40) as i8,
            lowest_sensor: rx_buffer[7],
        }
    }
}

#[derive(Debug, Clone)]
pub enum MosfetMode {
    Stationary,
    Charging,
    Discharging,
}

#[derive(Debug, Clone)]
pub struct MosfetStatus {
    mode: MosfetMode,
    charging_mosfet: bool,
    discharging_mosfet: bool,
    bms_cycles: u8,
    capacity_ah: f32,
}

impl From<RxBuffer> for MosfetStatus {
    fn from(rx_buffer: RxBuffer) -> Self {
        let mode = match rx_buffer[4] {
            0 => MosfetMode::Stationary,
            1 => MosfetMode::Charging,
            2 => MosfetMode::Discharging,
            _ => unreachable!(),
        };
        Self {
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
        }
    }
}

#[derive(Debug, Clone)]
pub struct IOState {
    di1: bool,
    di2: bool,
    di3: bool,
    di4: bool,
    do1: bool,
    do2: bool,
    do3: bool,
    do4: bool,
}

#[derive(Debug, Clone)]
pub struct Status {
    cells: u8,
    temperature_sensors: u8,
    charger_running: bool,
    load_running: bool,
    states: IOState,
    cycles: u16,
}

macro_rules! read_bit {
    ($byte:expr,$position:expr) => {
        ($byte >> $position) & 1 != 0
    };
}

impl From<RxBuffer> for Status {
    fn from(rx_buffer: RxBuffer) -> Self {
        Self {
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
        }
    }
}

pub fn decode_cell_voltages(rx_buffers: Vec<RxBuffer>, n_cells: u8) -> Vec<f32> {
    let mut result = Vec::with_capacity(n_cells as usize);
    let mut n_frame = 1;
    let mut n_cell = 1;
    for rx_buffer in rx_buffers {
        for i in 0..3 {
            let volt =
                u16::from_be_bytes([rx_buffer[5 + i + i], rx_buffer[6 + i + i]]) as f32 / 1000.0;
            log::trace!("Frame #{} cell #{} volt={}", n_frame, n_cell, volt);
            result.push(volt);
            n_cell += 1;
            if n_cell > n_cells {
                break;
            }
        }
        n_frame += 1;
    }
    result
}

pub fn decode_cell_temperatures(rx_buffers: Vec<RxBuffer>, sensors: u8) -> Vec<i32> {
    let mut result = Vec::with_capacity(sensors as usize);
    let mut n_frame = 1;
    let mut n_sensor = 1;
    for rx_buffer in rx_buffers {
        for i in 0..7 {
            let temperature = rx_buffer[5 + i] as i32 - 40;
            log::trace!("Frame #{} sensor #{} °C={}", n_frame, n_sensor, temperature);
            result.push(temperature);
            n_sensor += 1;
            if n_sensor > sensors {
                break;
            }
        }
        n_frame += 1;
    }
    result
}

pub fn decode_balancing_status(rx_buffer: RxBuffer, n_cells: u8) -> Vec<bool> {
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
    result
}

#[derive(Debug, Clone)]
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

impl From<RxBuffer> for Vec<ErrorCode> {
    fn from(rx_buffer: RxBuffer) -> Self {
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

        result
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

#[repr(u8)]
enum Address {
    Host = 0x40,
}

enum Command {
    Soc,
    CellVoltageRange,
    TemperatureRange,
    MosfetStatus,
    Status,
    CellVoltages,
    CellTemperature,
    CellBallanceState,
    Errors,

    SetDischargeMosfet(bool),
    SetChargeMosfet(bool),
    SetSoc(f32),
    BmsReset,
}

pub struct Bms {
    serial: Box<dyn serialport::SerialPort>,
    status: Option<Status>,
}

impl Bms {
    pub fn new(port: &str, timeout: Duration) -> Result<Self> {
        Ok(Self {
            serial: serialport::new(port, 9600)
                .data_bits(serialport::DataBits::Eight)
                .parity(serialport::Parity::None)
                .stop_bits(serialport::StopBits::One)
                .flow_control(serialport::FlowControl::None)
                .timeout(timeout)
                .open()
                .with_context(|| format!("Cannot open serial port '{}'", port))?,
            status: None,
        })
    }

    fn calc_crc(buffer: &[u8]) -> u8 {
        let mut checksum: u8 = 0;
        let slice = &buffer[0..buffer.len() - 1];
        for b in slice {
            checksum = checksum.wrapping_add(*b);
        }
        checksum
    }

    fn create_tx_buffer(cmd: Command) -> TxBuffer {
        let mut tx_buffer = TxBuffer::new();
        tx_buffer[0] = START_BYTE;
        tx_buffer[1] = Address::Host as u8;
        tx_buffer[3] = DATA_LENGTH;
        match cmd {
            Command::Soc => {
                tx_buffer[2] = 0x90;
            }
            Command::CellVoltageRange => {
                tx_buffer[2] = 0x91;
            }
            Command::TemperatureRange => {
                tx_buffer[2] = 0x92;
            }
            Command::MosfetStatus => {
                tx_buffer[2] = 0x93;
            }
            Command::Status => {
                tx_buffer[2] = 0x94;
            }
            Command::CellVoltages => {
                tx_buffer[2] = 0x95;
            }
            Command::CellTemperature => {
                tx_buffer[2] = 0x96;
            }
            Command::CellBallanceState => {
                tx_buffer[2] = 0x97;
            }
            Command::Errors => {
                tx_buffer[2] = 0x98;
            }
            Command::SetDischargeMosfet(enable) => {
                tx_buffer[2] = 0xD9;
                if enable {
                    tx_buffer[4] = 0x01;
                }
            }
            Command::SetChargeMosfet(enable) => {
                tx_buffer[2] = 0xDA;
                if enable {
                    tx_buffer[4] = 0x01;
                }
            }
            Command::SetSoc(soc_percent) => {
                tx_buffer[2] = 0x21;
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
            }
            Command::BmsReset => {
                tx_buffer[2] = 0x00;
            }
        }
        tx_buffer[TX_BUFFER_LENGTH - 1] = Self::calc_crc(&*tx_buffer);

        log::trace!("create_tx_buffer: {:?}", tx_buffer);

        tx_buffer
    }

    fn send_command(&mut self, cmd: Command) -> Result<()> {
        let tx_buffer = Self::create_tx_buffer(cmd);

        // clear all incoming serial to avoid data collision
        loop {
            let pending = self
                .serial
                .bytes_to_read()
                .with_context(|| "Cannot read number of pending bytes")?;
            if pending > 0 {
                log::trace!("Got {} pending bytes", pending);
                let mut buf: Vec<u8> = vec![0; 64];
                let received = self
                    .serial
                    .read(buf.as_mut_slice())
                    .with_context(|| "Cannot read  pending bytes")?;
                log::trace!("Read {} pending bytes", received);
            } else {
                break;
            }
        }

        self.serial
            .write_all(&*tx_buffer)
            .with_context(|| "Cannot write to serial")?;

        Ok(())
    }

    fn receive_bytes(&mut self) -> Result<RxBuffer> {
        // Clear out the input buffer
        let mut rx_buffer = RxBuffer::new();

        // Read bytes from the specified serial interface
        self.serial
            .read_exact(&mut *rx_buffer)
            .with_context(|| "Cannot receive response")?;

        log::trace!("receive_bytes: {:?}", rx_buffer);

        let checksum = Self::calc_crc(&*rx_buffer);
        if rx_buffer[RX_BUFFER_LENGTH - 1] != checksum {
            log::trace!(
                "Invalid checksum - calculated={:02X?} received={:02X?} rx_buffer={:?}",
                checksum,
                rx_buffer[RX_BUFFER_LENGTH - 1],
                rx_buffer
            );
            bail!("Checksum failed!");
        }
        Ok(rx_buffer)
    }

    fn receive_frames(&mut self, n_frames: u8) -> Result<Vec<RxBuffer>> {
        let mut result = Vec::with_capacity(n_frames as usize);
        log::trace!("receive_frames n_frames={}", n_frames);
        for n_frame in 1..=n_frames {
            let rx_buffer = self.receive_bytes()?;
            if n_frame != rx_buffer[4] {
                bail!(
                    "frame out of order, expected {} got {}",
                    n_frame,
                    rx_buffer[4]
                );
            }
            result.push(rx_buffer);
        }
        Ok(result)
    }

    pub fn get_soc(&mut self) -> Result<Soc> {
        self.send_command(Command::Soc)?;
        let rx_buffer = self.receive_bytes()?;
        Ok(Soc::from(rx_buffer))
    }

    pub fn get_cell_voltage_range(&mut self) -> Result<CellVoltageRange> {
        self.send_command(Command::CellVoltageRange)?;
        let rx_buffer = self.receive_bytes()?;
        Ok(CellVoltageRange::from(rx_buffer))
    }

    pub fn get_temperature_range(&mut self) -> Result<TemperatureRange> {
        self.send_command(Command::TemperatureRange)?;
        let rx_buffer = self.receive_bytes()?;
        Ok(TemperatureRange::from(rx_buffer))
    }

    pub fn get_mosfet_status(&mut self) -> Result<MosfetStatus> {
        self.send_command(Command::MosfetStatus)?;
        let rx_buffer = self.receive_bytes()?;
        Ok(MosfetStatus::from(rx_buffer))
    }

    pub fn get_status(&mut self) -> Result<Status> {
        self.send_command(Command::Status)?;
        let rx_buffer = self.receive_bytes()?;
        let status = Status::from(rx_buffer);
        self.status = Some(status.clone());
        Ok(status)
    }

    pub fn get_cell_voltages(&mut self) -> Result<Vec<f32>> {
        let n_cells = if let Some(status) = &self.status {
            status.cells
        } else {
            bail!("get_status() has to be called at least once before calling get_cell_voltages()");
        };
        self.send_command(Command::CellVoltages)?;

        let n_frames = (n_cells as f32 / 3.0).ceil() as u8;
        let rx_buffers = self.receive_frames(n_frames)?;
        Ok(decode_cell_voltages(rx_buffers, n_cells))
    }

    pub fn get_cell_temperatures(&mut self) -> Result<Vec<i32>> {
        let sensors = if let Some(status) = &self.status {
            status.temperature_sensors
        } else {
            bail!("get_status() has to be called at least once before calling get_cell_temperatures()");
        };
        self.send_command(Command::CellTemperature)?;
        let n_frames = (sensors as f32 / 7.0).ceil() as u8;
        let rx_buffers = self.receive_frames(n_frames)?;
        Ok(decode_cell_temperatures(rx_buffers, sensors))
    }

    pub fn get_balancing_status(&mut self) -> Result<Vec<bool>> {
        let n_cells = if let Some(status) = &self.status {
            status.cells
        } else {
            bail!(
                "get_status() has to be called at least once before calling get_balancing_status()"
            );
        };

        self.send_command(Command::CellBallanceState)?;
        let rx_buffer = self.receive_bytes()?;
        Ok(decode_balancing_status(rx_buffer, n_cells))
    }

    pub fn get_errors(&mut self) -> Result<Vec<ErrorCode>> {
        self.send_command(Command::Errors)?;
        let rx_buffer = self.receive_bytes()?;
        Ok(Vec::<ErrorCode>::from(rx_buffer))
    }

    pub fn set_discharge_mosfet(&mut self, enable: bool) -> Result<()> {
        self.send_command(Command::SetDischargeMosfet(enable))?;
        self.receive_bytes().map(|_| ())
    }

    pub fn set_charge_mosfet(&mut self, enable: bool) -> Result<()> {
        self.send_command(Command::SetChargeMosfet(enable))?;
        self.receive_bytes().map(|_| ())
    }

    pub fn set_soc(&mut self, soc_percent: f32) -> Result<()> {
        self.send_command(Command::SetSoc(soc_percent))?;
        self.receive_bytes().map(|_| ())
    }

    pub fn reset(&mut self) -> Result<()> {
        self.send_command(Command::BmsReset)?;
        self.receive_bytes().map(|_| ())
    }
}
