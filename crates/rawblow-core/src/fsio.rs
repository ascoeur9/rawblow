//! 원자적 파일 저장 헬퍼: 같은 폴더의 임시 파일에 쓴 뒤 rename으로 교체한다.
//! 사이드카·설정처럼 "저장 도중 크래시·전원차단·디스크풀이 나도 기존 파일이 잘린 채
//! 남으면 안 되는" 파일에 쓴다. rename은 같은 볼륨 안에서 원자적이므로(Windows도 기존
//! 파일 교체 지원 — 썸네일 캐시 cache.rs에서 검증된 패턴) 대상 경로에는 관찰 시점에
//! 항상 이전본 전체 또는 새본 전체만 존재한다.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 대상과 같은 폴더에 만드는 임시 경로. 같은 볼륨이어야 rename이 원자적이므로
/// 시스템 temp가 아니라 대상 옆에 만든다. pid+seq로 동시 쓰기 충돌 방지.
fn tmp_path_for(path: &Path) -> PathBuf {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    dir.join(format!(".{name}.tmp-{}-{seq}", std::process::id()))
}

/// `bytes`를 임시 파일에 쓰고 **fsync 후** `path`로 rename해 원자적으로 교체한다.
/// 전원차단 직후에도 새본이 디스크에 닿아 있도록 보장한다. 사용자 데이터(사이드카,
/// 컬링 캐시)처럼 유실되면 안 되는 파일에 쓴다. 실패 시 임시 파일은 정리된다.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic_inner(path, bytes, true)
}

/// `write_atomic`과 같지만 fsync를 생략한다. rename 원자성으로 "잘린 파일"은 여전히
/// 방지되지만, 전원차단 시 새본이 유실될 수 있다(이전본 또는 빈 상태). 설정처럼
/// 슬라이더 드래그 등 UI 스레드에서 빈번히 저장되고 유실돼도 재생성 가능한 파일에 쓴다.
pub fn write_atomic_nosync(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic_inner(path, bytes, false)
}

fn write_atomic_inner(path: &Path, bytes: &[u8], durable: bool) -> std::io::Result<()> {
    let tmp = tmp_path_for(path);
    let write_then_rename = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        if durable {
            f.sync_all()?;
        }
        drop(f);
        std::fs::rename(&tmp, path)
    };
    if let Err(e) = write_then_rename() {
        let _ = std::fs::remove_file(&tmp); // 실패 시 임시 파일 정리(없으면 무시)
        return Err(e);
    }
    // rename 자체(디렉토리 엔트리)의 내구성은 best-effort(Unix 전용, 실패 무시).
    #[cfg(unix)]
    if durable {
        if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            if let Ok(d) = std::fs::File::open(dir) {
                let _ = d.sync_all();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_creates_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("data.json");
        write_atomic(&p, b"v1").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"v1");
        write_atomic(&p, b"v2-longer").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"v2-longer");
        // 성공 후 임시 파일이 남지 않는다.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "임시 파일 잔존: {leftovers:?}");
    }

    #[test]
    fn write_atomic_fails_cleanly_when_dir_missing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("no-such-subdir").join("data.json");
        assert!(write_atomic(&p, b"x").is_err());
        assert!(!p.exists());
    }

    #[test]
    fn write_atomic_nosync_same_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("cfg.json");
        write_atomic_nosync(&p, b"a").unwrap();
        write_atomic_nosync(&p, b"b").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"b");
    }
}
