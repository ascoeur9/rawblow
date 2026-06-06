//! RawBlow core — GUI에 의존하지 않는 순수 로직 계층.
//!
//! 폴더 스캔/페어링, 이미지·RAW 디코딩, ICC 컬러 변환, EXIF 추출,
//! 사이드카 저장/복원, 파일 전송, 설정을 담당한다. eframe/egui를 일절
//! 참조하지 않으므로 헤드리스 환경에서도 전 범위 단위테스트가 가능하다.

pub mod cache;
pub mod color;
pub mod config;
pub mod decode;
pub mod meta;
pub mod model;
pub mod organize;
pub mod scan;
pub mod sidecar;
pub mod transfer;

pub use model::{
    ColorTag, Entry, Filter, Label, MatchMode, SortOrder, StarFilter, TagFilter, ViewMode,
};
