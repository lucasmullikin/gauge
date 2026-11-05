//! Turns readings into what the menu bar and panels show. Pure, so all of it is testable.

use crate::fan_control::FanState;
use crate::graph_math::Unit;
use gauge_core::battery::Battery;
use gauge_core::drives::{DiskActivity, Volume};
use gauge_core::gpu::Gpu;
use gauge_core::memory::{Memory, Pressure};
use gauge_core::network::{AppTraffic, InterfaceRate};
use gauge_core::processor::{Cpu, TopProcess, top_by};
use gauge_core::sensors::Fan;
use gauge_core::settings::{Settings, TemperatureUnit};
use gauge_core::smart::DriveHealth;
use gauge_core::wifi::{Wifi, quality_label, snr};

const HIDDEN_INTERFACE_PREFIXES: [&str; 8] = ["lo", "awdl", "llw", "anpi", "anri", "ap", "gif", "stf"];
const MAX_OTHER_CONNECTIONS: usize = 3;
const TOP_ROWS: usize = 5;
const AUTO_MODE: u8 = 0;

pub struct DiskRate {
    pub disk: DiskActivity,
    pub read_per_sec: f64,
    pub write_per_sec: f64,
}

#[derive(Default)]
pub struct Latest {
    pub interfaces: Vec<InterfaceRate>,
    pub apps: Vec<AppTraffic>,
    pub wifi: Option<Wifi>,
    pub ping_ms: Option<f32>,
    pub today: (u64, u64),
    pub month: (u64, u64),
    pub cpu: Option<Cpu>,
    pub processes: Vec<TopProcess>,
    pub gpu: Option<Gpu>,
    pub memory: Option<Memory>,
    pub volumes: Vec<Volume>,
    pub disks: Vec<DiskRate>,
    pub health: Vec<DriveHealth>,
    pub fans: Vec<Fan>,
    pub hottest: Option<(String, f32)>,
    pub battery: Option<Battery>,
    pub fan_state: FanState,
    pub install_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Primary,
    Secondary,
}

pub struct GraphSpec {
    pub series: Vec<(&'static str, Slot)>,
    pub unit: Unit,
    pub height: f64,
}

pub fn graphs(module: &str, settings: &Settings) -> Vec<GraphSpec> {
    let graph = |series, unit| GraphSpec { series, unit, height: 110.0 };
    match module {
        "network" => vec![graph(vec![("net.down", Slot::Primary), ("net.up", Slot::Secondary)], Unit::Rate)],
        "memory" => vec![graph(vec![("mem.used", Slot::Primary), ("mem.compressed", Slot::Secondary)], Unit::Percent)],
        "cpu" => vec![graph(vec![("cpu.total", Slot::Primary)], Unit::Percent)],
        "gpu" => vec![graph(vec![("gpu.device", Slot::Primary), ("gpu.memory", Slot::Secondary)], Unit::Percent)],
        "drives" => vec![graph(vec![("disk.read", Slot::Primary), ("disk.write", Slot::Secondary)], Unit::Rate)],
        "fans" => {
            let temperature = match settings.temperature_unit {
                TemperatureUnit::Celsius => Unit::Celsius,
                TemperatureUnit::Fahrenheit => Unit::Fahrenheit,
            };
            vec![
                graph(vec![("fan.0", Slot::Primary), ("fan.1", Slot::Secondary)], Unit::Rpm),
                GraphSpec { series: vec![("temp.hot", Slot::Secondary)], unit: temperature, height: 56.0 },
            ]
        }
        "battery" => vec![graph(vec![("battery.percent", Slot::Primary)], Unit::Percent)],
        _ => Vec::new(),
    }
}

pub fn format_bytes(bytes: f64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes.max(0.0);
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    match unit {
        0 => format!("{value:.0} {}", UNITS[unit]),
        _ if value < 10.0 => format!("{value:.1} {}", UNITS[unit]),
        _ => format!("{value:.0} {}", UNITS[unit]),
    }
}

/// RAM is sold and shown in binary units, so a 64 GB Mac should read as 64 GB, not 69.
pub fn format_memory(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        let gigabytes = value / GIB;
        if gigabytes < 10.0 {
            format!("{gigabytes:.1} GB")
        } else {
            format!("{gigabytes:.0} GB")
        }
    } else {
        format!("{:.0} MB", value / MIB)
    }
}

pub fn format_rate(bytes_per_sec: f64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec))
}

