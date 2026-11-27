//! 파일 이름 문자열 도우미.

/// 맥 HFS NFD 한글을 NFC로 묶는다(#94). 자모가 분리되어 깨져 보이던 경로·파일명 표시와,
/// NFC·NFD가 섞인 이름을 화면 순서대로 정렬할 때(LUT 폴더 목록) 쓴다. 한글 자모만 묶는다.
pub fn nfc_hangul(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let cu = c as u32;
        if (0x1100..=0x1112).contains(&cu) && i + 1 < chars.len() {
            let v = chars[i + 1] as u32;
            if (0x1161..=0x1175).contains(&v) {
                let mut t_idx = 0u32;
                let mut used = 2;
                if i + 2 < chars.len() {
                    let t = chars[i + 2] as u32;
                    if (0x11A8..=0x11C2).contains(&t) {
                        t_idx = t - 0x11A8 + 1;
                        used = 3;
                    }
                }
                let l = cu - 0x1100;
                let v = v - 0x1161;
                let sidx = (l * 21 + v) * 28 + t_idx;
                if let Some(ch) = char::from_u32(0xAC00 + sidx) {
                    out.push(ch);
                    i += used;
                    continue;
                }
            }
        }
        if (0xAC00..=0xD7A3).contains(&cu) && (cu - 0xAC00).is_multiple_of(28) && i + 1 < chars.len() {
            let t = chars[i + 1] as u32;
            if (0x11A8..=0x11C2).contains(&t) {
                if let Some(ch) = char::from_u32(cu + (t - 0x11A8 + 1)) {
                    out.push(ch);
                    i += 2;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}
