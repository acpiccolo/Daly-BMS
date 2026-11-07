# Unimplemented / Proposed Daly BMS Commands Research

This document collects reverse-engineered commands, research references, and byte definitions aggregated from open-source projects for Daly BMS firmware variants that are **not** currently implemented in `dalybms_lib`.

---

## Command Reference Summary

| Command Code | Name / Purpose | Verification Status | Reference Source |
|---|---|---|---|
| `0x01` | Reboot / Reset BMS MCU | Not Tested | [Daly UART Protocol V1.2](https://github.com/acpiccolo/Daly-BMS/blob/main/docs/Daly%20UART_485%20Communications%20Protocol%20V1.2.pdf) |
| `0x02` | Force BMS Deep Sleep Mode | Not Tested | [Daly UART Protocol V1.2](https://github.com/acpiccolo/Daly-BMS/blob/main/docs/Daly%20UART_485%20Communications%20Protocol%20V1.2.pdf) |
| `0x10` / `0x20` | Cell Overvoltage Protection / Release (mV) | Not Tested | [VenusOS dbus-serialbattery (`daly.py`)](https://github.com/Louisvdw/dbus-serialbattery/blob/master/etc/dbus-serialbattery/bms/daly.py#L180-L220) |
| `0x11` / `0x21` | Cell Undervoltage Protection / Release (mV) | Not Tested | [VenusOS dbus-serialbattery (`daly.py`)](https://github.com/Louisvdw/dbus-serialbattery/blob/master/etc/dbus-serialbattery/bms/daly.py#L180-L220) |
| `0x12` / `0x22` | Pack Overvoltage Protection / Release (100mV) | Not Tested | [VenusOS dbus-serialbattery (`daly.py`)](https://github.com/Louisvdw/dbus-serialbattery/blob/master/etc/dbus-serialbattery/bms/daly.py#L180-L220) |
| `0x13` / `0x23` | Pack Undervoltage Protection / Release (100mV) | Not Tested | [VenusOS dbus-serialbattery (`daly.py`)](https://github.com/Louisvdw/dbus-serialbattery/blob/master/etc/dbus-serialbattery/bms/daly.py#L180-L220) |
| `0x14` / `0x24` | Charge Overcurrent Protection Threshold | Not Tested | [VenusOS dbus-serialbattery (`daly.py`)](https://github.com/Louisvdw/dbus-serialbattery/blob/master/etc/dbus-serialbattery/bms/daly.py#L180-L220) |
| `0x15` / `0x25` | Discharge Overcurrent Protection Threshold | Not Tested | [VenusOS dbus-serialbattery (`daly.py`)](https://github.com/Louisvdw/dbus-serialbattery/blob/master/etc/dbus-serialbattery/bms/daly.py#L180-L220) |
| `0x16` / `0x26` | Balance Start Voltage (mV) & Delta Threshold | Not Tested | [Daly UART Protocol V1.2](https://github.com/acpiccolo/Daly-BMS/blob/main/docs/Daly%20UART_485%20Communications%20Protocol%20V1.2.pdf) |
| `0x2F` | Set Nominal Pack Capacity (Ah) | Not Tested | [VenusOS dbus-serialbattery (`daly.py`)](https://github.com/Louisvdw/dbus-serialbattery/blob/master/etc/dbus-serialbattery/bms/daly.py#L180-L200) |
| `0x99` | Rated Capacity & Parameter Overview (Legacy V1.2) | Not Tested | [ESPHome Daly BMS Component](https://github.com/esphome/esphome/blob/dev/esphome/components/daly_bms/daly_bms.cpp#L120-L150) |

---

## Detailed Byte Layouts (Proposed / Unimplemented)

### 1. Protection Threshold Settings (`0x10` .. `0x26`)

* **References:** [VenusOS dbus-serialbattery](https://github.com/Louisvdw/dbus-serialbattery/blob/master/etc/dbus-serialbattery/bms/daly.py#L180-L220)
* **Cell Overvoltage (`0x10` Read / `0x20` Write):**
  - `DATA0..1`: Overvoltage Trigger Voltage (`u16` BE, mV, e.g. `3650` = 3.650V)
  - `DATA2..3`: Overvoltage Release Voltage (`u16` BE, mV, e.g. `3550` = 3.550V)
  - `DATA4`: Protection Delay (seconds)
* **Cell Undervoltage (`0x11` Read / `0x21` Write):**
  - `DATA0..1`: Undervoltage Trigger Voltage (`u16` BE, mV, e.g. `2500` = 2.500V)
  - `DATA2..3`: Undervoltage Release Voltage (`u16` BE, mV, e.g. `2800` = 2.800V)
  - `DATA4`: Protection Delay (seconds)
* **Balance Start Voltage & Delta (`0x16` Read / `0x26` Write):**
  - `DATA0..1`: Balance Start Voltage (`u16` BE, mV, e.g. `3400` = 3.400V)
  - `DATA2..3`: Balance Start Differential Voltage (`u16` BE, mV, e.g. `30` = 0.030V)

### 2. Sleep & Reboot Commands (`0x01` / `0x02`)

* **References:** [Daly UART Protocol V1.2 (Sec. 4)](https://github.com/acpiccolo/Daly-BMS/blob/main/docs/Daly%20UART_485%20Communications%20Protocol%20V1.2.pdf)
* **Reset MCU (`0x01`):** Payload `0x00...0x00`. Causes the BMS MCU to reset.
* **Deep Sleep (`0x02`):** Payload `0x00...0x00`. Puts the BMS into low-power sleep mode until woken by load or charge detection.

### 3. Parameter Overview (`0x99`)

* **References:** [ESPHome Daly BMS Component](https://github.com/esphome/esphome/blob/dev/esphome/components/daly_bms/daly_bms.cpp#L120-L150)
* Returns aggregated configuration parameters on older firmware (V1.2) where `0x50` is reserved for setting the clock.
