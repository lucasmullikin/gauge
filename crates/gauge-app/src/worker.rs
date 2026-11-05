//! Samples on a background thread so slow reads never block the menu bar. Everything is stored
//! in history here, whether or not a panel is open, so graphs have no holes.

use crate::fan_control::{FanDriver, FanState};
use crate::view::{DiskRate, memory_percent};
use gauge_core::alerts::AlertTracker;
use gauge_core::battery::{self, Battery, DeviceBattery};
use gauge_core::drives::{self, DiskActivity, Volume};
use gauge_core::gpu::{self, Gpu};
use gauge_core::history::{History, local_day, local_midnight, month_start_day};
use gauge_core::memory::{self, Memory, Pressure};
use gauge_core::network::{self, AppTraffic, Counters, InterfaceRate};
use gauge_core::ping;
use gauge_core::processor::{Cpu, Sampler, TopProcess};
use gauge_core::sensors::{AreaTemperature, Fan, Kind, Reading, Sensors, curve_temperature, temperature_areas};
use gauge_core::settings::{FanMode, Settings};
use gauge_core::smart::{self, DriveHealth};
use gauge_core::wifi::{self, Wifi};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TICK: Duration = Duration::from_secs(1);
const SYSTEM_EVERY: u64 = 2;
const FAN_ACTIVE_EVERY: u64 = 2;
const FAN_IDLE_EVERY: u64 = 3;
const DRIVE_EVERY: u64 = 5;
const WIFI_EVERY: u64 = 10;
const ROUTE_EVERY: u64 = 10;
const POWER_EVERY: u64 = 2;
const DEVICE_BATTERY_EVERY: u64 = 30;
const FLUSH_EVERY: u64 = 60;
const SMART_EVERY: u64 = 60;
const SENSOR_LIST_EVERY: u64 = 3;
const PING_INTERVAL: Duration = Duration::from_secs(5);
const PING_TIMEOUT: Duration = Duration::from_secs(2);
const APP_SAMPLE_SECONDS: u32 = 1;
const ALERT_HOLD_SECONDS: i64 = 30;
pub const NO_MODULE: usize = usize::MAX;

pub enum Update {
    Network(Vec<InterfaceRate>),
    Totals((u64, u64), (u64, u64)),
    Apps(Vec<AppTraffic>),
    Wifi(Option<Wifi>),
    Ping(Option<f32>),
    Cpu(Cpu),
    Processes(Vec<TopProcess>),
    Gpu(Option<Gpu>),
    Memory(Memory),
    Drives(Vec<Volume>, Vec<DiskRate>),
    Health(Vec<DriveHealth>),
    Fans(Vec<Fan>, Option<(String, f32)>),
    FanState(FanState),
    Battery(Option<Box<Battery>>),
    Sensors(Vec<Reading>),
    Temperatures(Vec<AreaTemperature>),
    DeviceBatteries(Vec<DeviceBattery>),
    EnergyToday(f64),
}

pub struct Shared {
    pub history: Arc<Mutex<History>>,
    pub settings: Arc<Mutex<Settings>>,
    /// Index into `MODULES` of the panel that's open, or `NO_MODULE`.
    pub open_module: AtomicUsize,
    pub install_message: Mutex<Option<String>>,
    /// Reading every sensor is costly, so it only happens while their window is open.
    pub sensors_open: AtomicBool,
}

fn now_seconds() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn disk_rates(before: &[DiskActivity], after: Vec<DiskActivity>, seconds: f64) -> Vec<DiskRate> {
    after
        .into_iter()
        .map(|disk| {
            let previous = before.iter().find(|b| b.bsd_name == disk.bsd_name);
            DiskRate {
                read_per_sec: previous.map_or(0.0, |p| network::rate(p.bytes_read, disk.bytes_read, seconds)),
                write_per_sec: previous.map_or(0.0, |p| network::rate(p.bytes_written, disk.bytes_written, seconds)),
                disk,
            }
        })
        .collect()
}

