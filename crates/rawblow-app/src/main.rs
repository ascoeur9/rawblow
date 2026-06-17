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
    // 런타임 앱 아이콘. macOS에선 eframe이 `setApplicationIconImage`로 이 아이콘을 Dock·cmd+Tab에
    // 런타임 적용하며, 이게 `.app` 번들의 `.icns`(표준 여백 적용본)를 덮어쓴다. 따라서 여기서도 풀블리드면
    // .icns 여백이 무효가 돼 다른 앱보다 크게 보인다(#46 — v0.4.7에서 .icns만 고쳐선 안 잡힌 이유).
    // macOS만 .icns와 동일한 표준 여백을 두고, Windows 작업표시줄·Linux는 풀블리드(꽉 채움)가 표준이라 유지.
    #[cfg(target_os = "macos")]
    let icon_rgba = logo::icon_rgba_inset(256, logo::MACOS_ICON_MARGIN);
    #[cfg(not(target_os = "macos"))]
    let icon_rgba = logo::icon_rgba(256);
    let icon = egui::IconData {
        rgba: icon_rgba,
        width: 256,
        height: 256,
    };
    let opts = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        // 렌더 백엔드 선택(#43 + 구형 PC OpenGL 폴백) — 규칙은 select_backends() 주석 참고.
        // 모던 GPU(DX12/Vulkan)가 있으면 그것만 써서 구형 AMD OpenGL ICD(atio6axx.dll) 로드로 인한
        // #43 크래시를 피하고, 모던 GPU가 없으면 OpenGL을 폴백으로 켜 구형 PC도 최대한 실행되게 한다.
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            supported_backends: select_backends(),
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

/// 렌더 백엔드 선택(#43 + 구형 PC OpenGL 폴백).
///
/// 1) `WGPU_BACKEND` 환경변수가 있으면 그 지정을 최우선으로 따른다(진단·강제용. 예: `WGPU_BACKEND=gl`로
///    OpenGL 경로를 강제 — Vulkan/DX12가 되는 PC에서도 폴백 동작을 테스트할 수 있다).
/// 2) 없으면 **OpenGL을 제외한** 모던 백엔드(DX12/Vulkan/Metal = PRIMARY)로 그래픽 어댑터가 있는지 먼저
///    조용히 탐지한다. 이 탐지 단계는 OpenGL ICD(AMD `atio6axx.dll` 등)를 절대 로드하지 않으므로, #43 같은
///    구형 OpenGL 드라이버 크래시를 건드리지 않는다.
/// 3) 모던 GPU가 있으면 그것만 쓴다(OpenGL 미로드 → #43 회피). 없으면(아주 구형 PC) OpenGL을 폴백으로
///    추가해 최대한 실행되게 한다. 단 OpenGL 3.3 미만(예: Sandy Bridge HD 3000)은 wgpu가 지원하지 않아
///    이 폴백으로도 실행되지 않을 수 있다.
fn select_backends() -> eframe::wgpu::Backends {
    use eframe::wgpu;
    // (1) 사용자가 환경변수로 강제하면 그대로 따른다.
    if let Some(forced) = wgpu::util::backend_bits_from_env() {
        return forced;
    }
    // (2) OpenGL 없이 모던 GPU만 탐지(GL ICD를 로드하지 않는다).
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });
    let has_modern_gpu = instance
        .enumerate_adapters(wgpu::Backends::PRIMARY)
        .iter()
        .any(|a| a.get_info().device_type != wgpu::DeviceType::Cpu);
    // (3) 모던 GPU가 있으면 PRIMARY만, 없으면 OpenGL 폴백을 더한다.
    if has_modern_gpu {
        wgpu::Backends::PRIMARY
    } else {
        log::warn!("DX12/Vulkan 어댑터를 찾지 못해 OpenGL 폴백을 활성화합니다(구형 GPU).");
        wgpu::Backends::PRIMARY | wgpu::Backends::GL
    }
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
