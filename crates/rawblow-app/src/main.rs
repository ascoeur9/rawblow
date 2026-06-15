//! RawBlow — 빠른 사진 컬링 뷰어 (egui/wgpu 네이티브 GUI).
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod fonts;
mod i18n;
mod licenses;
mod logo;
mod map;
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
        // 이슈 #43: 일부 Windows + 구형 AMD GPU에서 시작 직후 크래시(0xc0000005, atio6axx.dll).
        // egui-wgpu의 기본 백엔드는 PRIMARY|GL이라 AMD OpenGL ICD(atio6axx.dll)까지 로드·초기화하는데,
        // 구형 드라이버에서 어댑터 열거 도중 액세스 위반이 났다. GL을 빼고 DX12/Vulkan만 쓰면 그 ICD를
        // 아예 로드하지 않아 크래시를 피한다. 진단·우회가 필요하면 WGPU_BACKEND 환경변수로 강제 지정한다.
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            supported_backends: eframe::wgpu::util::backend_bits_from_env()
                .unwrap_or(eframe::wgpu::Backends::PRIMARY),
            ..Default::default()
        },
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