fn notify(message: &str) {
    let _ = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", "on run argv", "-e", "display notification (item 1 of argv) with title \"Gauge\"", "-e", "end run", message])
        .spawn()
        .map(|mut child| std::thread::spawn(move || child.wait()));
}

fn spawn_ping(updates: Sender<Update>, shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let mut sequence = 0u16;
        loop {
            std::thread::sleep(PING_INTERVAL);
            sequence = sequence.wrapping_add(1);
            let custom = shared.settings.lock().map(|s| s.ping_host.clone()).unwrap_or_default();
            let target = custom.parse().ok().or_else(ping::default_gateway);
            let result = target.and_then(|address| ping::ping(address, sequence, PING_TIMEOUT));
            if let (Some(ms), Ok(mut history)) = (result, shared.history.lock()) {
                history.record("ping", now_seconds(), ms);
            }
            if updates.send(Update::Ping(result)).is_err() {
                return;
            }
        }
    });
}

/// Runs for the app's lifetime. Ends quietly once the main thread has gone away.
pub fn run(updates: Sender<Update>, shared: Arc<Shared>) {
    let sensors = Sensors::open().ok();
    let mut sampler = Sampler::new();
    let mut counters: Vec<Counters> = network::read_counters().unwrap_or_default();
    let mut counters_at = Instant::now();
    let mut disks = drives::read_activity();
    let mut disks_at = Instant::now();
    let mut addresses = BTreeMap::new();
    let mut default = None;
    let mut last_smart: Option<u64> = None;
    let mut driver = FanDriver::default();
    let mut alerts = AlertTracker::default();
    let apps_running = Arc::new(AtomicBool::new(false));
    let mut tick: u64 = 0;
    spawn_ping(updates.clone(), shared.clone());
    let network_index = 0;
    let memory_index = 1;
    let cpu_index = 2;
    let drives_index = 4;
    let fans_index = 5;
    loop {
        std::thread::sleep(TICK);
        let now = now_seconds();
        let open = shared.open_module.load(Ordering::Relaxed);
        let settings = shared.settings.lock().map(|s| s.clone()).unwrap_or_default();

        if tick.is_multiple_of(ROUTE_EVERY) {
            addresses = network::read_addresses();
            default = network::default_interface();
        }
        if let Ok(current) = network::read_counters() {
            let seconds = counters_at.elapsed().as_secs_f64();
            let rates = network::rates(&counters, &current, seconds, &addresses, default.as_deref());
            if let Some(route) = rates.iter().find(|r| r.is_default) {
                if let Ok(mut history) = shared.history.lock() {
                    history.record("net.down", now, route.in_per_sec as f32);
                    history.record("net.up", now, route.out_per_sec as f32);
                }
                let before = counters.iter().find(|c| c.name == route.name);
                if let (Some(before), Ok(mut history)) = (before, shared.history.lock()) {
                    history.add_total(
                        &local_day(now, 0),
                        &route.name,
                        route.bytes_in_total.saturating_sub(before.bytes_in),
                        route.bytes_out_total.saturating_sub(before.bytes_out),
                    );
                }
            }
            let connected = rates.iter().any(|r| r.is_default);
            if settings.alerts.connection_down_enabled
                && alerts.check("connection", !connected, ALERT_HOLD_SECONDS, now)
            {
                notify("Network connection is down");
            }
            counters = current;
            counters_at = Instant::now();
            if updates.send(Update::Network(rates)).is_err() {
                return;
            }
        }

        if tick.is_multiple_of(SYSTEM_EVERY) {
            let want_processes = open == cpu_index || open == memory_index;
            let (cpu, processes) = sampler.poll(want_processes);
            if let Ok(mut history) = shared.history.lock() {
                history.record("cpu.total", now, cpu.total_percent);
            }
            let _ = updates.send(Update::Cpu(cpu));
            if let Some(processes) = processes {
                let _ = updates.send(Update::Processes(processes));
            }
            if let Some(memory) = memory::read() {
                if let Ok(mut history) = shared.history.lock() {
                    history.record("mem.used", now, memory_percent(&memory) as f32);
                    history.record(
                        "mem.compressed",
                        now,
                        (memory.compressed_bytes as f64 / memory.total_bytes.max(1) as f64 * 100.0) as f32,
                    );
                }
                if settings.alerts.memory_pressure_enabled
                    && alerts.check("pressure", memory.pressure == Pressure::Critical, ALERT_HOLD_SECONDS, now)
                {
                    notify("Memory pressure is critical");
                }
                let _ = updates.send(Update::Memory(memory));
            }
            let gpu = gpu::read().into_iter().next();
            if let Some(gpu) = &gpu
                && let Ok(mut history) = shared.history.lock()
            {
                if let Some(percent) = gpu.device_percent {
                    history.record("gpu.device", now, percent as f32);
                }
                if let (Some(used), Some(total)) = (gpu.memory_in_use_bytes, memory::read().map(|m| m.total_bytes)) {
                    history.record("gpu.memory", now, (used as f64 / total.max(1) as f64 * 100.0) as f32);
                }
            }
            let _ = updates.send(Update::Gpu(gpu));
        }

        let controlling = driver.is_controlling() || !matches!(settings.preset().map(|p| &p.mode), None | Some(FanMode::Auto));
        let fan_every = if controlling { FAN_ACTIVE_EVERY } else { FAN_IDLE_EVERY };
        if let Some(sensors) = &sensors
            && tick.is_multiple_of(fan_every)
            && let Ok(fans) = sensors.fans()
        {
            let hot = sensors.hottest();
            if let Ok(mut history) = shared.history.lock() {
                for fan in &fans {
                    history.record(&format!("fan.{}", fan.index), now, fan.actual_rpm);
                }
                if let Some((_, c)) = &hot {
                    let shown = match settings.temperature_unit {
                        gauge_core::settings::TemperatureUnit::Celsius => *c,
                        gauge_core::settings::TemperatureUnit::Fahrenheit => *c * 9.0 / 5.0 + 32.0,
                    };
                    history.record("temp.hot", now, shown);
                }
            }
            let mode = settings.preset().map_or(FanMode::Auto, |p| p.mode.clone());
            // Reading every sensor costs a little, so it only happens when a curve needs it.
            let curve_temperature_c = matches!(mode, FanMode::Curve { .. })
                .then(|| curve_temperature(&temperature_areas(&sensors.read_kind(Kind::Temperature))))
                .flatten();
            let state = driver.step(&mode, &fans, curve_temperature_c);
            let _ = updates.send(Update::FanState(state));
            if settings.alerts.hot_enabled
                && alerts.check("hot", hot.as_ref().is_some_and(|h| h.1 >= settings.alerts.hot_c), ALERT_HOLD_SECONDS, now)
            {
                notify("This Mac is running very hot");
            }
            let _ = updates.send(Update::Fans(fans, hot));
            if open == fans_index {
                let _ = updates.send(Update::Temperatures(temperature_areas(&sensors.read_kind(Kind::Temperature))));
            }
        }

        if let Some(sensors) = &sensors
            && shared.sensors_open.load(Ordering::Relaxed)
            && tick.is_multiple_of(SENSOR_LIST_EVERY)
        {
            let _ = updates.send(Update::Sensors(sensors.read()));
        }

        if tick.is_multiple_of(DRIVE_EVERY) {
            let current = drives::read_activity();
            let rates = disk_rates(&disks, current.clone(), disks_at.elapsed().as_secs_f64());
            disks = current;
            disks_at = Instant::now();
            if let Ok(mut history) = shared.history.lock() {
                history.record("disk.read", now, rates.iter().map(|d| d.read_per_sec).sum::<f64>() as f32);
                history.record("disk.write", now, rates.iter().map(|d| d.write_per_sec).sum::<f64>() as f32);
            }
            let volumes = drives::read_volumes();
            if let Some(root) = volumes.iter().find(|v| v.mount_point == "/") {
                let free = root.available_bytes as f64 / root.total_bytes.max(1) as f64 * 100.0;
                if settings.alerts.disk_low_enabled
                    && alerts.check("disk", free < f64::from(settings.alerts.disk_low_percent), 0, now)
                {
                    notify("Your startup disk is low on space");
                }
            }
            let _ = updates.send(Update::Drives(volumes, rates));
        }

        if settings.show_wifi && tick.is_multiple_of(WIFI_EVERY) {
            let wifi = wifi::read();
            if let Some(wifi) = &wifi
                && let Ok(mut history) = shared.history.lock()
            {
                history.record("wifi.rssi", now, wifi.rssi_dbm as f32);
            }
            let _ = updates.send(Update::Wifi(wifi));
        }

        if tick.is_multiple_of(POWER_EVERY) {
            let battery = battery::read();
            if let Some(battery) = &battery
                && let Ok(mut history) = shared.history.lock()
            {
                if let Some(percent) = battery.percent {
                    history.record("battery.percent", now, percent as f32);
                }
                if let Some(watts) = battery.load_watts {
                    history.record("power.load", now, watts as f32);
                }
                history.record("power.in", now, battery.input_watts.unwrap_or(0.0) as f32);
            }
            let _ = updates.send(Update::Battery(battery.map(Box::new)));
            if let Ok(history) = shared.history.lock() {
                let energy = history.energy_wh("power.load", local_midnight(now)).unwrap_or(0.0);
                let _ = updates.send(Update::EnergyToday(energy));
            }
        }
        if tick.is_multiple_of(DEVICE_BATTERY_EVERY) {
            let _ = updates.send(Update::DeviceBatteries(battery::read_devices()));
        }

        if open == drives_index && last_smart.is_none_or(|at| tick >= at + SMART_EVERY) {
            last_smart = Some(tick);
            let _ = updates.send(Update::Health(smart::read_all()));
        }

        if open == network_index && settings.show_top_apps && !apps_running.swap(true, Ordering::SeqCst) {
            let (sender, running) = (updates.clone(), apps_running.clone());
            std::thread::spawn(move || {
                if let Ok(apps) = network::app_traffic(APP_SAMPLE_SECONDS) {
                    let _ = sender.send(Update::Apps(apps));
                }
                running.store(false, Ordering::SeqCst);
            });
        }

        if tick.is_multiple_of(FLUSH_EVERY) && let Ok(mut history) = shared.history.lock() {
            let today = local_day(now, 0);
            if let Err(error) = history.flush(now) {
                eprintln!("gauge: could not save history: {error}");
            }
            let totals = (
                history.totals_since(&today).unwrap_or_default(),
                history.totals_since(&month_start_day(now)).unwrap_or_default(),
            );
            let _ = updates.send(Update::Totals(totals.0, totals.1));
        }
        tick += 1;
    }
}

