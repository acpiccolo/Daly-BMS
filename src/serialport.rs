use crate::protocol::*;
use anyhow::{bail, Context, Result};
use std::time::{Duration, Instant};

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
                .open()
                .with_context(|| format!("Cannot open serial port '{}'", port))?,
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
        self.serial_await_delay();

        self.serial
            .write_all(tx_buffer)
            .with_context(|| "Cannot write to serial")?;

        if false {
            self.serial
                .flush()
                .with_context(|| "Cannot flush serial connection")?;
        }
        Ok(())
    }

    fn receive_bytes(&mut self, size: usize) -> Result<Vec<u8>> {
        // Clear out the input buffer
        let mut rx_buffer = vec![0; size];

        // Read bytes from the specified serial interface
        self.serial
            .read_exact(&mut rx_buffer)
            .with_context(|| "Cannot receive response")?;

        self.last_execution = Instant::now();

        log::trace!("receive_bytes: {:02X?}", rx_buffer);
        Ok(rx_buffer)
    }

    pub fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
        self.serial
            .set_timeout(timeout)
            .map_err(anyhow::Error::from)
    }

    pub fn set_delay(&mut self, delay: Duration) {
        self.delay = Duration::max(delay, MINIMUM_DELAY);
    }

    pub fn get_soc(&mut self) -> Result<Soc> {
        self.send_bytes(&Soc::request(Address::Host))?;
        Soc::decode(&self.receive_bytes(Soc::reply_size())?).with_context(|| "Cannot get SOC")
    }

    pub fn get_cell_voltage_range(&mut self) -> Result<CellVoltageRange> {
        self.send_bytes(&CellVoltageRange::request(Address::Host))?;
        CellVoltageRange::decode(&self.receive_bytes(CellVoltageRange::reply_size())?)
            .with_context(|| "Cannot get cell voltage range")
    }

    pub fn get_temperature_range(&mut self) -> Result<TemperatureRange> {
        self.send_bytes(&TemperatureRange::request(Address::Host))?;
        TemperatureRange::decode(&self.receive_bytes(TemperatureRange::reply_size())?)
            .with_context(|| "Cannot get temperature range")
    }

    pub fn get_mosfet_status(&mut self) -> Result<MosfetStatus> {
        self.send_bytes(&MosfetStatus::request(Address::Host))?;
        MosfetStatus::decode(&self.receive_bytes(MosfetStatus::reply_size())?)
            .with_context(|| "Cannot get mosfet status")
    }

    pub fn get_status(&mut self) -> Result<Status> {
        self.send_bytes(&Status::request(Address::Host))?;
        let status = Status::decode(&self.receive_bytes(Status::reply_size())?)
            .with_context(|| "Cannot get status")?;
        self.status = Some(status.clone());
        Ok(status)
    }

    pub fn get_cell_voltages(&mut self) -> Result<Vec<f32>> {
        let n_cells = if let Some(status) = &self.status {
            status.cells
        } else {
            bail!("get_status() has to be called at least once before calling get_cell_voltages()");
        };
        self.send_bytes(&CellVoltages::request(Address::Host))?;
        CellVoltages::decode(
            &self.receive_bytes(CellVoltages::reply_size(n_cells))?,
            n_cells,
        )
        .with_context(|| "Cannot get cell voltages")
    }

    pub fn get_cell_temperatures(&mut self) -> Result<Vec<i32>> {
        let n_sensors = if let Some(status) = &self.status {
            status.temperature_sensors
        } else {
            bail!("get_status() has to be called at least once before calling get_cell_temperatures()");
        };

        self.send_bytes(&CellTemperatures::request(Address::Host))?;
        CellTemperatures::decode(
            &self.receive_bytes(CellTemperatures::reply_size(n_sensors))?,
            n_sensors,
        )
        .with_context(|| "Cannot get cell temperatures")
    }

    pub fn get_balancing_status(&mut self) -> Result<Vec<bool>> {
        let n_cells = if let Some(status) = &self.status {
            status.cells
        } else {
            bail!(
                "get_status() has to be called at least once before calling get_balancing_status()"
            );
        };

        self.send_bytes(&CellBalanceState::request(Address::Host))?;
        CellBalanceState::decode(
            &self.receive_bytes(CellBalanceState::reply_size())?,
            n_cells,
        )
        .with_context(|| "Cannot get cell balancing status")
    }

    pub fn get_errors(&mut self) -> Result<Vec<ErrorCode>> {
        self.send_bytes(&ErrorCode::request(Address::Host))?;
        ErrorCode::decode(&self.receive_bytes(ErrorCode::reply_size())?)
            .with_context(|| "Cannot get errors")
    }

    pub fn set_discharge_mosfet(&mut self, enable: bool) -> Result<()> {
        self.send_bytes(&SetDischargeMosfet::request(Address::Host, enable))?;
        SetDischargeMosfet::decode(&self.receive_bytes(SetDischargeMosfet::reply_size())?)
            .with_context(|| "Cannot set discharge mosfet")
    }

    pub fn set_charge_mosfet(&mut self, enable: bool) -> Result<()> {
        self.send_bytes(&SetChargeMosfet::request(Address::Host, enable))?;
        SetChargeMosfet::decode(&self.receive_bytes(SetChargeMosfet::reply_size())?)
            .with_context(|| "Cannot set charge mosfet")
    }

    pub fn set_soc(&mut self, soc_percent: f32) -> Result<()> {
        self.send_bytes(&SetSoc::request(Address::Host, soc_percent))?;
        SetSoc::decode(&self.receive_bytes(SetSoc::reply_size())?).with_context(|| "Cannot set SOC")
    }

    pub fn reset(&mut self) -> Result<()> {
        self.send_bytes(&BmsReset::request(Address::Host))?;
        BmsReset::decode(&self.receive_bytes(BmsReset::reply_size())?)
            .with_context(|| "Cannot reset BMS")
    }
}
