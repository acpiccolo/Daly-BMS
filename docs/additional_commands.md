# Daly BMS Additional Communication Commands & Byte Layouts

This document details additional Daly BMS protocol commands beyond those implemented in the core library, including their exact frame structures, byte layouts, data conversions, and references/sources.

---

## Frame Structure Overview

All Daly BMS UART/RS485 frames follow a fixed 13-byte structure:

| Byte Index | Field | Description |
|---|---|---|
| `0` | Start Byte | `0xA5` (Fixed) |
| `1` | Address | `0x40` (Host) or `0x80` (BMS) |
| `2` | Command (CMD) | Command identifier byte (e.g. `0x90`, `0x50`, `0x10`) |
| `3` | Data Length | `0x08` (Fixed 8 bytes of payload) |
| `4..11` | Data Payload | 8 data bytes (`DATA0` .. `DATA7`) |
| `12` | Checksum (CRC) | Sum of bytes 0 to 11 modulo 256 (`SUM(Byte[0..11]) % 256`) |

---

## 1. RTC Date & Time Setting & Reading (`0x50`)

Used to read or synchronize the real-time clock inside the BMS.

* **Request Frame (`Host -> BMS`):**
  - `CMD`: `0x50`
  - Payload:
    - `DATA0`: Year (e.g., `24` for 2024)
    - `DATA1`: Month (`1..12`)
    - `DATA2`: Day of Month (`1..31`)
    - `DATA3`: Hour (`0..23`)
    - `DATA4`: Minute (`0..59`)
    - `DATA5`: Second (`0..59`)
    - `DATA6..7`: `0x00`
* **Response Frame (`BMS -> Host`):** Echoes the current or newly set date/time in the same byte layout.
* **Sources / References:**
  - *Daly UART/RS485 Protocol V1.2* (`docs/Daly UART_485 Communications Protocol V1.2.pdf`, Section 3.1)
  - ESPHome Daly BMS Integration (`esphome/components/daly_bms`)

---

## 2. Hardware, Software & Production Date Info (`0x51`)

Retrieves production date, hardware version, and firmware version.

* **Request Frame (`Host -> BMS`):**
  - `CMD`: `0x51`
  - Payload: All `0x00`
* **Response Frame (`BMS -> Host`):**
  - `DATA0`: Production Year (offset 2000, e.g., `21` = 2021)
  - `DATA1`: Production Month (`1..12`)
  - `DATA2`: Production Day (`1..31`)
  - `DATA3`: Hardware Version Major/Minor (`0x10` = v1.0)
  - `DATA4`: Software/Firmware Version Major
  - `DATA5`: Software/Firmware Version Minor
  - `DATA6..7`: Reserved
* **Sources / References:**
  - *Daly UART/RS485 Protocol V1.2* (Section 3.2)
  - VenusOS Daly BMS Driver (`Louisvdw/dbus-serialbattery`)

---

## 3. Device ID, Serial Number & Bluetooth Name (`0x52`)

Retrieves device string identifier / serial number in multi-frame ASCII format.

* **Request Frame (`Host -> BMS`):**
  - `CMD`: `0x52`
  - Payload: All `0x00`
* **Response Frame (`BMS -> Host`):**
  - Returned in 1 to 3 sequential frames (Frame Number in `DATA0`).
  - `DATA0`: Frame index (`0x01`, `0x02`, ...)
  - `DATA1..7`: ASCII bytes of serial number / ID string.
* **Sources / References:**
  - *Daly UART/RS485 Protocol V1.2* (Section 3.3)
  - Community protocol teardowns (e.g. *diysolarforum.com - Daly UART protocol specs*)

---

## 4. Parameter Overview & Rated Capacity (`0x99`)

Provides nominal capacity and high-level protection parameter summary.

* **Request Frame (`Host -> BMS`):**
  - `CMD`: `0x99`
  - Payload: All `0x00`
