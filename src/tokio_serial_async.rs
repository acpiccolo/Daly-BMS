use crate::protocol::*;
use anyhow::{bail, Context, Result};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::{SerialPort, SerialPortBuilderExt};

#[derive(Debug)]
pub struct DalyBMS {
    serial: tokio_serial::SerialStream,
    last_execution: Instant,
    io_timeout: Duration,
    delay: Duration,
    status: Option<Status>,
}

impl DalyBMS {
    pub fn new(port: &str) -> Result<Self> {
        Ok(Self {
            serial: tokio_serial::new(port, 9600)
                .data_bits(tokio_serial::DataBits::Eight)
                .parity(tokio_serial::Parity::None)
                .stop_bits(tokio_serial::StopBits::One)
                .flow_control(tokio_serial::FlowControl::None)
                .open_native_async()
                .with_context(|| format!("Cannot open serial port '{}'", port))?,
            last_execution: Instant::now(),
            delay: MINIMUM_DELAY,
            io_timeout: Duration::from_secs(5),
            status: None,
        })
    }

    async fn serial_await_delay(&self) {
        let last_exec_diff = Instant::now().duration_since(self.last_execution);
        if let Some(time_until_delay_reached) = self.delay.checked_sub(last_exec_diff) {
            tokio::time::sleep(time_until_delay_reached).await;
        }
    }

    async fn send_bytes(&mut self, tx_buffer: &[u8]) -> Result<()> {
        // clear all incoming serial to avoid data collision
        loop {
            let pending = self
                .serial
                .bytes_to_read()
                .with_context(|| "Cannot read number of pending bytes")?;
            if pending > 0 {
                log::trace!("Got {} pending bytes", pending);
                let mut buf: Vec<u8> = vec![0; 64];

                let received =
                    tokio::time::timeout(self.io_timeout, self.serial.read(buf.as_mut_slice()))
                        .await
                        .with_context(|| "Cannot read pending bytes")??;
                log::trace!("Read {} pending bytes", received);
            } else {
                break;
            }
        }
        self.serial_await_delay().await;

        tokio::time::timeout(self.io_timeout, self.serial.write_all(tx_buffer))
            .await
            .with_context(|| "Cannot write to serial")??;

        if false {
            tokio::time::timeout(self.io_timeout, self.serial.flush())
                .await
                .with_context(|| "Cannot flush serial connection")??;
        }
        Ok(())
    }

    async fn receive_bytes(&mut self, size: usize) -> Result<Vec<u8>> {
        // Clear out the input buffer
        let mut rx_buffer = vec![0; size];

        // Read bytes from the specified serial interface
        tokio::time::timeout(self.io_timeout, self.serial.read_exact(&mut rx_buffer))
            .await
            .with_context(|| "Cannot receive response")??;

        self.last_execution = Instant::now();

        log::trace!("receive_bytes: {:02X?}", rx_buffer);
        Ok(rx_buffer)
    }

