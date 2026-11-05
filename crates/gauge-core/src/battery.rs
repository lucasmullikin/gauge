//! Battery state from the AppleSmartBattery registry entry. Desktops simply report none.

use crate::iokit;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Battery {
    pub percent: Option<i64>,
    pub health_percent: Option<f64>,
    pub cycle_count: Option<i64>,
    pub voltage_mv: Option<i64>,
    pub amperage_ma: Option<i64>,
    pub power_watts: Option<f64>,
    pub temperature_c: Option<f32>,
    pub charging: Option<bool>,
    pub external_power: Option<bool>,
    pub fully_charged: Option<bool>,
    pub minutes_remaining: Option<i64>,
    pub minutes_to_full: Option<i64>,
    pub design_cycles: Option<i64>,
    pub nominal_mah: Option<i64>,
    pub design_mah: Option<i64>,
    pub remaining_mah: Option<i64>,
    pub adapter: Option<Adapter>,
    /// What's coming into the Mac right now. Zero or missing when unplugged.
    pub input_watts: Option<f64>,
    pub input_volts: Option<f64>,
    pub input_amps: Option<f64>,
    /// What the whole Mac is using right now.
    pub load_watts: Option<f64>,
    /// macOS's estimate of what's lost in the adapter.
    pub adapter_loss_watts: Option<f64>,
    /// Power sent out to USB-C devices.
    pub usb_out_watts: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Adapter {
    pub name: Option<String>,
    pub manufacturer: Option<String>,
    pub rated_watts: Option<i64>,
    pub volts: Option<f64>,
    pub amps: Option<f64>,
}

/// macOS reports these in thousandths: milliwatts, millivolts and milliamps.
pub fn from_milli(value: Option<i64>) -> Option<f64> {
    value.map(|v| v as f64 / 1000.0)
}

/// How much of the adapter's rating is in use.
pub fn adapter_load_percent(input_watts: Option<f64>, rated_watts: Option<i64>) -> Option<f64> {
    let (input, rated) = (input_watts?, rated_watts?);
    (rated > 0).then(|| input / rated as f64 * 100.0)
}

const UNKNOWN_TIME: i64 = 65535;
const BATTERY_TEMPERATURE_KEY: &str = "TB0T";

/// Nominal over design capacity gives the same 84% System Information shows on this Mac.
pub fn health_percent(nominal_mah: Option<i64>, design_mah: Option<i64>) -> Option<f64> {
    let (nominal, design) = (nominal_mah?, design_mah?);
    (design > 0).then(|| nominal as f64 / design as f64 * 100.0)
}

/// Negative while discharging, positive while charging.
pub fn power_watts(voltage_mv: Option<i64>, amperage_ma: Option<i64>) -> Option<f64> {
    Some(voltage_mv? as f64 * amperage_ma? as f64 / 1_000_000.0)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceBattery {
    pub name: String,
    pub percent: i64,
}

/// Wireless keyboards, mice and trackpads that report a charge. Headphones often don't appear
/// here, since macOS doesn't list them in this part of the registry.
pub fn read_devices() -> Vec<DeviceBattery> {
    iokit::services_of_class("AppleDeviceManagementHIDEventService")
        .iter()
        .filter_map(|service| {
            Some(DeviceBattery {
                percent: service.i64("BatteryPercent")?,
                name: iokit::as_string(&service.property("Product")?)?,
            })
        })
        .collect()
}

pub fn read() -> Option<Battery> {
    let services = iokit::services_of_class("AppleSmartBattery");
    let entry = services.first()?;
    let voltage_mv = entry.i64("Voltage");
    let telemetry = entry.property("PowerTelemetryData");
    let telemetry_value = |key: &str| telemetry.as_ref().and_then(|t| iokit::dict_i64(t, key));
    let details = entry.property("AdapterDetails");
    let adapter = details.as_ref().map(|d| Adapter {
        name: iokit::dict_string(d, "Name"),
        manufacturer: iokit::dict_string(d, "Manufacturer"),
        rated_watts: iokit::dict_i64(d, "Watts"),
        volts: from_milli(iokit::dict_i64(d, "AdapterVoltage")),
        amps: from_milli(iokit::dict_i64(d, "Current")),
    });
    let external = entry.bool("ExternalConnected");
    let plugged_in = external.unwrap_or(false);
    let usb_out_watts = entry.property("PowerOutDetails").map(|ports| {
        iokit::array_items(&ports)
            .iter()
            .filter_map(|port| from_milli(iokit::dict_i64(port, "Watts")))
            .sum()
    });
    let amperage_ma = entry.i64("InstantAmperage").or_else(|| entry.i64("Amperage"));
    let data = entry.property("BatteryData");
    let capacity = |key: &str| data.as_ref().and_then(|d| iokit::dict_i64(d, key));
    Some(Battery {
        percent: entry.i64("CurrentCapacity"),
        health_percent: health_percent(capacity("NominalChargeCapacity"), capacity("DesignCapacity")),
        cycle_count: entry.i64("CycleCount"),
        voltage_mv,
        amperage_ma,
        power_watts: power_watts(voltage_mv, amperage_ma),
        temperature_c: smc::Smc::open().ok().and_then(|s| s.read_f32(BATTERY_TEMPERATURE_KEY).ok()),
        charging: entry.bool("IsCharging"),
        external_power: external,
        fully_charged: entry.bool("FullyCharged"),
        minutes_remaining: entry.i64("TimeRemaining").filter(|m| *m != UNKNOWN_TIME),
        minutes_to_full: entry.i64("AvgTimeToFull").filter(|m| *m != UNKNOWN_TIME),
        design_cycles: entry.i64("DesignCycleCount9C"),
        nominal_mah: capacity("NominalChargeCapacity"),
        design_mah: capacity("DesignCapacity"),
        remaining_mah: capacity("RemainingCapacity"),
        adapter: adapter.filter(|_| plugged_in),
        input_watts: from_milli(telemetry_value("SystemPowerIn")).filter(|_| plugged_in),
        input_volts: from_milli(telemetry_value("SystemVoltageIn")).filter(|_| plugged_in),
        input_amps: from_milli(telemetry_value("SystemCurrentIn")).filter(|_| plugged_in),
        load_watts: from_milli(telemetry_value("SystemLoad")),
        adapter_loss_watts: from_milli(telemetry_value("AdapterEfficiencyLoss")).filter(|_| plugged_in),
        usb_out_watts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousandths_become_whole_units() {
        assert_eq!(from_milli(Some(56_453)), Some(56.453));
        assert_eq!(from_milli(None), None);
    }

    #[test]
    fn adapter_load_compares_input_to_the_rating() {
        assert_eq!(adapter_load_percent(Some(47.0), Some(94)), Some(50.0));
        assert_eq!(adapter_load_percent(Some(47.0), Some(0)), None);
        assert_eq!(adapter_load_percent(None, Some(94)), None);
    }

    #[test]
    fn health_compares_real_capacity_to_design() {
        assert_eq!(health_percent(Some(4000), Some(5000)), Some(80.0));
        assert_eq!(health_percent(Some(4000), Some(0)), None);
        assert_eq!(health_percent(None, Some(5000)), None);
    }

    #[test]
    fn battery_health_is_sensible_where_there_is_one() {
        if let Some(health) = read().and_then(|b| b.health_percent) {
            assert!((10.0..=120.0).contains(&health), "{health}");
        }
    }

    #[test]
    fn power_is_signed_by_direction() {
        assert_eq!(power_watts(Some(12000), Some(-1000)), Some(-12.0));
        assert_eq!(power_watts(Some(12000), None), None);
    }
}
