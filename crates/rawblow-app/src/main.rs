//! RawBlow — 빠른 사진 컬링 뷰어 (egui/wgpu 네이티브 GUI).
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod fonts;
mod logo;
#[cfg(target_os = "macos")]
mod macos_locale;
mod theme;
mod widgets;
mod worker;

use app::RawBlowApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    env_logger::init();
    install_panic_log();
    #[cfg(target_os = "macos")]
    macos_locale::align_with_system();
    let icon = egui::IconData {
        rgba: logo::icon_rgba(256),
        width: 256,
        height: 256,
    };
    let opts = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("RawBlow")
            .with_icon(std::sync::Arc::new(icon)),
        ..Default::default()
    };
    eframe::run_native(
        "RawBlow",
        opts,
        Box::new(|cc| Ok(Box::new(RawBlowApp::new(cc)))),
    )
}

/// 패닉(크래시)을 **바탕화면**의 `rawblow_crash.log`에 기록한다. 릴리스 빌드는 콘솔이
/// 없어 패닉 메시지가 사라지므로, 테스터가 바로 찾아 메일로 보낼 수 있게 눈에 띄는 곳에 남긴다.
fn install_panic_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let path = crash_log_path();
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use std::io::Write;
            let _ = writeln!(
                f,
                "\n==== RawBlow {} panic @ {:?} ====\n{}\n{}",
                env!("CARGO_PKG_VERSION"),
                std::time::SystemTime::now(),
                info,
                bt
            );
        }
        default(info);
    }));
}

/// 크래시 로그 경로: 바탕화면 우선(Windows: `%USERPROFILE%\Desktop` 또는 OneDrive 리디렉션,
/// macOS/Linux: `$HOME/Desktop`). 바탕화면을 못 찾으면 홈, 그래도 없으면 임시 폴더.
fn crash_log_path() -> std::path::PathBuf {
    const NAME: &str = "rawblow_crash.log";
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from);
    if let Some(home) = home {
        for sub in ["Desktop", "OneDrive/Desktop", "OneDrive - Personal/Desktop"] {
            let d = home.join(sub);
            if d.is_dir() {
                return d.join(NAME);
            }
        }
        return home.join(NAME);
    }
    std::env::temp_dir().join(NAME)
}
