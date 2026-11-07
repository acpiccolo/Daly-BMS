# Implemented Additional Commands Specification

This document details the newly added and implemented Daly BMS protocol commands that extend beyond the standard `0x90`–`0x98` status/measurement queries.

All commands documented here are fully implemented in `dalybms_lib` (both synchronous and asynchronous clients), integrated into the CLI, and monitored in the daemon.

---

## Command Overview

| Command Code | Name | Description | Response Frames | Implementation |
|---|---|---|---|---|
| `0x50` | `RatedCapacity` | Rated (design) pack capacity and nominal cell voltage | 1 frame | `dalybms_lib::protocol::RatedCapacity` |
| `0x51` | `BmsConfig` | Board number, series cell count, temperature sensor count | 1 frame | `dalybms_lib::protocol::BmsConfig` |
| `0x52` | `BatteryStat` | Lifetime cumulative charged and discharged Ampere-hours (Ah) | 1 frame | `dalybms_lib::protocol::BatteryStat` |
| `0x53` | `BatteryInfo` | Production date, raw operating mode, battery chemistry type, auto-sleep minutes | 1 frame | `dalybms_lib::protocol::BatteryInfo` |
| `0x57` | `BatteryCode` | Serial number / battery identification string (ASCII) | 5 frames | `dalybms_lib::protocol::BatteryCode` |
| `0x61` | `Rtc` | BMS internal Real-Time Clock date and time | 1 frame | `dalybms_lib::protocol::Rtc` |
| `0x62` | `SoftwareVersion` | Master / software firmware version string (ASCII) | 2 frames | `dalybms_lib::protocol::SoftwareVersion` |
| `0x63` | `HardwareVersion` | BMS hardware version string (ASCII) | 2 frames | `dalybms_lib::protocol::HardwareVersion` |
| `0x94` (ext) | `Status` | MOSFET board temperature parsed from byte 7 (offset -40°C) | 1 frame | `dalybms_lib::protocol::Status::mosfet_temperature` |

---

## Standard Frame Structure

All Daly BMS UART/RS485 frames consist of 13 bytes:

| Byte Index | Field Name | Value / Description |
|---|---|---|
| `0` | Start Byte | Fixed header byte `0xA5` |
| `1` | Address | `0x40` (Host request) or `0x01` / `0x80` (BMS response) |
| `2` | Command Code | Command ID byte (e.g. `0x50`, `0x51`, `0x52`, `0x53`, `0x57`, `0x61`, `0x62`, `0x63`) |
| `3` | Data Length | Fixed payload size `0x08` (8 bytes) |
| `4..11` | Payload Bytes | `DATA0` to `DATA7` |
| `12` | Checksum | Sum modulo 256 of bytes 0 to 11 (`calc_crc`) |

---

## Detailed Command Specifications & Byte Layouts

### 1. Rated Capacity & Nominal Voltage (`0x50`)

* **Struct:** `dalybms_lib::protocol::RatedCapacity`
* **CLI Command:** `dalybms rated-capacity`
* **Daemon Metric:** `rated-capacity`
* **Request (`Host -> BMS`):**
  ```text
  [0xA5, 0x40, 0x50, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3D]
  ```
* **Response Payload (`DATA0..DATA7`):**
  - `DATA0..3` (bytes 4..7): Rated / design pack capacity in milli-Ah (`u32` Big-Endian, divide by `1000.0` for Ah).
  - `DATA4..7` (bytes 8..11): Nominal cell voltage in milli-Volts (`u32` Big-Endian, divide by `1000.0` for V).

---

### 2. BMS Configuration (`0x51`)

* **Struct:** `dalybms_lib::protocol::BmsConfig`
* **CLI Command:** `dalybms bms-config`
* **Daemon Metric:** `bms-config`
* **Request (`Host -> BMS`):**
  ```text
  [0xA5, 0x40, 0x51, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3E]
  ```
* **Response Payload (`DATA0..DATA7`):**
  - `DATA0` (byte 4): Board (slave) number (`u8`).
  - `DATA1` (byte 5): Number of series cells in primary string (`u8`).
  - `DATA2..3` (bytes 6..7): Parallel string cell counts / reserved.
  - `DATA4` (byte 8): Number of NTC temperature sensors (`u8`).
  - `DATA5..7` (bytes 9..11): Reserved.

---

### 3. Lifetime Accumulated Charge & Discharge (`0x52`)

* **Struct:** `dalybms_lib::protocol::BatteryStat`
* **CLI Command:** `dalybms battery-stat`
* **Daemon Metric:** `battery-stat`
* **Request (`Host -> BMS`):**
  ```text
  [0xA5, 0x40, 0x52, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3F]
  ```