* **Response Frame (`BMS -> Host`):**
  - `DATA0..1`: Cell Nominal Voltage (u16 BE, divide by `1000.0` for Volts)
  - `DATA2..5`: Rated Capacity (u32 BE, divide by `1000.0` for Ah)
  - `DATA6`: Number of cell samples
  - `DATA7`: Number of temperature sensors
* **Sources / References:**
  - *Daly UART/RS485 Protocol V1.2* (Section 3.10)
  - VenusOS Daly BMS Driver

---

## 5. Protection Parameter Configuration Commands (`0x10` .. `0x3F`)

These commands read (`0x10`..`0x1F`) or set (`0x20`..`0x3F`) internal BMS threshold limits.

### A. Cell Overvoltage Threshold (`Read: 0x10` / `Write: 0x20`)
* **Data Payload:**
  - `DATA0..1`: Cell Overvoltage Protection Voltage (u16 BE, divide by `1000.0` for V, e.g. `3650` = 3.650V)
  - `DATA2..3`: Cell Overvoltage Release Voltage (u16 BE, divide by `1000.0` for V, e.g. `3550` = 3.550V)
  - `DATA4`: Delay time in seconds

### B. Cell Undervoltage Threshold (`Read: 0x11` / `Write: 0x21`)
* **Data Payload:**
  - `DATA0..1`: Cell Undervoltage Protection Voltage (u16 BE, divide by `1000.0` for V, e.g. `2500` = 2.500V)
  - `DATA2..3`: Cell Undervoltage Release Voltage (u16 BE, divide by `1000.0` for V, e.g. `2800` = 2.800V)
  - `DATA4`: Delay time in seconds

### C. Sum Overvoltage Threshold (`Read: 0x12` / `Write: 0x22`)
* **Data Payload:**
  - `DATA0..1`: Pack Overvoltage Protection Voltage (u16 BE, divide by `10.0` for V)
  - `DATA2..3`: Pack Overvoltage Release Voltage (u16 BE, divide by `10.0` for V)
  - `DATA4`: Delay time in seconds

### D. Sum Undervoltage Threshold (`Read: 0x13` / `Write: 0x23`)
* **Data Payload:**
  - `DATA0..1`: Pack Undervoltage Protection Voltage (u16 BE, divide by `10.0` for V)
  - `DATA2..3`: Pack Undervoltage Release Voltage (u16 BE, divide by `10.0` for V)
  - `DATA4`: Delay time in seconds

### E. Charge Overcurrent Protection (`Read: 0x14` / `Write: 0x24`)
* **Data Payload:**
  - `DATA0..1`: Charge Overcurrent Protection Current (u16 BE, offset `30000`, divide by `10.0` for Amps)
  - `DATA2`: Delay time in seconds

### F. Discharge Overcurrent Protection (`Read: 0x15` / `Write: 0x25`)
* **Data Payload:**
  - `DATA0..1`: Discharge Overcurrent Protection Current (u16 BE, offset `30000`, divide by `10.0` for Amps)
  - `DATA2`: Delay time in seconds

### G. Balance Start Voltage & Delta (`Read: 0x16` / `Write: 0x26`)
* **Data Payload:**
  - `DATA0..1`: Balance Start Voltage (u16 BE, divide by `1000.0` for V, e.g., `3400` = 3.400V)
  - `DATA2..3`: Balance Start Differential Voltage (u16 BE, divide by `1000.0` for V, e.g., `30` = 0.030V)

* **Sources / References:**
  - *Daly UART/RS485 Protocol V1.2* (Section 4: Parameter Read & Write Settings)
  - Daly Upper Computer PC Software Protocol Specification

---

## 6. System Sleep, Wakeup & Shutdown (`0x01` / `0x02`)

* **BMS Sleep / Shutdown (`0x01`):**
  - **Request:** `CMD`: `0x01`, `DATA0..7`: `0x00`
  - Forces the BMS into deep sleep / power down mode.
* **BMS Wakeup (`0x02`):**
  - Send a dummy UART pulse or frame `CMD`: `0x02` to wake up the BMS MCU from sleep mode.
* **Sources / References:**
  - Daly Upper Computer Protocol Manual
  - OpenInverter Daly BMS driver notes
