# Daly BMS Additional Communication Commands & Byte Layouts

> **Disclaimer:** The commands and byte structures documented in this specification have been aggregated from various open-source projects, community reverse-engineering efforts, and manufacturer documentation. Some commands may vary across different Daly BMS hardware revisions and firmware versions (e.g., standard UART V1.2 vs. DL_HKMS firmware series) and may not be fully verified on all hardware models.

---

## Command Overview Table

| CMD | Description | Tested / Verification | Primary Source / Reference |
|---|---|---|---|
| `0x01` | System Sleep / Deep Power Down | Not Tested | Daly Upper Computer Protocol |
| `0x02` | MCU Wakeup Command | Not Tested | Daly Upper Computer Protocol |
| `0x10` / `0x20` | Cell Overvoltage Protection Threshold | Not Tested | Daly UART Protocol V1.2 (Sec. 4) |
| `0x11` / `0x21` | Cell Undervoltage Protection Threshold | Not Tested | Daly UART Protocol V1.2 (Sec. 4) |
| `0x12` / `0x22` | Pack / Sum Overvoltage Threshold | Not Tested | Daly UART Protocol V1.2 (Sec. 4) |
| `0x13` / `0x23` | Pack / Sum Undervoltage Threshold | Not Tested | Daly UART Protocol V1.2 (Sec. 4) |
| `0x14` / `0x24` | Charge Overcurrent Protection Threshold | Not Tested | Daly UART Protocol V1.2 (Sec. 4) |
| `0x15` / `0x25` | Discharge Overcurrent Protection Threshold | Not Tested | Daly UART Protocol V1.2 (Sec. 4) |
| `0x16` / `0x26` | Balance Start Voltage & Delta | Not Tested | Daly UART Protocol V1.2 (Sec. 4) |
| `0x2F` | Set Nominal Pack Capacity (Ah) | Not Tested | VenusOS Daly BMS / dbus-serialbattery |
| `0x50` | RTC Time (Legacy V1.2) / Rated Capacity (DL_HKMS) | Not Tested | Daly Protocol V1.2 & `dignifiedquire/Daly-BMS` |
| `0x51` | Hardware & Firmware Info / Board Config | Not Tested | Daly Protocol V1.2 & `dignifiedquire/Daly-BMS` |
| `0x52` | Serial Number (Legacy) / Lifetime Accumulated Ah (DL_HKMS) | Not Tested | Daly Protocol V1.2 & `dignifiedquire/Daly-BMS` |
| `0x53` | Production Date & Raw Battery Operating Info | Not Tested | `dignifiedquire/Daly-BMS` |
| `0x57` | Multi-frame Serial Number / Battery Code | Not Tested | `dignifiedquire/Daly-BMS` |
| `0x61` | Real-Time Clock (RTC) Date & Time | Not Tested | `dignifiedquire/Daly-BMS` |
| `0x62` | Software / Firmware Version String (ASCII) | Not Tested | `dignifiedquire/Daly-BMS` |
| `0x63` | Hardware Version String (ASCII) | Not Tested | `dignifiedquire/Daly-BMS` |
| `0x94` | Status Frame (Byte 7 MOSFET Temp Variant) | Not Tested | `dignifiedquire/Daly-BMS` |
| `0x99` | Rated Capacity & Parameter Overview | Not Tested | Daly UART Protocol V1.2 (Sec. 3.10) |

---

## Standard Frame Structure

All Daly BMS UART/RS485 frames consist of 13 bytes:

| Byte Index | Field Name | Description |
|---|---|---|
| `0` | Start Byte | Fixed header byte `0xA5` |
| `1` | Address | `0x40` (Host) or `0x80` / `0x01` (BMS) |
| `2` | Command Code | Command ID byte (e.g. `0x90`, `0x50`, `0x61`) |
| `3` | Data Length | Fixed payload size `0x08` (8 bytes) |
| `4..11` | Payload Bytes | `DATA0` to `DATA7` |
| `12` | Checksum | Sum modulo 256 of bytes 0 to 11 |

---

## Detailed Command Specifications & Byte Layouts

### 1. Rated Capacity & Nominal Voltage (`0x50` / `0x99`)

* **Tested / Verification:** Not Tested
* **Description:** Depending on BMS firmware variant:
  - On **DL_HKMS** series: `0x50` returns rated capacity and nominal cell voltage.
  - On **Legacy V1.2**: `0x50` is used for RTC setting, while `0x99` returns rated capacity.
* **Request (`Host -> BMS`):** `[0xA5, 0x40, 0x50, 0x08, 0, 0, 0, 0, 0, 0, 0, 0, CRC]`
* **Response Payload (`DATA0..DATA7`):**
  - `DATA0..3`: Rated / Design Capacity in milli-Ah (u32 BE, divide by `1000.0` for Ah)
  - `DATA4..7`: Nominal Cell Voltage in milli-Volts (u32 BE, divide by `1000.0` for V)