    pub fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
        self.io_timeout = timeout;
        Ok(())
        // self.serial
        //     .set_timeout(timeout)
        //     .map_err(anyhow::Error::from)
    }

    pub fn set_delay(&mut self, delay: Duration) {
        self.delay = Duration::max(delay, MINIMUM_DELAY);
    }

    pub async fn get_soc(&mut self) -> Result<Soc> {
        self.send_bytes(&Soc::request(Address::Host)).await?;
        Soc::decode(&self.receive_bytes(Soc::reply_size()).await?).with_context(|| "Cannot get SOC")
    }

    pub async fn get_cell_voltage_range(&mut self) -> Result<CellVoltageRange> {
        self.send_bytes(&CellVoltageRange::request(Address::Host))
            .await?;
        CellVoltageRange::decode(&self.receive_bytes(CellVoltageRange::reply_size()).await?)
            .with_context(|| "Cannot get cell voltage range")
    }

    pub async fn get_temperature_range(&mut self) -> Result<TemperatureRange> {
        self.send_bytes(&TemperatureRange::request(Address::Host))
            .await?;
        TemperatureRange::decode(&self.receive_bytes(TemperatureRange::reply_size()).await?)
            .with_context(|| "Cannot get temperature range")
    }

    pub async fn get_mosfet_status(&mut self) -> Result<MosfetStatus> {
        self.send_bytes(&MosfetStatus::request(Address::Host))
            .await?;
        MosfetStatus::decode(&self.receive_bytes(MosfetStatus::reply_size()).await?)
            .with_context(|| "Cannot get mosfet status")
    }

    pub async fn get_status(&mut self) -> Result<Status> {
        self.send_bytes(&Status::request(Address::Host)).await?;
        let status = Status::decode(&self.receive_bytes(Status::reply_size()).await?)
            .with_context(|| "Cannot get status")?;
        self.status = Some(status.clone());
        Ok(status)
    }

    pub async fn get_cell_voltages(&mut self) -> Result<Vec<f32>> {
        let n_cells = if let Some(status) = &self.status {
            status.cells
        } else {
            bail!("get_status() has to be called at least once before calling get_cell_voltages()");
        };
        self.send_bytes(&CellVoltages::request(Address::Host))
            .await?;
        CellVoltages::decode(
            &self
                .receive_bytes(CellVoltages::reply_size(n_cells))
                .await?,
            n_cells,
        )
        .with_context(|| "Cannot get cell voltages")
    }

    pub async fn get_cell_temperatures(&mut self) -> Result<Vec<i32>> {
        let n_sensors = if let Some(status) = &self.status {
            status.temperature_sensors
        } else {
            bail!("get_status() has to be called at least once before calling get_cell_temperatures()");
        };

        self.send_bytes(&CellTemperatures::request(Address::Host))
            .await?;
        CellTemperatures::decode(
            &self
                .receive_bytes(CellTemperatures::reply_size(n_sensors))
                .await?,
            n_sensors,
        )
        .with_context(|| "Cannot get cell temperatures")
    }

    pub async fn get_balancing_status(&mut self) -> Result<Vec<bool>> {
        let n_cells = if let Some(status) = &self.status {
            status.cells
        } else {
            bail!(
                "get_status() has to be called at least once before calling get_balancing_status()"
            );
        };

        self.send_bytes(&CellBalanceState::request(Address::Host))
            .await?;
        CellBalanceState::decode(
            &self.receive_bytes(CellBalanceState::reply_size()).await?,
            n_cells,
        )
        .with_context(|| "Cannot get cell balancing status")
    }

    pub async fn get_errors(&mut self) -> Result<Vec<ErrorCode>> {
        self.send_bytes(&ErrorCode::request(Address::Host)).await?;
        ErrorCode::decode(&self.receive_bytes(ErrorCode::reply_size()).await?)
            .with_context(|| "Cannot get errors")
    }

    pub async fn set_discharge_mosfet(&mut self, enable: bool) -> Result<()> {
        self.send_bytes(&SetDischargeMosfet::request(Address::Host, enable))
            .await?;
        SetDischargeMosfet::decode(&self.receive_bytes(SetDischargeMosfet::reply_size()).await?)
            .with_context(|| "Cannot set discharge mosfet")
    }

    pub async fn set_charge_mosfet(&mut self, enable: bool) -> Result<()> {
        self.send_bytes(&SetChargeMosfet::request(Address::Host, enable))
            .await?;
        SetChargeMosfet::decode(&self.receive_bytes(SetChargeMosfet::reply_size()).await?)
            .with_context(|| "Cannot set charge mosfet")
    }

    pub async fn set_soc(&mut self, soc_percent: f32) -> Result<()> {
        self.send_bytes(&SetSoc::request(Address::Host, soc_percent))
            .await?;
        SetSoc::decode(&self.receive_bytes(SetSoc::reply_size()).await?)
            .with_context(|| "Cannot set SOC")
    }

    pub async fn reset(&mut self) -> Result<()> {
        self.send_bytes(&BmsReset::request(Address::Host)).await?;
        BmsReset::decode(&self.receive_bytes(BmsReset::reply_size()).await?)
            .with_context(|| "Cannot reset BMS")
    }
}
