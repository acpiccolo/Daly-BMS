use crate::protocol::*;
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("get_status() has to be called at least once before")]
    StatusError,
    #[error("Daly error: {0}")]
    DalyError(#[from] crate::Error),
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Tokio serial error: {0}")]
    Serial(#[from] serialport::Error),
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct DalyBMS {
    serial: Box<dyn serialport::SerialPort>,
    last_execution: Instant,
    delay: Duration,
    status: Option<Status>,
}

impl DalyBMS {
    pub fn new(port: &str) -> Result<Self> {
        Ok(Self {
            serial: serialport::new(port, 9600)
                .data_bits(serialport::DataBits::Eight)
                .parity(serialport::Parity::None)
                .stop_bits(serialport::StopBits::One)
                .flow_control(serialport::FlowControl::None)
                .open()?,
            last_execution: Instant::now(),
            delay: MINIMUM_DELAY,
            status: None,
        })
    }

    fn serial_await_delay(&self) {
        let last_exec_diff = Instant::now().duration_since(self.last_execution);
        if let Some(time_until_delay_reached) = self.delay.checked_sub(last_exec_diff) {
            std::thread::sleep(time_until_delay_reached);
        }
    }

    fn send_bytes(&mut self, tx_buffer: &[u8]) -> Result<()> {
        // clear all incoming serial to avoid data collision
        loop {
            log::trace!("read to see if there is any pending data");
            let pending = self.serial.bytes_to_read()?;
            log::trace!("got {} pending bytes", pending);
            if pending > 0 {
                let mut buf: Vec<u8> = vec![0; 64];
                let received = self.serial.read(buf.as_mut_slice())?;
                log::trace!("{} pending bytes consumed", received);
            } else {
                break;
            }
        }
        self.serial_await_delay();

        log::trace!("write bytes: {:02X?}", tx_buffer);
        self.serial.write_all(tx_buffer)?;

        if false {
            log::trace!("flush connection");
            self.serial.flush()?;
        }
        Ok(())
    }

    fn receive_bytes(&mut self, size: usize) -> Result<Vec<u8>> {
        // Clear out the input buffer
        let mut rx_buffer = vec![0; size];

        // Read bytes from the specified serial interface
        log::trace!("read {} bytes", rx_buffer.len());
        self.serial.read_exact(&mut rx_buffer)?;

        self.last_execution = Instant::now();

        log::trace!("receive bytes: {:02X?}", rx_buffer);
        Ok(rx_buffer)
    }

    /// Sets the timeout for I/O operations
    pub fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
        log::trace!("set timeout to {:?}", timeout);
        self.serial.set_timeout(timeout).map_err(Error::from)
    }

    /// Delay between multiple commands
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

    pub fn get_soc(&mut self) -> Result<Soc> {
        log::trace!("get SOC");
        self.send_bytes(&Soc::request(Address::Host))?;
        Ok(Soc::decode(&self.receive_bytes(Soc::reply_size())?)?)
    }

    pub fn get_cell_voltage_range(&mut self) -> Result<CellVoltageRange> {
        log::trace!("get cell voltage range");
        self.send_bytes(&CellVoltageRange::request(Address::Host))?;
        Ok(CellVoltageRange::decode(
            &self.receive_bytes(CellVoltageRange::reply_size())?,
        )?)
    }

    pub fn get_temperature_range(&mut self) -> Result<TemperatureRange> {
        log::trace!("get temperature range");
        self.send_bytes(&TemperatureRange::request(Address::Host))?;
        Ok(TemperatureRange::decode(
            &self.receive_bytes(TemperatureRange::reply_size())?,
        )?)
    }

    pub fn get_mosfet_status(&mut self) -> Result<MosfetStatus> {
        log::trace!("get mosfet status");
        self.send_bytes(&MosfetStatus::request(Address::Host))?;
        Ok(MosfetStatus::decode(
            &self.receive_bytes(MosfetStatus::reply_size())?,
        )?)
    }

    pub fn get_status(&mut self) -> Result<Status> {
        log::trace!("get status");
        self.send_bytes(&Status::request(Address::Host))?;
        let status = Status::decode(&self.receive_bytes(Status::reply_size())?)?;
        self.status = Some(status.clone());
        Ok(status)
    }

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

    pub fn get_errors(&mut self) -> Result<Vec<ErrorCode>> {
        log::trace!("get errors");
        self.send_bytes(&ErrorCode::request(Address::Host))?;
        Ok(ErrorCode::decode(
            &self.receive_bytes(ErrorCode::reply_size())?,
        )?)
    }

    pub fn set_discharge_mosfet(&mut self, enable: bool) -> Result<()> {
        log::trace!("set discharge mosfet to {}", enable);
        self.send_bytes(&SetDischargeMosfet::request(Address::Host, enable))?;
        Ok(SetDischargeMosfet::decode(
            &self.receive_bytes(SetDischargeMosfet::reply_size())?,
        )?)
    }

    pub fn set_charge_mosfet(&mut self, enable: bool) -> Result<()> {
        log::trace!("set charge mosfet to {}", enable);
        self.send_bytes(&SetChargeMosfet::request(Address::Host, enable))?;
        Ok(SetChargeMosfet::decode(
            &self.receive_bytes(SetChargeMosfet::reply_size())?,
        )?)
    }

    pub fn set_soc(&mut self, soc_percent: f32) -> Result<()> {
        log::trace!("set SOC to {}", soc_percent);
        self.send_bytes(&SetSoc::request(Address::Host, soc_percent))?;
        Ok(SetSoc::decode(&self.receive_bytes(SetSoc::reply_size())?)?)
    }

    pub fn reset(&mut self) -> Result<()> {
        log::trace!("reset to factory default settings");
        self.send_bytes(&BmsReset::request(Address::Host))?;
        Ok(BmsReset::decode(
            &self.receive_bytes(BmsReset::reply_size())?,
        )?)
    }
}