---

### 2. Lifetime Accumulated Charge & Discharge (`0x52`)

* **Tested / Verification:** Not Tested
* **Description:** On newer firmware (e.g. DL_HKMS), `0x52` returns cumulative lifetime Ampere-hours charged and discharged.
* **Request (`Host -> BMS`):** `[0xA5, 0x40, 0x52, 0x08, 0, 0, 0, 0, 0, 0, 0, 0, CRC]`
* **Response Payload (`DATA0..DATA7`):**
  - `DATA0..3`: Total Lifetime Charged Ah (u32 BE, divide by `1000.0` or `1.0` depending on scaling)
  - `DATA4..7`: Total Lifetime Discharged Ah (u32 BE, divide by `1000.0` or `1.0`)

---

### 3. Production Date & Operating Parameters (`0x53`)

* **Tested / Verification:** Not Tested
* **Description:** Reads production date and raw operating configuration flags.
* **Request (`Host -> BMS`):** `[0xA5, 0x40, 0x53, 0x08, 0, 0, 0, 0, 0, 0, 0, 0, CRC]`
* **Response Payload (`DATA0..DATA7`):**
  - `DATA0`: Production Year (e.g. `22` = 2022)
  - `DATA1`: Production Month (`1..12`)
  - `DATA2`: Production Day (`1..31`)
  - `DATA3`: Operating mode / battery type flag byte
  - `DATA4`: Auto-sleep delay setting
  - `DATA5..7`: Reserved

---

### 4. Multi-Frame Serial / Battery Code String (`0x57`)

* **Tested / Verification:** Not Tested
* **Description:** Reads battery code string across 5 concatenated response frames.
* **Request (`Host -> BMS`):** `[0xA5, 0x40, 0x57, 0x08, 0, 0, 0, 0, 0, 0, 0, 0, CRC]`
* **Response Payload:**
  - Received across 5 frames with frame index in `DATA0` (`1..5`).
  - `DATA1..7` of each frame contains ASCII string fragments.

---

### 5. Real-Time Clock (RTC) Date & Time (`0x61` / `0x50`)

* **Tested / Verification:** Not Tested
* **Description:** On modern firmware (DL_HKMS), `0x61` reads or sets the BMS internal clock.
* **Request Payload (`DATA0..DATA7`):**
  - `DATA0`: Year (e.g. `24` for 2024)
  - `DATA1`: Month (`1..12`)
  - `DATA2`: Day of Month (`1..31`)
  - `DATA3`: Hour (`0..23`)
  - `DATA4`: Minute (`0..59`)
  - `DATA5`: Second (`0..59`)
  - `DATA6..7`: `0x00`

---

### 6. Software & Hardware Version Strings (`0x62` / `0x63`)

* **Tested / Verification:** Not Tested
* **Description:**
  - `0x62`: Software/Firmware Version ASCII String (multi-frame)
  - `0x63`: Hardware Version ASCII String (multi-frame)
* **Request (`Host -> BMS`):** `[0xA5, 0x40, <0x62|0x63>, 0x08, 0, 0, 0, 0, 0, 0, 0, 0, CRC]`
* **Response Payload:** `DATA0` contains frame sequence number; `DATA1..7` contains ASCII string.

---

### 7. Status Frame Byte 7 MOSFET Temperature Variant (`0x94`)

* **Tested / Verification:** Not Tested
* **Description:** Standard status command `0x94`. On select firmware variants, `DATA7` (byte index 7) reports the MOSFET / board temperature in degrees Celsius with a -40°C offset.
* **Conversion:** `temperature = (DATA7 as i16) - 40` (Valid when `DATA7 > 0`).

---

### 8. Protection Threshold Settings (`0x10` .. `0x26`)

* **Tested / Verification:** Not Tested
* **Cell Overvoltage (`0x10` Read / `0x20` Write):**
  - `DATA0..1`: Overvoltage Trigger Voltage (u16 BE, mV, e.g. `3650` = 3.650V)
  - `DATA2..3`: Overvoltage Release Voltage (u16 BE, mV, e.g. `3550` = 3.550V)
  - `DATA4`: Protection Delay (seconds)
* **Cell Undervoltage (`0x11` Read / `0x21` Write):**
  - `DATA0..1`: Undervoltage Trigger Voltage (u16 BE, mV, e.g. `2500` = 2.500V)
  - `DATA2..3`: Undervoltage Release Voltage (u16 BE, mV, e.g. `2800` = 2.800V)
  - `DATA4`: Protection Delay (seconds)
* **Balance Start Voltage & Delta (`0x16` Read / `0x26` Write):**
  - `DATA0..1`: Balance Start Voltage (u16 BE, mV, e.g. `3400` = 3.400V)
  - `DATA2..3`: Balance Start Differential Voltage (u16 BE, mV, e.g. `30` = 0.030V)
