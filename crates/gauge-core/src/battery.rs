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
        external_power: entry.bool("ExternalConnected"),
        fully_charged: entry.bool("FullyCharged"),
        minutes_remaining: entry.i64("TimeRemaining").filter(|m| *m != UNKNOWN_TIME),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