pub fn format_temperature(celsius: f32, unit: TemperatureUnit) -> String {
    match unit {
        TemperatureUnit::Celsius => format!("{celsius:.0}°C"),
        TemperatureUnit::Fahrenheit => format!("{:.0}°F", celsius * 9.0 / 5.0 + 32.0),
    }
}

pub fn format_value(value: f32, unit: Unit) -> String {
    match unit {
        Unit::Rate => format_rate(f64::from(value)),
        Unit::Percent => format!("{value:.0}%"),
        Unit::Rpm => format!("{value:.0} rpm"),
        Unit::Celsius => format!("{value:.0}°C"),
        Unit::Fahrenheit => format!("{value:.0}°F"),
    }
}

pub fn pressure_label(pressure: Pressure) -> &'static str {
    match pressure {
        Pressure::Normal => "Normal",
        Pressure::Warning => "Warning",
        Pressure::Critical => "Critical",
        Pressure::Unknown => "Unknown",
    }
}

pub fn memory_percent(memory: &Memory) -> f64 {
    if memory.total_bytes == 0 {
        return 0.0;
    }
    memory.used_bytes as f64 / memory.total_bytes as f64 * 100.0
}

pub struct Token {
    pub label: &'static str,
    pub name: &'static str,
}

const fn token(label: &'static str, name: &'static str) -> Token {
    Token { label, name }
}

const NETWORK_TOKENS: &[Token] = &[
    token("Upload speed", "up"),
    token("Download speed", "down"),
    token("Download today", "today_down"),
    token("Upload today", "today_up"),
    token("Gateway ping", "ping"),
    token("Wi-Fi signal", "signal"),
];
const MEMORY_TOKENS: &[Token] = &[
    token("Percent used", "percent"),
    token("Amount used", "used"),
    token("Free", "free"),
    token("Swap used", "swap"),
    token("Pressure", "pressure"),
];
const CPU_TOKENS: &[Token] = &[
    token("Percent used", "percent"),
    token("Load average", "load"),
];
const GPU_TOKENS: &[Token] = &[
    token("Percent used", "percent"),
    token("Memory", "memory"),
];
const DRIVE_TOKENS: &[Token] = &[
    token("Free space", "free"),
    token("Percent used", "percent"),
    token("Read speed", "read"),
    token("Write speed", "write"),
];
const FAN_TOKENS: &[Token] = &[
    token("Fastest fan, rpm", "rpm"),
    token("Hottest sensor", "temp"),
    token("Fan 1, rpm", "fan1"),
    token("Fan 2, rpm", "fan2"),
    token("Preset", "preset"),
];
const BATTERY_TOKENS: &[Token] = &[
    token("Charge", "percent"),
    token("Power", "power"),
    token("Health", "health"),
];

pub fn tokens(module: &str) -> &'static [Token] {
    match module {
        "network" => NETWORK_TOKENS,
        "memory" => MEMORY_TOKENS,
        "cpu" => CPU_TOKENS,
        "gpu" => GPU_TOKENS,
        "drives" => DRIVE_TOKENS,
        "fans" => FAN_TOKENS,
        "battery" => BATTERY_TOKENS,
        _ => &[],
    }
}

pub const ICONS: [&str; 20] = [
    "↑", "↓", "⇡", "⇣", "▲", "▼", "●", "◆", "■", "▪︎", "⚡︎", "♨︎", "☀︎", "⌘", "∿", "≋", "🌡", "💾", "🌐", "🌀",
];

pub fn default_template(module: &str) -> &'static str {
    match module {
        "network" => "↑{up} ↓{down}",
        "memory" => "RAM {percent}",
        "cpu" => "CPU {percent}",
        "gpu" => "GPU {percent}",
        "drives" => "{free} free",
        "fans" => "{rpm} rpm {temp}",
        "battery" => "{percent}",
        _ => "",
    }
}

pub fn template_for<'a>(module: &str, settings: &'a Settings) -> &'a str {
    settings
        .module(module)
        .and_then(|m| m.bar_template.as_deref())
        .unwrap_or_else(|| default_template(module))
}

