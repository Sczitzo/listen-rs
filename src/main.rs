#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Listen — 100% pure-Rust native local dictation, Command Mode, and meeting notetaker for Windows
//! (original by Sczitzo, MIT License).

mod app;
mod app_tone;
mod audio;
mod cleanup;
mod command_mode;
mod controller;
mod dashboard_state;
mod diagnostics;
mod engine;
mod formatting;
#[cfg(windows)]
mod history_icon;
mod lantern;
mod mark;
mod mechanics;
mod meeting;
mod models;
mod settings_bridge;
mod snippets;
mod speech;
mod storage;
mod theme;
mod tray;
mod ui;
mod winput;
mod workspace;

use app::{LaunchOptions, ListenApp, Page};
use eframe::egui;
use std::path::PathBuf;
use theme::{LayoutId, ThemeId};

fn icon(size: u32) -> egui::IconData {
    egui::IconData { rgba: mark::rgba(size, mark::IDLE), width: size, height: size }
}

fn main() -> eframe::Result<()> {
    let mut opts = LaunchOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--measure-mechanics" => {
                let result = args.next().ok_or_else(|| "--measure-mechanics needs a new output path".to_owned())
                    .and_then(|path| mechanics::write(std::path::Path::new(&path)));
                if let Err(error) = result {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            #[cfg(all(windows, debug_assertions))]
            "--check-multiline" => {
                let Some(directory) = args.next() else { return Ok(()) };
                if let Err(error) = winput::check_multiline(std::path::Path::new(&directory)) {
                    eprintln!("Multiline test failed: {error}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            "--hardware-trial" if cfg!(debug_assertions) => {
                let Some(directory) = args.next() else { return Ok(()) };
                let directory = PathBuf::from(directory);
                if std::env::var_os("APPDATA").map(PathBuf::from).as_ref() != Some(&directory)
                    || !directory.join("rust-test-isolated").is_file() {
                    eprintln!("The physical trial requires a fresh isolated profile");
                    return Ok(());
                }
                opts.hardware_trial = Some(directory);
            }
            "--dashboard-check" | "--ui-check" => {
                let Some(directory) = args.next() else { return Ok(()) };
                opts.ui_check = Some(PathBuf::from(directory));
            }
            "--screenshot" => {
                let Some(path) = args.next() else {
                    eprintln!("--screenshot needs an output path");
                    return Ok(());
                };
                opts.screenshot = Some(PathBuf::from(path));
            }
            "--screenshot-lantern" => {
                let Some(path) = args.next() else {
                    eprintln!("--screenshot-lantern needs an output path");
                    return Ok(());
                };
                opts.screenshot = Some(PathBuf::from(path));
                opts.demo_lantern = true;
            }
            "--theme" => opts.theme = args.next().and_then(|v| ThemeId::from_key(&v)),
            "--layout" => opts.layout = args.next().and_then(|v| LayoutId::from_key(&v)),
            "--page" => opts.page = args.next().and_then(|v| Page::from_key(&v)),
            "--settings-section" => opts.settings_section = args.next(),
            "--demo-recording" => opts.demo_recording = true,
            _ => {}
        }
    }
    let preview = opts.screenshot.is_some() || opts.ui_check.is_some();
    if !preview && !winput::single_instance() {
        return Ok(());
    }
    let report_profile = opts.ui_check.clone()
        .or_else(|| opts.screenshot.as_ref().map(|file| file.with_extension("profile")))
        .unwrap_or_else(storage::data_dir);
    diagnostics::install_crash_report(&report_profile);
    if !preview {
        opts.launch_target = winput::capture_target();
    }
    let mut settings = if preview { storage::Settings::default() } else { storage::load_settings() };
    if opts.ui_check.is_some() {
        settings.microphone = "Disconnected sample microphone".into();
        settings.live_preview = true;
        settings.floating_card = true;
    }
    let layout = if preview { opts.layout.unwrap_or(LayoutId::Desk) } else { opts.layout.unwrap_or(settings.layout) };
    let size = if opts.demo_lantern {
        [lantern::WIDTH, 247.0]
    } else if layout == LayoutId::Compact {
        [480.0, 760.0]
    } else if preview {
        [920.0, 760.0]
    } else {
        [920.0, 720.0]
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(if opts.hardware_trial.is_some() { "Listen Rust Trial" } else { "Listen" })
        .with_app_id("listen-rs")
        .with_inner_size(size)
        .with_min_inner_size([480.0, 520.0])
        .with_icon(std::sync::Arc::new(icon(256)));
    if opts.hardware_trial.is_some() {
        viewport = viewport.with_active(false);
    }
    if preview {
        viewport = viewport.with_active(false);
        if opts.demo_lantern {
            viewport = viewport.with_decorations(false).with_resizable(false)
                .with_transparent(true).with_min_inner_size([1.0, 1.0]);
        }
        #[cfg(windows)]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let native = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    let result = eframe::run_native(
        "Listen",
        native,
        Box::new(move |cc| Ok(Box::new(ListenApp::new(cc, settings, opts)))),
    );
    if result.is_err() {
        diagnostics::error_at(&report_profile, "Listen could not start or run the desktop window.");
    }
    result
}
