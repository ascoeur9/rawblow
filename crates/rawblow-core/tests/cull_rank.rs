//! 연사·유사 그룹 베스트(#124). 유한한 미적이 선명도만보다 앞이고,
//! 미적이 같으면 선명도가 높은 컷, 없거나 유한하지 않으면 선명도만.
//! `rank`는 예전 한 숫자 섞기(미적 없으면 선명도)라, 그 값과 어긋나도 새 순서가 이긴다.

use rawblow_core::cull_ext::{
    explain_group_culling, group_best_cmp, CullItem, GroupCullParams, MetaFilter, PhotoMeta,
};
use std::cmp::Ordering;

fn shot(i: i64, aesthetic: Option<f32>, sharp: f32, rank: f32) -> CullItem {
    CullItem {
        good: true,
        rank,
        aesthetic,
        sharp: Some(sharp),
        dhash: Some(0x1111_2222_3333_4444),
        shot_time: Some(1_000 + i),
        meta: PhotoMeta::default(),
    }
}

fn params(burst: bool, dedup: bool) -> GroupCullParams {
    GroupCullParams {
        use_meta: false,
        meta_filter: MetaFilter::default(),
        use_burst: burst,
        burst_gap_secs: 5,
        burst_keep: 1,
        use_dedup: dedup,
        dedup_hamming: 6,
        dedup_keep: 1,
    }
}

/// 연사와 유사 묶음 둘 다 같은 승자를 남기는지. 그룹이 안 묶이면 둘 다 좋음으로 남아 실패한다.
fn assert_kept(items: &[CullItem], good: &[bool]) {
    for (burst, dedup, name) in [(true, false, "burst"), (false, true, "dedup")] {
        let out = explain_group_culling(items, &params(burst, dedup));
        let got: Vec<bool> = out.iter().map(|o| o.good).collect();
        assert_eq!(got, good, "{name}");
        let grouped = if burst { out[0].burst.is_some() } else { out[0].dedup.is_some() };
        assert!(grouped, "{name} 그룹이 생기지 않음");
    }
}

#[test]
fn finite_aesthetic_outranks_sharpness_only() {
    // 예전 rank는 선명도 0.99가 미적 0.05를 이긴다. 지금은 미적이 있는 쪽이 남는다.
    let items = vec![shot(0, None, 0.99, 0.99), shot(1, Some(0.05), 0.0, 0.05)];
    assert_kept(&items, &[false, true]);
    // 미적이 음수여도 유한하면 선명도만보다 앞.
    let neg = vec![shot(0, None, 1.0, 1.0), shot(1, Some(-0.2), 0.0, -0.2)];
    assert_kept(&neg, &[false, true]);
}

#[test]
fn equal_aesthetic_keeps_the_sharper_shot() {
    // rank가 같으면 예전 정렬은 인덱스가 작은 쪽을 남긴다. 선명도로 뒤집혀야 한다.
    let items = vec![shot(0, Some(0.4), 0.1, 0.4), shot(1, Some(0.4), 0.9, 0.4)];
    assert_kept(&items, &[false, true]);
    // 미적이 다르면 선명도가 높아도 미적이 높은 쪽.
    let higher = vec![shot(0, Some(0.2), 1.0, 0.2), shot(1, Some(0.8), 0.0, 0.8)];
    assert_kept(&higher, &[false, true]);
}

#[test]
fn missing_or_non_finite_aesthetic_uses_sharpness_only() {
    // rank는 반대로 넣어 둔다. 선명도만으로 갈라야 한다.
    let missing = vec![shot(0, None, 0.2, 0.9), shot(1, None, 0.8, 0.1)];
    assert_kept(&missing, &[false, true]);
    let nan = vec![shot(0, Some(f32::NAN), 0.99, 0.99), shot(1, Some(0.0), 0.0, 0.0)];
    assert_kept(&nan, &[false, true]);
    let inf = vec![shot(0, Some(f32::INFINITY), 0.2, 1.0), shot(1, None, 0.8, 0.1)];
    assert_kept(&inf, &[false, true]);
}

#[test]
fn full_tie_keeps_the_earlier_index() {
    let items = vec![shot(0, Some(0.5), 0.3, 0.0), shot(1, Some(0.5), 0.3, 9.0)];
    assert_kept(&items, &[true, false]);
    let out = explain_group_culling(&items, &params(true, false));
    assert_eq!(out[0].burst.unwrap().rank, 1);
    assert_eq!(out[1].burst.unwrap().rank, 2);
}

#[test]
fn group_best_cmp_matches_the_three_rules() {
    assert_eq!(group_best_cmp(Some(0.05), 0.0, None, 0.99), Ordering::Less);
    assert_eq!(group_best_cmp(None, 1.0, Some(-0.2), 0.0), Ordering::Greater);
    assert_eq!(group_best_cmp(Some(0.4), 0.1, Some(0.4), 0.9), Ordering::Greater);
    assert_eq!(group_best_cmp(Some(0.2), 1.0, Some(0.8), 0.0), Ordering::Greater);
    assert_eq!(group_best_cmp(None, 0.2, None, 0.8), Ordering::Greater);
    assert_eq!(group_best_cmp(Some(f32::NAN), 0.99, Some(0.0), 0.0), Ordering::Greater);
    assert_eq!(group_best_cmp(Some(f32::INFINITY), 0.2, None, 0.8), Ordering::Greater);
    assert_eq!(group_best_cmp(Some(0.5), 0.3, Some(0.5), 0.3), Ordering::Equal);
    assert_eq!(group_best_cmp(None, f32::NAN, None, 0.0), Ordering::Greater);
}