* **Response Payload (`DATA0..DATA7`):**
  - `DATA0..3` (bytes 4..7): Cumulative lifetime charged Ah (`u32` Big-Endian).
  - `DATA4..7` (bytes 8..11): Cumulative lifetime discharged Ah (`u32` Big-Endian).

---

### 4. Production Date & Operating Parameters (`0x53`)

* **Struct:** `dalybms_lib::protocol::BatteryInfo`
* **CLI Command:** `dalybms battery-info`
* **Daemon Metric:** `battery-info`
* **Request (`Host -> BMS`):**
  ```text
  [0xA5, 0x40, 0x53, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40]
  ```
* **Response Payload (`DATA0..DATA7`):**
  - `DATA0` (byte 4): Raw operating mode byte (`u8`).
  - `DATA1` (byte 5): Raw battery type / chemistry byte (`u8`).
  - `DATA2` (byte 6): Production year offset from 2000 (`production_year = byte + 2000`).
  - `DATA3` (byte 7): Production month (`1..12`).
  - `DATA4` (byte 8): Production day (`1..31`).
  - `DATA5` (byte 9): Auto-sleep delay in minutes (`u8`).
  - `DATA6..7` (bytes 10..11): Reserved.

---

### 5. Multi-Frame Serial Number / Battery Code String (`0x57`)

* **Struct:** `dalybms_lib::protocol::BatteryCode`
* **CLI Command:** `dalybms battery-code`
* **Daemon Metric:** `battery-code`
* **Request (`Host -> BMS`):**
  ```text
  [0xA5, 0x40, 0x57, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x44]
  ```
* **Response Structure (5 Frames, 65 Bytes total):**
  - Arrives as 5 frames with 1-based frame index in `DATA0` (byte 4).
  - Bytes 5..11 (`DATA1..DATA7`) of each frame contain 7 ASCII characters (35 characters total).
  - Null bytes (`\0`) are stripped and collapsed to spaces.

---

### 6. Real-Time Clock (RTC) Date & Time (`0x61`)

* **Struct:** `dalybms_lib::protocol::Rtc`
* **CLI Command:** `dalybms rtc`
* **Daemon Metric:** `rtc`
* **Request (`Host -> BMS`):**
  ```text
  [0xA5, 0x40, 0x61, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4E]
  ```
* **Response Payload (`DATA0..DATA7`):**
  - `DATA0` (byte 4): Year offset from 2000 (`year = byte + 2000`).
  - `DATA1` (byte 5): Month (`1..12`).
  - `DATA2` (byte 6): Day (`1..31`).
  - `DATA3` (byte 7): Hour (`0..23`).
  - `DATA4` (byte 8): Minute (`0..59`).
  - `DATA5` (byte 9): Second (`0..59`).
  - `DATA6..7` (bytes 10..11): Reserved (`0x00`).

---

### 7. Software / Master Version String (`0x62`)

* **Struct:** `dalybms_lib::protocol::SoftwareVersion`
* **CLI Command:** `dalybms software-version`
* **Daemon Metric:** `software-version`
* **Request (`Host -> BMS`):**
  ```text
  [0xA5, 0x40, 0x62, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4F]
  ```
* **Response Structure (2 Frames, 26 Bytes total):**
  - Arrives as 2 frames with frame index in `DATA0` (byte 4).
  - Bytes 5..11 (`DATA1..DATA7`) of each frame contain 7 ASCII characters (14 characters total, e.g. `"12_250520_BRST"`).

---

### 8. Hardware Version String (`0x63`)

* **Struct:** `dalybms_lib::protocol::HardwareVersion`
* **CLI Command:** `dalybms hardware-version`
* **Daemon Metric:** `hardware-version`
* **Request (`Host -> BMS`):**
  ```text
  [0xA5, 0x40, 0x63, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x50]
  ```
* **Response Structure (2 Frames, 26 Bytes total):**
  - Arrives as 2 frames with frame index in `DATA0` (byte 4).
  - Bytes 5..11 (`DATA1..DATA7`) of each frame contain 7 ASCII characters (14 characters total, e.g. `"H2.1_103E_30XF"`).

---

### 9. Status Frame MOSFET Temperature (`0x94`, Byte 7)

* **Field:** `dalybms_lib::protocol::Status::mosfet_temperature: Option<i8>`
* **Description:**
  - Standard status command (`0x94`).
  - On supported firmware versions (e.g. DL_HKMS series), byte index 7 (`DATA3`) reports the internal MOSFET / board temperature in degrees Celsius with a -40°C offset.
  - When byte 7 is non-zero, `mosfet_temperature = Some((byte7 as i16 - 40) as i8)`. When zero (unsupported firmware), it is `None`.
