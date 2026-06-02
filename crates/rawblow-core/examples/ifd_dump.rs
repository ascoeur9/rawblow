//! [임시] TIFF/RAW IFD 전수 덤프 — IFD0 + IFD chain + SubIFD(0x014A) 재귀, JPEG 포인터
//! (StripOffsets 0x0111, JPEGInterchangeFormat 0x0201, type-7) 위치/head 표시.
//! 사용: cargo run --release -p rawblow-core --example ifd_dump -- "<file>"

use std::io::{Read, Seek, SeekFrom};

fn main() {
    let path = std::env::args().nth(1).expect("file arg");
    let mut f = std::fs::File::open(&path).unwrap();
    let flen = f.metadata().unwrap().len();
    let mut buf = vec![0u8; 256 * 1024]; // 앞 256KB
    let n = f.read(&mut buf).unwrap();
    buf.truncate(n);

    let le = match &buf[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => { println!("not TIFF: {:02x}{:02x}", buf[0], buf[1]); return; }
    };
    let r16 = |o: usize| -> Option<u16> { let b = buf.get(o..o+2)?; Some(if le {u16::from_le_bytes([b[0],b[1]])} else {u16::from_be_bytes([b[0],b[1]])}) };
    let r32 = |o: usize| -> Option<u32> { let b = buf.get(o..o+4)?; Some(if le {u32::from_le_bytes([b[0],b[1],b[2],b[3]])} else {u32::from_be_bytes([b[0],b[1],b[2],b[3]])}) };
    println!("magic={:#x} filelen={:.1}MB header={}KB", r16(2).unwrap_or(0)&0xff, flen as f64/1e6, n/1024);

    let head_at = |off: u64| -> String {
        if (off as usize)+3 < buf.len() {
            format!("{:02x}{:02x}{:02x}", buf[off as usize], buf[off as usize+1], buf[off as usize+2])
        } else {
            // seek read 3 bytes
            let mut fh = std::fs::File::open(&path).unwrap();
            if fh.seek(SeekFrom::Start(off)).is_ok() {
                let mut b=[0u8;3]; if fh.read_exact(&mut b).is_ok() { return format!("{:02x}{:02x}{:02x}", b[0],b[1],b[2]); }
            }
            "??".into()
        }
    };
    let tsz = |t:u16| match t {1|2|6|7=>1,3|8=>2,4|9|11=>4,5|10|12=>8,_=>1};

    let mut to_scan: Vec<(usize, String)> = vec![(r32(4).unwrap_or(0) as usize, "IFD0".into())];
    let mut done = 0;
    while done < to_scan.len() && done < 12 {
        let (ifd, name) = to_scan[done].clone(); done += 1;
        let cnt = match r16(ifd) { Some(v)=>v as usize, None=>{ println!("{name} @ {ifd}: OUT OF HEADER (>{}KB)", n/1024); continue } };
        println!("--- {name} @ {ifd} ({cnt} entries) ---");
        for i in 0..cnt.min(60) {
            let e = ifd + 2 + i*12;
            let (tag,typ,c,val) = match (r16(e),r16(e+2),r32(e+4),r32(e+8)) { (Some(a),Some(b),Some(cc),Some(d))=>(a,b,cc as usize,d as u64), _=>break };
            let blen = c*tsz(typ);
            // JPEG 포인터 의심 태그
            let jpeg = (tag==0x0111||tag==0x0201) && blen<=4; // offset-holding
            let mark = match tag {
                0x0111=>"StripOffsets", 0x0117=>"StripByteCounts", 0x0201=>"JpgIFOffset",
                0x0202=>"JpgIFLength", 0x014a=>"SubIFDs", 0x0103=>"Compression",
                0x00fe=>"NewSubFileType", 0x0112=>"Orientation", _=> if typ==7 && c>1024 {"undef-blob"} else {""}
            };
            if !mark.is_empty() || (typ==7 && c>1024) {
                let h = if (tag==0x0111||tag==0x0201) || (typ==7 && c>1024) { head_at(val) } else { String::new() };
                println!("  tag={:#06x} type={typ} count={c} blen={blen} val={val} {mark} {}", tag, if h.is_empty(){String::new()}else{format!("head={h}")});
            }
            if tag==0x014a {
                if c==1 { to_scan.push((val as usize, format!("Sub@{val}"))); }
                else { for k in 0..c.min(8) { if let Some(so)=r32(val as usize + k*4) { to_scan.push((so as usize, format!("Sub[{k}]@{so}"))); } } }
            }
        }
        // IFD chain: next IFD pointer after entries
        let nxt_off = ifd + 2 + cnt*12;
        if let Some(nxt) = r32(nxt_off) { if nxt!=0 && (nxt as usize) < flen as usize && done < 6 { to_scan.push((nxt as usize, format!("IFD-next@{nxt}"))); } }
    }
}
