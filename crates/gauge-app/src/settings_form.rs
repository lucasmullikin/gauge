//! What each settings page contains, and how a control's value changes the settings. Pure, so
//! it's testable; the window only turns these rows into controls.

use crate::view;
use gauge_core::settings::{
    BarStyle, FanMode, GraphScale, GraphStyle, MAX_ITEM_SPACING, MODULES, RANGE_LABELS, RANGES_SECONDS, Settings,
    TemperatureUnit,
};

pub const PAGES: [&str; 9] = ["General", "Network", "Memory", "CPU", "GPU", "Drives", "Fans", "Battery", "Alerts"];
const GENERAL: i64 = 0;
const ALERTS: i64 = 8;
const FIELD_SHOWN: i64 = 0;
const FIELD_BAR: i64 = 1;
const FIELD_STYLE: i64 = 2;
const FIELD_SCALE: i64 = 3;
const FIELD_PEAKS: i64 = 4;
const FIELD_RANGE: i64 = 5;
const FIELD_COLOR: i64 = 6;
const FIELD_COLOR2: i64 = 7;
const FIELD_TEMPLATE: i64 = 8;
const FIELD_ADD_VALUE: i64 = 9;
const FIELD_ADD_ICON: i64 = 10;
const FIELD_RESET_TEXT: i64 = 11;
const FIELD_TOP_APPS: i64 = 20;
const FIELD_WIFI: i64 = 21;
const FIELD_PING: i64 = 22;
const FIELD_FIXED: i64 = 30;
const FIELD_UNIT: i64 = 1;
const FIELD_LOGIN: i64 = 2;
const FIELD_SPACING: i64 = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    /// A section title rather than a control.
    Heading,
    Checkbox(bool),
    /// Few choices, all visible at once.
    Segmented(Vec<&'static str>, usize),
    /// Two color wells on one row; the second uses the row's tag plus one.
    Colors(String, String),
    Text(String, &'static str),
    Slider { value: f64, min: f64, max: f64, unit: &'static str },
    /// A menu that runs a command rather than holding a value. The first entry is its title.
    Menu(Vec<String>),
    /// One tap adds that icon. Each button's tag comes from `icon_tag`.
    Icons,
    /// A button that runs a command.
    Button(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormRow {
    pub label: &'static str,
    pub control: Control,
    pub tag: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Bool(bool),
    Index(usize),
    Color(String),
    Text(String),
    Number(f64),
}

fn heading(label: &'static str) -> FormRow {
    FormRow { label, control: Control::Heading, tag: -1 }
}

const ICON_TAG_BASE: i64 = 100_000;

pub fn icon_tag(page: i64, index: usize) -> i64 {
    ICON_TAG_BASE + page * 100 + index as i64
}

fn module_rows(page: i64, settings: &Settings) -> Vec<FormRow> {
    let name = MODULES[page as usize - 1];
    let Some(module) = settings.module(name) else {
        return Vec::new();
    };
    let tag = |field| page * 100 + field;
    let bar = match module.bar_style {
        BarStyle::Text => 0,
        BarStyle::Graph => 1,
        BarStyle::Both => 2,
    };
    let style = match module.graph_style {
        GraphStyle::Line => 0,
        GraphStyle::Area => 1,
        GraphStyle::Bars => 2,
    };
    let scale = usize::from(module.graph_scale == GraphScale::Log);
    vec![
        heading("Menu bar"),
        FormRow { label: "Show this item", control: Control::Checkbox(module.shown), tag: tag(FIELD_SHOWN) },
        FormRow { label: "Display", control: Control::Segmented(vec!["Text", "Graph", "Both"], bar), tag: tag(FIELD_BAR) },
        FormRow {
            label: "Text",
            control: Control::Text(view::template_for(name, settings).to_string(), "Empty shows no text"),
            tag: tag(FIELD_TEMPLATE),
        },
        FormRow {
            label: "Add value",
            control: Control::Menu(
                std::iter::once("Choose a value".to_string())
                    .chain(view::tokens(name).iter().map(|t| t.label.to_string()))
                    .collect(),
            ),
            tag: tag(FIELD_ADD_VALUE),
        },
        FormRow { label: "Add icon", control: Control::Icons, tag: tag(FIELD_ADD_ICON) },
        FormRow { label: "", control: Control::Button("Reset text"), tag: tag(FIELD_RESET_TEXT) },
        heading("Graph"),
        FormRow { label: "Style", control: Control::Segmented(vec!["Line", "Area", "Bars"], style), tag: tag(FIELD_STYLE) },
        FormRow { label: "Scale", control: Control::Segmented(vec!["Linear", "Logarithmic"], scale), tag: tag(FIELD_SCALE) },
        FormRow { label: "Time range", control: Control::Segmented(RANGE_LABELS.to_vec(), module.range_index), tag: tag(FIELD_RANGE) },
        FormRow { label: "Show peaks", control: Control::Checkbox(module.show_peaks), tag: tag(FIELD_PEAKS) },
        FormRow { label: "Colors", control: Control::Colors(module.color.clone(), module.secondary_color.clone()), tag: tag(FIELD_COLOR) },
    ]
}

fn fixed_percent(settings: &Settings) -> f32 {
    settings
        .presets
        .iter()
        .find_map(|p| match (&p.name[..], &p.mode) {
            ("Fixed", FanMode::Fixed { percent }) => Some(*percent),
            _ => None,
        })
        .unwrap_or(50.0)
}

pub fn set_fixed_percent(settings: &mut Settings, percent: f32) {
    for preset in &mut settings.presets {
        if preset.name == "Fixed" {
            preset.mode = FanMode::Fixed { percent: percent.clamp(0.0, 100.0) };
        }
    }
}

pub fn rows(page: i64, settings: &Settings) -> Vec<FormRow> {
    match page {
        GENERAL => vec![
            FormRow {
                label: "Temperature",
                control: Control::Segmented(vec!["Celsius", "Fahrenheit"], usize::from(settings.temperature_unit == TemperatureUnit::Fahrenheit)),
                tag: FIELD_UNIT,
            },
            FormRow { label: "Open at login", control: Control::Checkbox(settings.launch_at_login), tag: FIELD_LOGIN },
            FormRow {
                label: "Item spacing",
                control: Control::Slider { value: settings.item_spacing, min: 0.0, max: MAX_ITEM_SPACING, unit: " pt" },
                tag: FIELD_SPACING,
            },
        ],
        ALERTS => {
            let a = &settings.alerts;
            vec![
                heading("Storage"),
                FormRow { label: "Startup disk low", control: Control::Checkbox(a.disk_low_enabled), tag: ALERTS * 100 },
                FormRow { label: "Alert below", control: Control::Slider { value: f64::from(a.disk_low_percent), min: 1.0, max: 50.0, unit: "% free" }, tag: ALERTS * 100 + 1 },
                heading("Temperature"),
                FormRow { label: "Running hot", control: Control::Checkbox(a.hot_enabled), tag: ALERTS * 100 + 2 },
                FormRow { label: "Alert above", control: Control::Slider { value: f64::from(a.hot_c), min: 60.0, max: 110.0, unit: "°C" }, tag: ALERTS * 100 + 3 },
                heading("System"),
                FormRow { label: "Memory pressure critical", control: Control::Checkbox(a.memory_pressure_enabled), tag: ALERTS * 100 + 4 },
                FormRow { label: "Connection down", control: Control::Checkbox(a.connection_down_enabled), tag: ALERTS * 100 + 5 },
            ]
        }
        _ if (1..=7).contains(&page) => {
            let mut form = module_rows(page, settings);
            match MODULES[page as usize - 1] {
                "network" => {
                    form.push(heading("Details"));
                    form.push(FormRow { label: "Show top apps", control: Control::Checkbox(settings.show_top_apps), tag: page * 100 + FIELD_TOP_APPS });
                    form.push(FormRow { label: "Show Wi-Fi details", control: Control::Checkbox(settings.show_wifi), tag: page * 100 + FIELD_WIFI });
                    form.push(FormRow {
                        label: "Ping address",
                        control: Control::Text(settings.ping_host.clone(), "Your gateway, which stays on your network"),
                        tag: page * 100 + FIELD_PING,
                    });
                }
                "fans" => {
                    form.push(heading("Fan speeds"));
                    form.push(FormRow {
                        label: "Fixed preset",
                        control: Control::Slider { value: f64::from(fixed_percent(settings)), min: 0.0, max: 100.0, unit: "%" },
                        tag: page * 100 + FIELD_FIXED,
                    });
                }
                _ => {}
            }
            form
        }
        _ => Vec::new(),
    }
}

/// Checkboxes report their state; plain buttons just say they were pressed.
pub fn input_for_button(tag: i64, on: bool) -> Input {
    if tag >= ICON_TAG_BASE || tag % 100 == FIELD_RESET_TEXT {
        Input::Index(1)
    } else {
        Input::Bool(on)
    }
}

/// Commands change a text field that's on screen, so its page must be rebuilt to show that.
pub fn needs_page_refresh(tag: i64) -> bool {
    tag >= ICON_TAG_BASE || tag / 100 != 0 && matches!(tag % 100, FIELD_ADD_VALUE | FIELD_ADD_ICON | FIELD_RESET_TEXT)
}

fn append_to_template(settings: &mut Settings, name: &str, text: &str) {
    let mut template = view::template_for(name, settings).to_string();
    template.push_str(text);
    if let Some(module) = settings.module_mut(name) {
        module.bar_template = Some(template);
    }
}

/// Returns false for a tag or value that doesn't fit, leaving settings unchanged.
pub fn apply(settings: &mut Settings, tag: i64, input: Input) -> bool {
    if tag >= ICON_TAG_BASE {
        let (page, index) = ((tag - ICON_TAG_BASE) / 100, (tag - ICON_TAG_BASE) % 100);
        return match (usize::try_from(page - 1).ok().and_then(|p| MODULES.get(p)), view::ICONS.get(index as usize)) {
            (Some(name), Some(icon)) => {
                append_to_template(settings, name, icon);
                true
            }
            _ => false,
        };
    }
    let (page, field) = (tag / 100, tag % 100);
    match (page, field, input) {
        (GENERAL, FIELD_UNIT, Input::Index(index)) => {
            settings.temperature_unit = if index == 1 { TemperatureUnit::Fahrenheit } else { TemperatureUnit::Celsius };
        }
        (GENERAL, FIELD_LOGIN, Input::Bool(on)) => settings.launch_at_login = on,
        (GENERAL, FIELD_SPACING, Input::Number(value)) => settings.item_spacing = value.clamp(0.0, MAX_ITEM_SPACING),
        (ALERTS, 0, Input::Bool(on)) => settings.alerts.disk_low_enabled = on,
        (ALERTS, 1, Input::Number(value)) => settings.alerts.disk_low_percent = value as f32,
        (ALERTS, 2, Input::Bool(on)) => settings.alerts.hot_enabled = on,
        (ALERTS, 3, Input::Number(value)) => settings.alerts.hot_c = value as f32,
        (ALERTS, 4, Input::Bool(on)) => settings.alerts.memory_pressure_enabled = on,
        (ALERTS, 5, Input::Bool(on)) => settings.alerts.connection_down_enabled = on,
        (1, FIELD_TOP_APPS, Input::Bool(on)) => settings.show_top_apps = on,
        (1, FIELD_WIFI, Input::Bool(on)) => settings.show_wifi = on,
        (1, FIELD_PING, Input::Text(host)) => settings.ping_host = host.trim().to_string(),
        (6, FIELD_FIXED, Input::Number(value)) => set_fixed_percent(settings, value as f32),
        (1..=7, FIELD_ADD_VALUE, Input::Index(index)) => {
            let name = MODULES[page as usize - 1];
            // Entry 0 is the menu's title.
            let Some(token) = index.checked_sub(1).and_then(|i| view::tokens(name).get(i)) else {
                return false;
            };
            append_to_template(settings, name, &format!("{{{}}}", token.name));
        }
        (1..=7, FIELD_RESET_TEXT, Input::Index(index)) => {
            if index == 0 {
                return false;
            }
            if let Some(module) = settings.module_mut(MODULES[page as usize - 1]) {
                module.bar_template = None;
            }
        }
        (1..=7, _, input) => {
            let Some(module) = settings.module_mut(MODULES[page as usize - 1]) else {
                return false;
            };
            match (field, input) {
                (FIELD_SHOWN, Input::Bool(on)) => module.shown = on,
                (FIELD_PEAKS, Input::Bool(on)) => module.show_peaks = on,
                (FIELD_BAR, Input::Index(index)) => {
                    module.bar_style = [BarStyle::Text, BarStyle::Graph, BarStyle::Both][index.min(2)];
                }
                (FIELD_STYLE, Input::Index(index)) => {
                    module.graph_style = [GraphStyle::Line, GraphStyle::Area, GraphStyle::Bars][index.min(2)];
                }
                (FIELD_SCALE, Input::Index(index)) => {
                    module.graph_scale = if index == 1 { GraphScale::Log } else { GraphScale::Linear };
                }
                (FIELD_RANGE, Input::Index(index)) => module.range_index = index.min(RANGES_SECONDS.len() - 1),
                (FIELD_COLOR, Input::Color(color)) => module.color = color,
                (FIELD_COLOR2, Input::Color(color)) => module.secondary_color = color,
                (FIELD_TEMPLATE, Input::Text(text)) => module.bar_template = Some(text),
                _ => return false,
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_has_controls() {
        let settings = Settings::default();
        for page in 0..PAGES.len() as i64 {
            assert!(!rows(page, &settings).is_empty(), "{}", PAGES[page as usize]);
        }
    }

    #[test]
    fn every_row_can_be_applied() {
        for page in 0..PAGES.len() as i64 {
            let mut settings = Settings::default();
            for row in rows(page, &Settings::default()) {
                let (tag, input) = match row.control {
                    Control::Heading => continue,
                    Control::Checkbox(on) => (row.tag, Input::Bool(!on)),
                    Control::Segmented(_, index) => (row.tag, Input::Index(index)),
                    Control::Colors(color, _) => (row.tag, Input::Color(color)),
                    Control::Text(text, _) => (row.tag, Input::Text(text)),
                    Control::Slider { value, .. } => (row.tag, Input::Number(value)),
                    Control::Menu(_) | Control::Button(_) => (row.tag, Input::Index(1)),
                    Control::Icons => (icon_tag(page, 0), Input::Index(1)),
                };
                assert!(apply(&mut settings, tag, input), "{}", row.label);
            }
        }
    }

    #[test]
    fn segmented_options_fit_their_saved_choice() {
        let settings = Settings::default();
        for page in 0..PAGES.len() as i64 {
            for row in rows(page, &settings) {
                if let Control::Segmented(options, selected) = row.control {
                    assert!(selected < options.len(), "{}", row.label);
                }
            }
        }
    }

    #[test]
    fn a_checkbox_changes_the_right_module() {
        let mut settings = Settings::default();
        let row = rows(3, &settings).into_iter().find(|r| r.label == "Show this item").unwrap();
        apply(&mut settings, row.tag, Input::Bool(false));
        assert!(!settings.cpu.shown);
        assert!(settings.network.shown);
    }

    #[test]
    fn adding_a_value_and_icon_extends_the_text() {
        let mut settings = Settings::default();
        assert!(apply(&mut settings, icon_tag(1, 4), Input::Index(1)));
        assert_eq!(settings.network.bar_template.as_deref(), Some("↑{up} ↓{down}▲"));
        assert!(apply(&mut settings, 100 + FIELD_ADD_VALUE, Input::Index(5)));
        assert_eq!(settings.network.bar_template.as_deref(), Some("↑{up} ↓{down}▲{ping}"));
        assert!(needs_page_refresh(100 + FIELD_ADD_VALUE));
        assert!(needs_page_refresh(icon_tag(1, 0)));
        assert!(!needs_page_refresh(100 + FIELD_TEMPLATE));
    }

    #[test]
    fn a_menu_title_click_adds_nothing() {
        let mut settings = Settings::default();
        assert!(!apply(&mut settings, 100 + FIELD_ADD_VALUE, Input::Index(0)));
        assert_eq!(settings.network.bar_template, None);
    }

    #[test]
    fn reset_returns_to_the_original_text() {
        let mut settings = Settings::default();
        apply(&mut settings, 100 + FIELD_TEMPLATE, Input::Text("x".into()));
        assert_eq!(view::template_for("network", &settings), "x");
        apply(&mut settings, 100 + FIELD_RESET_TEXT, Input::Index(1));
        assert_eq!(view::template_for("network", &settings), "↑{up} ↓{down}");
    }

    #[test]
    fn icons_reach_the_right_monitor_and_bad_tags_do_nothing() {
        let mut settings = Settings::default();
        assert!(apply(&mut settings, icon_tag(2, 0), Input::Index(1)));
        assert_eq!(settings.memory.bar_template.as_deref(), Some("RAM {percent}↑"));
        assert!(!apply(&mut settings, icon_tag(0, 0), Input::Index(1)));
        assert!(!apply(&mut settings, icon_tag(1, 99), Input::Index(1)));
    }

    #[test]
    fn buttons_and_checkboxes_report_differently() {
        assert_eq!(input_for_button(100 + FIELD_SHOWN, true), Input::Bool(true));
        assert_eq!(input_for_button(100 + FIELD_RESET_TEXT, false), Input::Index(1));
        assert_eq!(input_for_button(icon_tag(1, 2), false), Input::Index(1));
    }

    #[test]
    fn spacing_is_clamped() {
        let mut settings = Settings::default();
        apply(&mut settings, FIELD_SPACING, Input::Number(500.0));
        assert_eq!(settings.item_spacing, MAX_ITEM_SPACING);
    }

    #[test]
    fn unknown_tags_and_wrong_types_change_nothing() {
        let mut settings = Settings::default();
        assert!(!apply(&mut settings, 9999, Input::Bool(true)));
        assert!(!apply(&mut settings, 100, Input::Text("x".into())));
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn out_of_range_choices_are_clamped() {
        let mut settings = Settings::default();
        apply(&mut settings, 100 + FIELD_RANGE, Input::Index(99));
        assert_eq!(settings.network.range_index, RANGES_SECONDS.len() - 1);
    }

    #[test]
    fn ping_host_is_trimmed_and_fixed_speed_is_clamped() {
        let mut settings = Settings::default();
        apply(&mut settings, 100 + FIELD_PING, Input::Text(" 1.1.1.1 ".into()));
        assert_eq!(settings.ping_host, "1.1.1.1");
        apply(&mut settings, 600 + FIELD_FIXED, Input::Number(140.0));
        assert_eq!(fixed_percent(&settings), 100.0);
    }
}
