//! All numeric SMC sensors, plus fan state. Labels per chip come later, so keys are raw for now.

use serde::Serialize;
use smc::Smc;
use std::cell::{Cell, RefCell};

/// Each sensor read is a kernel call, and sensors are the app's biggest idle cost. So most
/// checks only read the sensors that were hottest at the last full sweep.
const FULL_SWEEP_EVERY: u32 = 20;
const HOT_CANDIDATES: usize = 24;
const HOTTEST_MIN_C: f32 = 15.0;
const HOTTEST_MAX_C: f32 = 115.0;

/// Positions of the `count` largest values, largest first.
pub fn top_positions(values: &[(usize, f32)], count: usize) -> Vec<usize> {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1));
    sorted.into_iter().take(count).map(|(position, _)| position).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Temperature,
    Voltage,
    Current,
    Power,
}

/// SMC key prefixes. I and P on Apple Silicon are current and power, by convention, not documented.
pub fn kind_of(key: &str) -> Option<Kind> {
    match key.as_bytes().first()? {
        b'T' => Some(Kind::Temperature),
        b'V' => Some(Kind::Voltage),
        b'I' => Some(Kind::Current),
        b'P' => Some(Kind::Power),
        _ => None,
    }
}

/// Unconnected sensors read as zero or absurd values, so temperatures get a sanity range.
pub fn is_plausible(kind: Kind, value: f32) -> bool {
    value.is_finite()
        && match kind {
            Kind::Temperature => (5.0..=150.0).contains(&value),
            Kind::Voltage | Kind::Current | Kind::Power => value.abs() < 10_000.0,
        }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reading {
    pub key: String,
    pub kind: Kind,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fan {
    pub index: u8,
    pub mode: u8,
    pub target_rpm: f32,
    pub actual_rpm: f32,
    pub min_rpm: f32,
    pub max_rpm: f32,
}

/// Remembers which keys are numeric so later reads skip scanning all the SMC's keys.
pub struct Sensors {
    smc: Smc,
    keys: Vec<(String, u32, Kind)>,
    candidates: RefCell<Vec<usize>>,
    checks: Cell<u32>,
}

impl Sensors {
    pub fn open() -> Result<Sensors, smc::Error> {
        let smc = Smc::open()?;
        let mut keys = Vec::new();
        for index in 0..smc.key_count()? {
            let Ok(key) = smc.key_at(index) else { continue };
            if let Some(kind) = kind_of(&key)
                && smc.read_f32(&key).is_ok()
            {
                keys.push((key.clone(), smc::fourcc(&key)?, kind));
            }
        }
        Ok(Sensors { smc, keys, candidates: RefCell::default(), checks: Cell::new(0) })
    }

    pub fn read(&self) -> Vec<Reading> {
        self.read_where(|_| true)
    }

    /// Only touches the requested kinds, since each sensor read is an IOKit call.
    pub fn read_kind(&self, wanted: Kind) -> Vec<Reading> {
        self.read_where(|kind| kind == wanted)
    }

    fn read_where(&self, wanted: impl Fn(Kind) -> bool) -> Vec<Reading> {
        self.keys
            .iter()
            .filter(|(_, _, kind)| wanted(*kind))
            .filter_map(|(key, code, kind)| {
                let value = self.smc.read_known_f32(*code).ok()?;
                is_plausible(*kind, value).then(|| Reading {
                    key: key.clone(),
                    kind: *kind,
                    value,
                })
            })
            .collect()
    }

    fn temperature_at(&self, position: usize) -> Option<f32> {
        let (_, code, kind) = self.keys.get(position)?;
        if *kind != Kind::Temperature {
            return None;
        }
        self.smc
            .read_known_f32(*code)
            .ok()
            .filter(|c| (HOTTEST_MIN_C..=HOTTEST_MAX_C).contains(c))
    }

    pub fn hottest(&self) -> Option<(String, f32)> {
        let check = self.checks.get();
        self.checks.set(check.wrapping_add(1));
        let positions: Vec<usize> = if check.is_multiple_of(FULL_SWEEP_EVERY) {
            (0..self.keys.len()).collect()
        } else {
            self.candidates.borrow().clone()
        };
        let readings: Vec<(usize, f32)> = positions
            .into_iter()
            .filter_map(|position| self.temperature_at(position).map(|c| (position, c)))
            .collect();
        if check.is_multiple_of(FULL_SWEEP_EVERY) {
            *self.candidates.borrow_mut() = top_positions(&readings, HOT_CANDIDATES);
        }
        readings
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(position, c)| (self.keys[position].0.clone(), c))
    }

    pub fn fans(&self) -> Result<Vec<Fan>, smc::Error> {
        let mut fans = Vec::new();
        for index in 0..self.smc.read_u8("FNum")? {
            let float = |suffix: &str| self.smc.read_f32(&format!("F{index}{suffix}"));
            fans.push(Fan {
                index,
                mode: self.smc.read_u8(&format!("F{index}Md"))?,
                target_rpm: float("Tg")?,
                actual_rpm: float("Ac")?,
                min_rpm: float("Mn")?,
                max_rpm: float("Mx")?,
            });
        }
        Ok(fans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_key_prefix() {
        assert_eq!(kind_of("Tp0c"), Some(Kind::Temperature));
        assert_eq!(kind_of("VP0R"), Some(Kind::Voltage));
        assert_eq!(kind_of("IBAC"), Some(Kind::Current));
        assert_eq!(kind_of("PSTR"), Some(Kind::Power));
        assert_eq!(kind_of("F0Ac"), None);
        assert_eq!(kind_of(""), None);
    }

    #[test]
    fn rejects_implausible_values() {
        assert!(is_plausible(Kind::Temperature, 45.0));
        assert!(!is_plausible(Kind::Temperature, 1.0));
        assert!(!is_plausible(Kind::Temperature, 129.0 + 100.0));
        assert!(!is_plausible(Kind::Power, f32::NAN));
        assert!(is_plausible(Kind::Current, -2.5));
    }

    #[test]
    fn top_positions_are_the_largest_first() {
        assert_eq!(top_positions(&[(4, 40.0), (7, 90.0), (2, 60.0)], 2), [7, 2]);
        assert!(top_positions(&[], 2).is_empty());
    }

    #[test]
    fn quick_checks_agree_with_a_full_sweep() {
        let sensors = Sensors::open().unwrap();
        let (_, full) = sensors.hottest().unwrap();
        let (_, quick) = sensors.hottest().unwrap();
        assert!((full - quick).abs() < 25.0, "{full} vs {quick}");
    }

    #[test]
    fn reads_this_macs_sensors_and_fans() {
        let sensors = Sensors::open().unwrap();
        assert!(sensors.read().iter().any(|r| r.kind == Kind::Temperature));
        let temperatures = sensors.read_kind(Kind::Temperature);
        assert!(!temperatures.is_empty());
        assert!(temperatures.iter().all(|r| r.kind == Kind::Temperature));
        let fans = sensors.fans().unwrap();
        assert_eq!(fans.len(), 2);
        assert!(fans[0].max_rpm > fans[0].min_rpm);
    }
}
