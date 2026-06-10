//! 오픈소스 라이센스 고지(#39). `scripts/gen_licenses.ps1`이 배포 타겟 의존성에서 수집한
//! 라이센스 전문 JSON을 임베드하고, 설정에서 여는 라이센스 페이지의 상태를 담는다.
//! 의존성이 바뀌면 릴리즈 전에 스크립트를 재실행해 JSON을 갱신한다.

use serde::Deserialize;

/// 임베드된 수집 결과(JSON, ~365KB). 페이지를 처음 열 때 한 번 파싱한다.
const RAW: &str = include_str!("../assets/third_party_licenses.json");

#[derive(Deserialize)]
pub struct Doc {
    /// 수집 일자(YYYY-MM-DD).
    #[serde(default)]
    pub generated: String,
    pub crates: Vec<CrateEntry>,
    /// 라이센스 전문 풀(내용 기준 중복 제거). `CrateEntry::t`가 인덱스로 참조.
    pub texts: Vec<String>,
}

#[derive(Deserialize)]
pub struct CrateEntry {
    /// 크레이트 이름.
    pub n: String,
    /// 버전.
    pub v: String,
    /// SPDX 라이센스 표현식(Cargo.toml `license` 필드).
    pub l: String,
    /// 저장소 URL(LGPL 소스 입수처 안내 겸용).
    #[serde(default)]
    pub r: Option<String>,
    /// `Doc::texts` 인덱스들(라이센스 전문 + NOTICE).
    pub t: Vec<usize>,
}

/// 라이센스 페이지 UI 상태. 설정에서 열 때 생성, 닫으면 버린다(파싱 결과도 함께 해제).
pub struct LicensesPage {
    pub doc: Doc,
    pub selected: usize,
    pub filter: String,
}

impl LicensesPage {
    pub fn new() -> Self {
        // 임베드 데이터라 파싱 실패는 빌드 시점 버그 — 빈 문서로 폴백해 패닉만 피한다.
        let doc = serde_json::from_str(RAW).unwrap_or(Doc {
            generated: String::new(),
            crates: vec![],
            texts: vec![],
        });
        Self { doc, selected: 0, filter: String::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 임베드 JSON이 파싱되고, 모든 크레이트가 전문을 가지며 인덱스가 유효한지 검증.
    #[test]
    fn embedded_licenses_parse_and_are_complete() {
        let doc: Doc = serde_json::from_str(RAW).expect("third_party_licenses.json must parse");
        assert!(doc.crates.len() > 100, "dependency list looks truncated");
        assert!(!doc.texts.is_empty());
        for c in &doc.crates {
            assert!(!c.t.is_empty(), "{} has no license text", c.n);
            for &i in &c.t {
                assert!(i < doc.texts.len(), "{} has out-of-range text index", c.n);
            }
        }
        // LGPL 구성요소(#39의 핵심 의무)가 빠지지 않았는지.
        for name in ["rawloader", "imagepipe", "multicache"] {
            assert!(doc.crates.iter().any(|c| c.n == name), "{name} missing from notice");
        }
    }
}
