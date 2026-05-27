//! 컬러 매니지먼트 (F9): 임베디드 ICC 프로파일을 sRGB로 변환.
//!
//! `color` 피처(qcms)에서 동작. 모니터 프로파일 조회는 v1 범위에서 sRGB 가정
//! (대부분의 모니터가 sRGB). HEIC가 흔히 다는 Display P3 등 광색역 태그를
//! sRGB로 라운드트립해 과채도를 막는 것이 1차 목표.
//!
//! RAW의 색공간/프리셋 처리는 범위 외(요청자 결정).

/// RGBA8 버퍼를 임베디드 ICC 기준으로 sRGB로 in-place 변환한다.
/// 변환을 수행했으면 true. (프로파일 파싱 실패·피처 off 시 false, 원본 유지.)
#[cfg(feature = "color")]
pub fn convert_to_srgb(rgba: &mut [u8], icc: &[u8]) -> bool {
    // 입력 프로파일.
    let mut input = match qcms::Profile::new_from_slice(icc, false) {
        Some(p) => p,
        None => return false,
    };
    // sRGB가 이미 입력이면 변환 불필요(서명 비교는 생략하고 변환 비용만 절감하려면
    // 별도 검사가 필요하지만, qcms가 동일 색공간이면 거의 항등 변환이므로 그대로 진행).
    let mut srgb = qcms::Profile::new_sRGB();
    srgb.precache_output_transform();
    input.precache_output_transform();

    let transform = match qcms::Transform::new(
        &input,
        &srgb,
        qcms::DataType::RGBA8,
        qcms::Intent::Perceptual,
    ) {
        Some(t) => t,
        None => return false,
    };
    transform.apply(rgba);
    true
}

#[cfg(not(feature = "color"))]
pub fn convert_to_srgb(_rgba: &mut [u8], _icc: &[u8]) -> bool {
    false
}