/// Swaps each `{name}` for its value. Anything unrecognized stays as typed, so a typo is visible.
pub fn render_template(template: &str, value: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                match value(&after[..close]) {
                    Some(text) => out.push_str(&text),
                    None => out.push_str(&rest[open..open + close + 2]),
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

const MISSING: &str = "–";

fn token_value(module: &str, name: &str, latest: &Latest, settings: &Settings) -> Option<String> {
    let or_missing = |value: Option<String>| Some(value.unwrap_or_else(|| MISSING.to_string()));
    let default = latest.interfaces.iter().find(|i| i.is_default);
    let root = latest.volumes.iter().find(|v| v.mount_point == "/");
    let total_rate = |pick: fn(&DiskRate) -> f64| format_rate(latest.disks.iter().map(pick).sum());
    let fan_rpm = |index: usize| latest.fans.get(index).map(|f| format!("{:.0}", (f.actual_rpm / 10.0).round() * 10.0));
    match (module, name) {
        ("network", "up") => or_missing(default.map(|i| format_rate(i.out_per_sec))),
        ("network", "down") => or_missing(default.map(|i| format_rate(i.in_per_sec))),
        ("network", "today_down") => Some(format_bytes(latest.today.0 as f64)),
        ("network", "today_up") => Some(format_bytes(latest.today.1 as f64)),
        ("network", "ping") => or_missing(latest.ping_ms.map(|ms| format!("{ms:.0} ms"))),
        ("network", "signal") => or_missing(latest.wifi.as_ref().map(|w| format!("{} dBm", w.rssi_dbm))),
        ("memory", "percent") => or_missing(latest.memory.as_ref().map(|m| format!("{:.0}%", memory_percent(m)))),
        ("memory", "used") => or_missing(latest.memory.as_ref().map(|m| format_memory(m.used_bytes))),
        ("memory", "free") => or_missing(latest.memory.as_ref().map(|m| format_memory(m.free_bytes))),
        ("memory", "swap") => or_missing(latest.memory.as_ref().map(|m| format_memory(m.swap_used_bytes))),
        ("memory", "pressure") => or_missing(latest.memory.as_ref().map(|m| pressure_label(m.pressure).to_string())),
        ("cpu", "percent") => or_missing(latest.cpu.as_ref().map(|c| format!("{:.0}%", c.total_percent))),
        ("cpu", "load") => or_missing(latest.cpu.as_ref().map(|c| format!("{:.2}", c.load_average[0]))),
        ("gpu", "percent") => or_missing(latest.gpu.as_ref().and_then(|g| g.device_percent).map(|p| format!("{p}%"))),
        ("gpu", "memory") => or_missing(latest.gpu.as_ref().and_then(|g| g.memory_in_use_bytes).map(|b| format_memory(b as u64))),
        ("drives", "free") => or_missing(root.map(|v| format_bytes(v.available_bytes as f64))),
        ("drives", "percent") => or_missing(root.map(|v| {
            let used = v.total_bytes.saturating_sub(v.available_bytes) as f64;
            format!("{:.0}%", used / v.total_bytes.max(1) as f64 * 100.0)
        })),
        ("drives", "read") => Some(total_rate(|d| d.read_per_sec)),
        ("drives", "write") => Some(total_rate(|d| d.write_per_sec)),
        ("fans", "rpm") => {
            // Rounded to tens: fans jitter by a few rpm, and every visible change costs a redraw.
            let fastest = latest.fans.iter().map(|f| f.actual_rpm).fold(0.0, f32::max);
            Some(format!("{:.0}", (fastest / 10.0).round() * 10.0))
        }
        ("fans", "temp") => or_missing(latest.hottest.as_ref().map(|(_, c)| format_temperature(*c, settings.temperature_unit))),
        ("fans", "fan1") => or_missing(fan_rpm(0)),
        ("fans", "fan2") => or_missing(fan_rpm(1)),
        ("fans", "preset") => Some(settings.active_preset.clone()),
        ("battery", "percent") => or_missing(latest.battery.as_ref().and_then(|b| b.percent).map(|p| format!("{p}%"))),
        ("battery", "power") => or_missing(latest.battery.as_ref().and_then(|b| b.power_watts).map(|w| format!("{:.1} W", w.abs()))),
        ("battery", "health") => or_missing(latest.battery.as_ref().and_then(|b| b.health_percent).map(|h| format!("{h:.0}%"))),
        _ => None,
    }
}

pub fn bar_title(module: &str, latest: &Latest, settings: &Settings) -> String {
    render_template(template_for(module, settings), |name| token_value(module, name, latest, settings))
}

/// The menu bar as it stands, leftmost first, with hidden items included so they can be moved.
pub fn preview_items(latest: &Latest, settings: &Settings) -> Vec<crate::order_view::BarItem> {
    settings
        .order
        .iter()
        .map(|name| crate::order_view::BarItem {
            name: name.clone(),
            title: bar_title(name, latest, settings),
            shown: settings.module(name).is_some_and(|m| m.shown),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub value: String,
    pub header: bool,
}

fn row(key: impl Into<String>, value: impl Into<String>) -> Row {
    Row { key: key.into(), value: value.into(), header: false }
}

fn header(key: impl Into<String>) -> Row {
    Row { key: key.into(), value: String::new(), header: true }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Content {
    pub title: String,
    pub subtitle: String,
    /// Each legend entry's text, matched in order to the first graph's series.
    pub legend: Vec<String>,
    pub rows: Vec<Row>,
}

fn is_shown(interface: &InterfaceRate) -> bool {
    !interface.is_default
        && interface.up
        && !interface.addresses.is_empty()
        && interface.in_per_sec + interface.out_per_sec > 0.0
        && !HIDDEN_INTERFACE_PREFIXES.iter().any(|p| interface.name.starts_with(p))
}

fn first_routable(interface: &InterfaceRate) -> Option<&String> {
    let ipv4 = interface.addresses.iter().find(|a| a.contains('.'));
    ipv4.or_else(|| interface.addresses.iter().find(|a| !a.starts_with("fe80:")))
}

fn network_content(latest: &Latest, settings: &Settings) -> Content {
    let default = latest.interfaces.iter().find(|i| i.is_default);
    let mut content = Content {
        title: "Network".into(),
        subtitle: default.map_or_else(|| "Not connected".to_string(), |i| i.name.clone()),
        legend: match default {
            Some(i) => vec![format!("Down {}", format_rate(i.in_per_sec)), format!("Up {}", format_rate(i.out_per_sec))],
            None => vec!["Down".into(), "Up".into()],
        },
        rows: Vec::new(),
    };
    let rows = &mut content.rows;
    let totals = |(down, up): (u64, u64)| format!("↓{}  ↑{}", format_bytes(down as f64), format_bytes(up as f64));
    rows.push(row("Today", totals(latest.today)));
    rows.push(row("This month", totals(latest.month)));
    if let Some(i) = default {
        rows.push(row("Since boot", totals((i.bytes_in_total, i.bytes_out_total))));
        if let Some(address) = first_routable(i) {
            rows.push(row("Address", address.clone()));
        }
    }
    rows.push(row(
        "Gateway ping",
        latest.ping_ms.map_or_else(|| "No reply".to_string(), |ms| format!("{ms:.0} ms")),
    ));
    if settings.show_wifi && let Some(wifi) = &latest.wifi {
        let quality = quality_label(snr(wifi.rssi_dbm, wifi.noise_dbm));
        rows.push(row("Wi-Fi signal", format!("{} dBm, {quality}", wifi.rssi_dbm)));
        rows.push(row("Wi-Fi speed", format!("{:.0} Mb/s", wifi.transmit_mbps)));
        if let Some(channel) = wifi.channel {
            rows.push(row("Wi-Fi channel", channel.to_string()));
        }
    }
    let mut others: Vec<&InterfaceRate> = latest.interfaces.iter().filter(|i| is_shown(i)).collect();
    others.sort_by(|a, b| (b.in_per_sec + b.out_per_sec).total_cmp(&(a.in_per_sec + a.out_per_sec)));
    others.truncate(MAX_OTHER_CONNECTIONS);
    if !others.is_empty() {
        rows.push(header("Other connections"));
        for i in others {
            rows.push(row(i.name.clone(), format!("↓{}  ↑{}", format_rate(i.in_per_sec), format_rate(i.out_per_sec))));
        }
    }
    if settings.show_top_apps && !latest.apps.is_empty() {
        rows.push(header("Top apps"));
        for app in latest.apps.iter().take(TOP_ROWS) {
            rows.push(row(
                app.process.clone(),
                format!("↓{}  ↑{}", format_bytes(app.bytes_in as f64), format_bytes(app.bytes_out as f64)),
            ));
        }
    }
    content
}

fn memory_content(latest: &Latest) -> Content {
    let Some(m) = &latest.memory else {
        return Content { title: "Memory".into(), ..Content::default() };
    };
    let gigabytes = format_memory;
    let mut rows = vec![
        row("Pressure", pressure_label(m.pressure)),
        row("App memory", gigabytes(m.app_bytes)),
        row("Wired", gigabytes(m.wired_bytes)),
        row("Compressed", gigabytes(m.compressed_bytes)),
        row("Cached files", gigabytes(m.cached_files_bytes)),
        row("Free", gigabytes(m.free_bytes)),
        row("Swap used", format!("{} of {}", gigabytes(m.swap_used_bytes), gigabytes(m.swap_total_bytes))),
    ];
    if !latest.processes.is_empty() {
        rows.push(header("Top apps"));
        for p in top_by(latest.processes.clone(), TOP_ROWS, |p| p.memory_bytes) {
            rows.push(row(p.name, format_memory(p.memory_bytes)));
        }
    }
    Content {
        title: "Memory".into(),
        subtitle: format!("{} of {}", gigabytes(m.used_bytes), gigabytes(m.total_bytes)),
        legend: vec![
            format!("Used {:.0}%", memory_percent(m)),
            format!("Compressed {:.0}%", m.compressed_bytes as f64 / m.total_bytes.max(1) as f64 * 100.0),
        ],
        rows,
    }
}

fn cpu_content(latest: &Latest) -> Content {
    let Some(c) = &latest.cpu else {
        return Content { title: "CPU".into(), ..Content::default() };
    };
    let mut rows = vec![
        row("Load average", format!("{:.2}  {:.2}  {:.2}", c.load_average[0], c.load_average[1], c.load_average[2])),
        row("Uptime", format!("{}d {}h", c.uptime_seconds / 86400, c.uptime_seconds % 86400 / 3600)),
    ];
    if let (Some(p), Some(e)) = (c.performance_cores, c.efficiency_cores) {
        rows.push(row("Cores", format!("{p} performance, {e} efficiency")));
    }
    let cores: Vec<String> = c.per_core_percent.iter().map(|p| format!("{p:.0}")).collect();
    rows.push(row("Per core, %", cores.join(" ")));
    if !latest.processes.is_empty() {
        rows.push(header("Top apps"));
        for p in top_by(latest.processes.clone(), TOP_ROWS, |p| (p.cpu_percent * 100.0) as u64) {
            rows.push(row(p.name, format!("{:.0}%", p.cpu_percent)));
        }
    }
    Content {
        title: "CPU".into(),
        subtitle: String::new(),
        legend: vec![format!("Total {:.0}%", c.total_percent)],
        rows,
    }
}

fn gpu_content(latest: &Latest) -> Content {
    let Some(g) = &latest.gpu else {
        return Content { title: "GPU".into(), ..Content::default() };
    };
    let percent = |p: Option<i64>| p.map_or_else(|| "Unknown".to_string(), |v| format!("{v}%"));
    let memory = g.memory_in_use_bytes.map_or_else(|| "Unknown".to_string(), |b| format_bytes(b as f64));
    Content {
        title: "GPU".into(),
        subtitle: g.model.clone().unwrap_or_default(),
        legend: vec![format!("Use {}", percent(g.device_percent)), format!("Memory {memory}")],
        rows: vec![
            row("Renderer", percent(g.renderer_percent)),
            row("Tiler", percent(g.tiler_percent)),
            row("Cores", g.core_count.map_or_else(|| "Unknown".to_string(), |c| c.to_string())),
        ],
    }
}

fn drives_content(latest: &Latest, settings: &Settings) -> Content {
    let mut rows = vec![header("Volumes")];
    for v in &latest.volumes {
        let name = v.mount_point.rsplit('/').find(|p| !p.is_empty()).unwrap_or("Macintosh HD");
        let kind = if v.is_network { " (network)" } else { "" };
        rows.push(row(
            format!("{name}{kind}"),
            format!("{} free of {}", format_bytes(v.available_bytes as f64), format_bytes(v.total_bytes as f64)),
        ));
    }
    for d in &latest.disks {
        rows.push(header(d.disk.model.clone().unwrap_or_else(|| d.disk.bsd_name.clone())));
        match latest.health.iter().find(|h| h.bsd_name.as_deref() == Some(&d.disk.bsd_name)) {
            Some(h) => {
                let h = &h.health;
                let healthy = h.critical_warning == 0 && h.media_errors == 0;
                rows.push(row("Health", if healthy { "Healthy" } else { "WARNING" }));
                rows.push(row("Temperature", format_temperature(h.temperature_c as f32, settings.temperature_unit)));
                rows.push(row("Wear", format!("{}% used, {}% spare", h.percent_used, h.available_spare_percent)));
                rows.push(row("Written", format_bytes(h.bytes_written as f64)));
                rows.push(row("Powered on", format!("{} hours", h.power_on_hours)));
                rows.push(row("Media errors", h.media_errors.to_string()));
            }
            None => rows.push(row("Health", "Unavailable")),
        }
    }
    let (read, write) = latest
        .disks
        .iter()
        .fold((0.0, 0.0), |(r, w), d| (r + d.read_per_sec, w + d.write_per_sec));
    Content {
        title: "Drives".into(),
        subtitle: String::new(),
        legend: vec![format!("Read {}", format_rate(read)), format!("Write {}", format_rate(write))],
        rows,
    }
}

fn fans_content(latest: &Latest, settings: &Settings) -> Content {
    let mut rows: Vec<Row> = latest
        .fans
        .iter()
        .map(|f| {
            let mode = if f.mode == AUTO_MODE { "Auto" } else { "Manual" };
            row(
                format!("Fan {}", f.index + 1),
                format!("{:.0} rpm, {mode}", f.actual_rpm),
            )
        })
        .collect();
    rows.push(row("Status", latest.fan_state.label()));
    if let Some(message) = &latest.install_message {
        rows.push(row("Installer", message.clone()));
    }
    if let Some((key, c)) = &latest.hottest {
        rows.push(row("Hottest sensor", format!("{key} {}", format_temperature(*c, settings.temperature_unit))));
    }
    Content {
        title: "Fans".into(),
        subtitle: settings.active_preset.clone(),
        legend: latest.fans.iter().map(|f| format!("Fan {} {:.0}", f.index + 1, f.actual_rpm)).collect(),
        rows,
    }
}

fn battery_content(latest: &Latest, settings: &Settings) -> Content {
    let Some(b) = &latest.battery else {
        return Content { title: "Battery".into(), subtitle: "None".into(), ..Content::default() };
    };
    let state = match (b.charging, b.external_power, b.fully_charged) {
        (_, _, Some(true)) => "Charged",
        (Some(true), _, _) => "Charging",
        (_, Some(true), _) => "On power",
        _ => "On battery",
    };
    let mut rows = vec![row("State", state)];
    if let Some(h) = b.health_percent {
        rows.push(row("Health", format!("{h:.0}%")));
    }
    if let Some(c) = b.cycle_count {
        rows.push(row("Cycles", c.to_string()));
    }
    if let Some(w) = b.power_watts {
        rows.push(row("Power", format!("{:.1} W", w.abs())));
    }
    if let Some(t) = b.temperature_c {
        rows.push(row("Temperature", format_temperature(t, settings.temperature_unit)));
    }
    if let Some(m) = b.minutes_remaining {
        rows.push(row("Time left", format!("{}h {:02}m", m / 60, m % 60)));
    }
    Content {
        title: "Battery".into(),
        subtitle: String::new(),
        legend: vec![b.percent.map_or_else(|| "Charge".to_string(), |p| format!("Charge {p}%"))],
        rows,
    }
}

pub fn content(module: &str, latest: &Latest, settings: &Settings) -> Content {
    match module {
        "network" => network_content(latest, settings),
        "memory" => memory_content(latest),
        "cpu" => cpu_content(latest),
        "gpu" => gpu_content(latest),
        "drives" => drives_content(latest, settings),
        "fans" => fans_content(latest, settings),
        "battery" => battery_content(latest, settings),
        _ => Content::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interface(name: &str, is_default: bool, addresses: &[&str]) -> InterfaceRate {
        InterfaceRate {
            name: name.into(),
            up: true,
            is_default,
            addresses: addresses.iter().map(|a| a.to_string()).collect(),
            bytes_in_total: 2_000_000,
            bytes_out_total: 1_000_000,
            in_per_sec: 14_000_000.0,
            out_per_sec: 2_100.0,
        }
    }

    fn value<'a>(content: &'a Content, key: &str) -> &'a str {
        &content.rows.iter().find(|r| r.key == key).unwrap_or_else(|| panic!("no {key}")).value
    }

    #[test]
    fn formats_bytes_at_a_readable_scale() {
        assert_eq!(format_bytes(0.0), "0 B");
        assert_eq!(format_bytes(2_100.0), "2.1 KB");
        assert_eq!(format_bytes(14_000_000.0), "14 MB");
        assert_eq!(format_bytes(-5.0), "0 B");
    }

    #[test]
    fn temperatures_follow_the_setting() {
        assert_eq!(format_temperature(100.0, TemperatureUnit::Celsius), "100°C");
        assert_eq!(format_temperature(100.0, TemperatureUnit::Fahrenheit), "212°F");
    }

    #[test]
    fn network_title_uses_the_default_route_so_vpns_do_not_double_count() {
        let latest = Latest {
            interfaces: vec![interface("utun3", false, &["10.0.0.1"]), interface("en0", true, &[])],
            ..Latest::default()
        };
        assert_eq!(bar_title("network", &latest, &Settings::default()), "↑2.1 KB/s ↓14 MB/s");
        assert_eq!(bar_title("network", &Latest::default(), &Settings::default()), "↑– ↓–");
    }

    #[test]
    fn network_panel_shows_totals_ping_and_hides_system_interfaces() {
        let latest = Latest {
            interfaces: vec![
                interface("lo0", false, &["127.0.0.1"]),
                interface("awdl0", false, &["fe80::1"]),
                interface("utun2", false, &["100.1.2.3"]),
                InterfaceRate { in_per_sec: 0.0, out_per_sec: 0.0, ..interface("utun9", false, &["10.9.9.9"]) },
                interface("en0", true, &["fe80::2", "192.168.1.5"]),
            ],
            today: (18_000_000_000, 3_200_000_000),
            ping_ms: Some(3.4),
            ..Latest::default()
        };
        let got = content("network", &latest, &Settings::default());
        assert_eq!(got.subtitle, "en0");
        assert_eq!(value(&got, "Today"), "↓18 GB  ↑3.2 GB");
        assert_eq!(value(&got, "Address"), "192.168.1.5");
        assert_eq!(value(&got, "Gateway ping"), "3 ms");
        assert!(got.rows.iter().any(|r| r.key == "utun2"));
        assert!(!got.rows.iter().any(|r| r.key == "utun9"), "idle tunnels stay hidden");
        assert!(!got.rows.iter().any(|r| r.key == "lo0" || r.key == "awdl0"));
    }

    #[test]
    fn a_failed_ping_says_so() {
        let got = content("network", &Latest::default(), &Settings::default());
        assert_eq!(value(&got, "Gateway ping"), "No reply");
    }

    #[test]
    fn wifi_rows_follow_the_setting() {
        let latest = Latest {
            wifi: Some(Wifi {
                interface: None,
                ssid: None,
                rssi_dbm: -43,
                noise_dbm: -90,
                transmit_mbps: 1200.0,
                channel: Some(161),
            }),
            ..Latest::default()
        };
        let got = content("network", &latest, &Settings::default());
        assert_eq!(value(&got, "Wi-Fi signal"), "-43 dBm, Excellent");
        let off = Settings { show_wifi: false, ..Settings::default() };
        assert!(!content("network", &latest, &off).rows.iter().any(|r| r.key == "Wi-Fi signal"));
    }

    fn memory() -> Memory {
        Memory {
            total_bytes: 64 * 1024 * 1024 * 1024,
            used_bytes: 32 * 1024 * 1024 * 1024,
            app_bytes: 20 * 1024 * 1024 * 1024,
            wired_bytes: 10 * 1024 * 1024 * 1024,
            compressed_bytes: 2 * 1024 * 1024 * 1024,
            cached_files_bytes: 5 * 1024 * 1024 * 1024,
            free_bytes: 1024 * 1024 * 1024,
            swap_total_bytes: 1024 * 1024 * 1024,
            swap_used_bytes: 200 * 1024 * 1024,
            pressure: Pressure::Warning,
        }
    }

    #[test]
    fn memory_shows_percent_breakdown_and_pressure() {
        let latest = Latest { memory: Some(memory()), ..Latest::default() };
        assert_eq!(bar_title("memory", &latest, &Settings::default()), "RAM 50%");
        let got = content("memory", &latest, &Settings::default());
        assert_eq!(got.subtitle, "32 GB of 64 GB");
        assert_eq!(value(&got, "Pressure"), "Warning");
        assert_eq!(value(&got, "Swap used"), "200 MB of 1.0 GB");
        assert_eq!(format_memory(68_719_476_736), "64 GB");
        assert_eq!(got.legend[0], "Used 50%");
    }

    fn disk(name: &str) -> DiskRate {
        DiskRate {
            disk: DiskActivity {
                bsd_name: name.into(),
                model: Some("SSD".into()),
                location: None,
                bytes_read: 0,
                bytes_written: 0,
            },
            read_per_sec: 0.0,
            write_per_sec: 1_000_000.0,
        }
    }

    fn health(errors: u128) -> DriveHealth {
        DriveHealth {
            bsd_name: Some("disk0".into()),
            model: None,
            health: gauge_core::smart::Health {
                critical_warning: 0,
                temperature_c: 36,
                available_spare_percent: 100,
                percent_used: 3,
                bytes_read: 0,
                bytes_written: 182_000_000_000_000,
                power_on_hours: 1,
                power_cycles: 1,
                unsafe_shutdowns: 0,
                media_errors: errors,
            },
        }
    }

    #[test]
    fn a_disk_without_smart_says_so_instead_of_looking_fine() {
        let latest = Latest { disks: vec![disk("disk3")], ..Latest::default() };
        assert_eq!(value(&content("drives", &latest, &Settings::default()), "Health"), "Unavailable");
    }

    #[test]
    fn media_errors_show_a_warning() {
        let latest = |h| Latest { disks: vec![disk("disk0")], health: vec![h], ..Latest::default() };
        let settings = Settings::default();
        assert_eq!(value(&content("drives", &latest(health(0)), &settings), "Health"), "Healthy");
        assert_eq!(value(&content("drives", &latest(health(2)), &settings), "Health"), "WARNING");
    }

    fn fan() -> Fan {
        Fan { index: 0, mode: 1, target_rpm: 2500.0, actual_rpm: 2480.0, min_rpm: 1499.0, max_rpm: 5348.0 }
    }

    #[test]
    fn fan_panel_reports_real_mode_and_helper_state() {
        let latest = Latest {
            fans: vec![fan()],
            hottest: Some(("Tp0c".to_string(), 62.4)),
            fan_state: FanState::HelperMissing,
            ..Latest::default()
        };
        let settings = Settings::default();
        assert_eq!(bar_title("fans", &latest, &settings), "2480 rpm 62°C");
        let jitter = Latest { fans: vec![Fan { actual_rpm: 2483.0, ..fan() }], ..Latest::default() };
        assert_eq!(bar_title("fans", &jitter, &settings), "2480 rpm –");
        let got = content("fans", &latest, &settings);
        assert_eq!(value(&got, "Fan 1"), "2480 rpm, Manual");
        assert_eq!(value(&got, "Status"), "Fan helper not installed");
        assert_eq!(value(&got, "Hottest sensor"), "Tp0c 62°C");
    }

    #[test]
    fn templates_fill_in_values_and_keep_everything_else() {
        let value = |name: &str| (name == "up").then(|| "5 KB/s".to_string());
        assert_eq!(render_template("▲{up} net", value), "▲5 KB/s net");
        assert_eq!(render_template("{typo}", value), "{typo}");
        assert_eq!(render_template("open { brace", value), "open { brace");
        assert_eq!(render_template("", value), "");
    }

    #[test]
    fn a_custom_template_replaces_the_default() {
        let mut settings = Settings::default();
        settings.memory.bar_template = Some("🌀 {used} ({pressure})".into());
        let latest = Latest { memory: Some(memory()), ..Latest::default() };
        assert_eq!(bar_title("memory", &latest, &settings), "🌀 32 GB (Warning)");
        settings.memory.bar_template = Some(String::new());
        assert_eq!(bar_title("memory", &latest, &settings), "");
    }

    #[test]
    fn every_token_produces_a_value() {
        let settings = Settings::default();
        for module in gauge_core::settings::MODULES {
            for token in tokens(module) {
                let got = token_value(module, token.name, &Latest::default(), &settings);
                assert!(got.is_some(), "{module} {}", token.name);
            }
        }
    }

    #[test]
    fn the_preview_follows_order_and_marks_hidden_items() {
        let settings = Settings { order: vec!["fans".to_string(), "gpu".to_string()], ..Settings::default() };
        let items = preview_items(&Latest::default(), &settings);
        assert_eq!(items[0].name, "fans");
        assert!(items[0].shown);
        assert!(!items[1].shown, "GPU is hidden by default");
    }

    #[test]
    fn every_module_has_a_graph() {
        for module in gauge_core::settings::MODULES {
            assert!(!graphs(module, &Settings::default()).is_empty(), "{module}");
        }
    }

    #[test]
    fn empty_readings_never_panic() {
        for module in gauge_core::settings::MODULES {
            let _ = content(module, &Latest::default(), &Settings::default());
            let _ = bar_title(module, &Latest::default(), &Settings::default());
        }
    }
}