pub fn absorb(latest: &mut crate::view::Latest, update: Update) {
    match update {
        Update::Network(interfaces) => latest.interfaces = interfaces,
        Update::Totals(today, month) => {
            latest.today = today;
            latest.month = month;
        }
        Update::Apps(apps) => latest.apps = apps,
        Update::Wifi(wifi) => latest.wifi = wifi,
        Update::Ping(ms) => latest.ping_ms = ms,
        Update::Cpu(cpu) => latest.cpu = Some(cpu),
        Update::Processes(processes) => latest.processes = processes,
        Update::Gpu(gpu) => latest.gpu = gpu,
        Update::Memory(memory) => latest.memory = Some(memory),
        Update::Drives(volumes, disks) => {
            latest.volumes = volumes;
            latest.disks = disks;
        }
        Update::Health(health) => latest.health = health,
        Update::Fans(fans, hottest) => {
            latest.fans = fans;
            latest.hottest = hottest;
        }
        Update::FanState(state) => latest.fan_state = state,
        Update::Battery(battery) => latest.battery = battery.map(|b| *b),
        Update::Sensors(sensors) => latest.sensors = sensors,
        Update::Temperatures(areas) => latest.temperatures = areas,
        Update::DeviceBatteries(devices) => latest.device_batteries = devices,
        Update::EnergyToday(watt_hours) => latest.energy_today_wh = watt_hours,
    }
}
