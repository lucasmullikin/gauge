//! `--render`: draws a panel or settings page to a PNG, so layouts can be checked without
//! screen recording permission. Uses a throwaway in-memory history, never the real one.

use crate::panel::Panel;
use crate::settings_window::SettingsWindow;
use crate::view::Latest;
use crate::worker::{self, NO_MODULE, Shared};
use gauge_core::history::History;
use gauge_core::settings::{MODULES, Settings};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSBitmapImageFileType, NSBox,
    NSBoxType, NSColor, NSView,
};
use objc2_foundation::{MainThreadMarker, NSDictionary, NSSize, NSString};
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const COLLECT_SECONDS: u64 = 9;

fn seed(history: &mut History, now: i64, range: i64) {
    let series: [(&str, f32); 14] = [
        ("net.down", 6e6), ("net.up", 8e5), ("mem.used", 80.0), ("mem.compressed", 9.0),
        ("cpu.total", 35.0), ("gpu.device", 60.0), ("gpu.memory", 38.0), ("disk.read", 2e7),
        ("disk.write", 8e6), ("fan.0", 2600.0), ("fan.1", 2800.0), ("temp.hot", 70.0),
        ("battery.percent", 76.0), ("ping", 4.0),
    ];
    let step = (range / 600).max(1);
    let mut t = now - range;
    let mut noise = 12345u32;
    while t < now {
        noise = noise.wrapping_mul(1103515245).wrapping_add(12345);
        let random = (noise >> 16) as f32 / 65535.0;
        let wave = ((t as f32 / range as f32) * 40.0).sin() * 0.3 + 0.7;
        let asleep = (t - (now - range)) * 10 / range == 6;
        if !asleep {
            for (name, level) in series {
                history.record(name, t, level * wave * (0.8 + random * 0.4));
            }
        }
        t += step;
    }
    let _ = history.flush(now + 120);
}

fn write_png(view: &NSView, path: &str) -> Result<(), String> {
    let bounds = view.bounds();
    let rep = view
        .bitmapImageRepForCachingDisplayInRect(bounds)
        .ok_or("no bitmap")?;
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    // SAFETY: an empty properties dictionary is valid for PNG output.
    let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
        .ok_or("no png")?;
    if data.writeToFile_atomically(&NSString::from_str(path), true) {
        Ok(())
    } else {
        Err(format!("couldn't write {path}"))
    }
}

/// Puts the view on a solid background, since a popover's material doesn't exist off screen.
fn on_background(mtm: MainThreadMarker, view: &NSView, dark: bool) -> Retained<NSBox> {
    view.layoutSubtreeIfNeeded();
    let size = view.fittingSize();
    let plate = NSBox::new(mtm);
    plate.setBoxType(NSBoxType::Custom);
    plate.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
    plate.setBorderWidth(0.0);
    plate.setContentViewMargins(NSSize::ZERO);
    let (r, g, b) = if dark { (0.13, 0.13, 0.14) } else { (0.96, 0.96, 0.96) };
    plate.setFillColor(&NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0));
    plate.setContentView(Some(view));
    // SAFETY: the appearance name statics are valid for the process's lifetime.
    let appearance = unsafe {
        NSAppearance::appearanceNamed(if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua })
    };
    plate.setAppearance(appearance.as_deref());
    plate.setFrameSize(size);
    plate.layoutSubtreeIfNeeded();
    plate
}

fn memory_sample() -> Option<gauge_core::memory::Memory> {
    gauge_core::memory::read()
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
    let [kind, name, path, rest @ ..] = args else {
        return Err("usage: --render panel MODULE FILE [dark] | settings PAGE FILE [dark]".into());
    };
    let dark = rest.iter().any(|a| a == "dark");
    let dummy: Retained<NSObject> = NSObject::new();
    let target: &AnyObject = dummy.as_ref();
    let settings = Settings::default();
    match kind.as_str() {
        "panel" => {
            let index = MODULES.iter().position(|m| m == name).ok_or("unknown module")?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
            // "real" reads the saved history, so point HOME at a copy of it to leave the original alone.
            let real = rest.iter().any(|a| a == "real");
            let mut history = if real {
                History::open(&gauge_core::settings::support_dir().join("history.sqlite"))
            } else {
                History::open_in_memory()
            }
            .map_err(|e| e.to_string())?;
            let module = settings.module(name).ok_or("unknown module")?;
            // "All" has no fixed length, so show it with three days of sample history.
            let seeded = match Panel::range_seconds(module) {
                gauge_core::settings::ALL_HISTORY => 3 * 86400,
                fixed => fixed,
            };
            if !real {
                seed(&mut history, now, seeded);
            }
            let shared = Arc::new(Shared {
                history: Arc::new(Mutex::new(history)),
                settings: Arc::new(Mutex::new(settings.clone())),
                open_module: AtomicUsize::new(NO_MODULE),
                install_message: Mutex::new(None),
            });
            shared.open_module.store(index, std::sync::atomic::Ordering::Relaxed);
            let (sender, receiver) = mpsc::channel();
            let worker_shared = shared.clone();
            std::thread::spawn(move || worker::run(sender, worker_shared));
            std::thread::sleep(Duration::from_secs(COLLECT_SECONDS));
            let mut latest = Latest::default();
            while let Ok(update) = receiver.try_recv() {
                worker::absorb(&mut latest, update);
            }
            let panel = Panel::new(mtm, MODULES[index], index, &settings, target);
            panel.refresh_text(&latest, &settings);
            let history = shared.history.lock().map_err(|e| e.to_string())?;
            let drawn_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(now, |d| d.as_secs() as i64);
            panel.refresh_graphs(&history, &settings, drawn_at, true);
            write_png(&on_background(mtm, panel.root_view(), dark), path)
        }
        "settings" => {
            let page: i64 = name.parse().map_err(|_| "bad page")?;
            let callbacks = crate::order_view::PreviewCallbacks {
                on_order: Box::new(|_| {}),
                on_select: Box::new(|_| {}),
            };
            let window = SettingsWindow::new(mtm, target, callbacks);
            window.show_page(mtm, page, &settings, target, Box::new(|_| {}));
            let sample = Latest {
                memory: memory_sample(),
                ..Latest::default()
            };
            window.set_preview(crate::view::preview_items(&sample, &settings), settings.item_spacing);
            let content = window.window.contentView().ok_or("no content")?;
            let plate = on_background(mtm, &content, dark);
            write_png(&plate, path)
        }
        _ => Err("unknown render kind".into()),
    }
}
