//! 새 릴리즈 안내(#33)(분해 2/8): GitHub Releases 확인·semver 비교.
//! app.rs에서 순수 이동 — 동작 변경 없음.

use super::*;

impl RawBlowApp {
    /// 새 릴리즈 안내(#33): 실행 후 로딩이 끝나고 유휴 상태일 때 **세션당 1회** 백그라운드로
    /// GitHub 최신 릴리즈를 확인한다(설정·시작 프롬프트 없이 자동). 결과는 채널로 받아 새 버전이면
    /// `update_available`에 담아 좌측 레일 배너로 알린다. 네트워크 실패·rate limit은 조용히 무시.
    pub(super) fn maybe_check_update(&mut self, ctx: &egui::Context) {
        // 진행 중이면 결과 수신.
        if let Some(rx) = &self.update_rx {
            match rx.try_recv() {
                Ok(msg) => {
                    self.update_available = msg;
                    self.update_rx = None;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => self.update_rx = None,
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    // 결과 올 때까지 가끔 깨운다(네트워크 스레드는 egui를 못 깨움).
                    ctx.request_repaint_after(Duration::from_secs(1));
                }
            }
        }
        // 자동 확인 끔(#69): 새 확인을 시작하지 않는다(이미 뜬 배너는 그대로 둔다). 나중에 다시 켜면
        // update_checked가 아직 false라 그때 1회 확인한다.
        if !self.cfg.check_updates {
            return;
        }
        if self.update_checked {
            return;
        }
        // 유휴 조건: 초기 디코딩/프리페치·진행 작업·모달이 모두 없을 때 한 번만.
        let idle = self.pending_preview.is_empty()
            && self.pending_thumb.is_empty()
            && self.pending_prefetch.is_empty()
            && self.progress.is_none()
            && !self.has_modal();
        if !idle {
            return;
        }
        self.update_checked = true;
        let (tx, rx) = crossbeam_channel::unbounded::<Option<String>>();
        std::thread::spawn(move || {
            let res = fetch_latest_release_tag().and_then(|tag| newer_version(&tag));
            let _ = tx.send(res);
        });
        self.update_rx = Some(rx);
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

// ── 새 릴리즈 안내(#33) ─────────────────────────────────────────────

/// GitHub 최신 릴리즈 tag_name을 가져온다(네트워크, update-check 기능 시). 실패 시 None.
/// 동기 HTTP(ureq) — 호출부가 백그라운드 스레드에서 돌린다. User-Agent 필수(없으면 403).
#[cfg(feature = "update-check")]
pub(super) fn fetch_latest_release_tag() -> Option<String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(8))
        .build();
    let body = agent
        .get("https://api.github.com/repos/ascoeur9/rawblow/releases/latest")
        .set("User-Agent", concat!("RawBlow/", env!("CARGO_PKG_VERSION")))
        .set("Accept", "application/vnd.github+json")
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    v.get("tag_name").and_then(|t| t.as_str()).map(|s| s.to_string())
}

/// update-check 기능을 끈 빌드(예: cc 없는 환경의 cargo check)에서는 항상 None.
#[cfg(not(feature = "update-check"))]
pub(super) fn fetch_latest_release_tag() -> Option<String> {
    None
}

/// `v1.2.3`/`1.2.3`(프리릴리스/빌드 메타 허용) → (major, minor, patch). 실패 시 None.
pub(super) fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let t = s.trim().trim_start_matches(['v', 'V']);
    let core = t.split(['-', '+']).next().unwrap_or(t);
    let mut it = core.split('.');
    let a = it.next()?.trim().parse().ok()?;
    let b = it.next().unwrap_or("0").trim().parse().ok()?;
    let c = it.next().unwrap_or("0").trim().parse().ok()?;
    Some((a, b, c))
}

/// 최신 태그가 현재 버전보다 높으면 표시용 버전 문자열을 반환(아니면 None).
pub(super) fn newer_version(latest_tag: &str) -> Option<String> {
    let cur = parse_semver(env!("CARGO_PKG_VERSION"))?;
    let lat = parse_semver(latest_tag)?;
    (lat > cur).then(|| latest_tag.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::{newer_version, parse_semver};

    #[test]
    fn semver_parse_and_compare() {
        assert_eq!(parse_semver("v0.4.3"), Some((0, 4, 3)));
        assert_eq!(parse_semver("0.4.3"), Some((0, 4, 3)));
        assert_eq!(parse_semver("v1.2"), Some((1, 2, 0)));
        assert_eq!(parse_semver("v0.6.0-rc1"), Some((0, 6, 0)));
        assert_eq!(parse_semver("nope"), None);
        // 현재 버전보다 높은 태그만 새 버전으로 인식.
        assert!(newer_version("v999.0.0").is_some());
        assert!(newer_version("v0.0.1").is_none());
        assert!(newer_version(env!("CARGO_PKG_VERSION")).is_none(), "동일 버전은 안내 안 함");
    }
}
