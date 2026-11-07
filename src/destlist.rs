//! 从 Windows 跳转列表里读「最近打开的文件夹」。
//!
//! 数据源是资源管理器的跳转列表容器：
//!
//! ```text
//! %APPDATA%\Microsoft\Windows\Recent\AutomaticDestinations\
//!     f01b4d95cf55d32a.automaticDestinations-ms
//! ```
//!
//! `f01b4d95cf55d32a` 是资源管理器固定的 AppID，这个文件是一个 **复合文档**
//! （Compound File Binary，`D0 CF 11 E0 …`），里面除了 `DestList` 还有一堆以数字命名的
//! `.lnk` 流。`DestList` 流才是我们要的：它是按**时间倒序**排列的访问记录，每条带
//! 目标路径、最后访问时间和一个递增编号。
//!
//! 这里不借 COM（`StgOpenStorageEx`），自己解 CFB：解出来的东西能写成纯函数、能用
//! 合成字节做单测，比在测试里依赖真实用户目录稳得多。
//!
//! `DestList` 版本 6（Win11）的条目布局（相对路径起点）：
//!
//! ```text
//! -0x28  u64  entry_id       2657, 2655, 2654 … 严格递减
//! -0x20  f32  weight         1.000, 0.980 … 新近度衰减分
//! -0x1c  u64  FILETIME       最后访问时间
//! -0x14  i32  -1             常量
//! -0x10  u32  stream_number  对应同目录下数字命名的 `.lnk` 流
//! -0x0c  u32  pinned
//! -0x08  u32  0
//! -0x04  u32  0
//! +0x00  u16  路径字符数
//! +0x02  …    UTF-16LE 路径
//! ```
//!
//! 条目之间还有不定长的填充（`.lnk` 的 shell item 数据），所以扫描是「按 2 字节步进，
//! 每个候选位置用上面那串字段做交叉校验」，而不是顺序遍历。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 资源管理器的固定 AppID（跳转列表文件名去掉扩展名）
pub const EXPLORER_APP_ID: &str = "f01b4d95cf55d32a";

/// `DestList` 流的名字
const DEST_LIST: &str = "DestList";

/// CFB 文件头长度
const HEADER_LEN: usize = 512;
/// CFB 签名
const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// FAT 链表里的结束标记
const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
/// FAT 里的「空闲扇区」标记
const FREE_SECT: u32 = 0xFFFF_FFFF;

/// 一条访问记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 打开过的路径（可能是盘符路径或 UNC）
    pub path: PathBuf,
    /// 最后访问时间（`DestList` 里的 FILETIME；早于 1970 的当缺失）
    pub last_access: Option<SystemTime>,
    /// 递增编号，越大越新
    pub entry_id: u64,
}

/// 跳转列表文件的位置（`%APPDATA%\Microsoft\Windows\Recent\AutomaticDestinations\…`）
pub fn list_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        Path::new(&appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Recent")
            .join("AutomaticDestinations")
            .join(format!("{EXPLORER_APP_ID}.automaticDestinations-ms")),
    )
}

/// 读文件 → 取 `DestList` 流 → 解析成条目。任何一步失败都返回空（这是一个「有更好、
/// 没有也能用」的数据源，不该因为文件被占用 / 版本不认识就报错）。
pub fn load() -> Vec<Entry> {
    let Some(path) = list_path() else {
        return Vec::new();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    let Some(stream) = read_stream(&bytes, DEST_LIST) else {
        return Vec::new();
    };
    parse_dest_list(&stream)
}

// ---------------------------------------------------------------------------
// 复合文档（CFB）读取
// ---------------------------------------------------------------------------

/// 从复合文档字节里取出一个命名流的内容。
///
/// 只做读：走 DIFAT → FAT → 目录项，再按流的起始扇区沿 FAT 链（小于
/// `mini stream cutoff` 时改走 mini FAT）拼出内容。
fn read_stream(data: &[u8], name: &str) -> Option<Vec<u8>> {
    if data.len() < HEADER_LEN || data[..8] != SIGNATURE {
        return None;
    }

    let sector_size = 1usize << u16_at(data, 0x1E)?;
    let mini_sector_size = 1usize << u16_at(data, 0x20)?;
    if sector_size == 0 || mini_sector_size == 0 {
        return None;
    }

    let num_fat = u32_at(data, 0x2C)?;
    let first_dir = u32_at(data, 0x30)?;
    let mini_cutoff = u32_at(data, 0x38)?;
    let first_mini_fat = u32_at(data, 0x3C)?;
    let num_mini_fat = u32_at(data, 0x40)?;
    let first_difat = u32_at(data, 0x44)?;
    let num_difat = u32_at(data, 0x48)?;

    let fat = read_fat(data, sector_size, num_fat, first_difat, num_difat)?;
    let dir = read_chain(data, sector_size, &fat, first_dir, usize::MAX)?;
    let entries = parse_directory(&dir);
    let stream = entries.iter().find(|e| e.name == name && e.kind == 2)?;

    let size = stream.size as usize;
    if size == 0 {
        return Some(Vec::new());
    }

    if (size as u64) < u64::from(mini_cutoff) {
        // 小流放在 mini stream 里：先取根目录项指向的 mini stream，再走 mini FAT
        let root = entries.iter().find(|e| e.kind == 5)?;
        let mini_fat = read_mini_fat(data, sector_size, &fat, first_mini_fat, num_mini_fat)?;
        let mini = read_chain(data, sector_size, &fat, root.start, usize::MAX)?;
        let mut out = read_mini_chain(&mini, mini_sector_size, &mini_fat, stream.start, size)?;
        out.truncate(size);
        Some(out)
    } else {
        let mut out = read_chain(data, sector_size, &fat, stream.start, size)?;
        out.truncate(size);
        Some(out)
    }
}

/// 扇区编号 → 文件里的字节偏移（扇区 0 紧跟在 512 字节文件头之后）
fn sector_offset(sector: u32, sector_size: usize) -> usize {
    (sector as usize + 1) * sector_size
}

/// 收集并展开 FAT（DIFAT 前 109 项在文件头里，再往后走 DIFAT 扇区链）
fn read_fat(
    data: &[u8],
    sector_size: usize,
    num_fat: u32,
    first_difat: u32,
    num_difat: u32,
) -> Option<Vec<u32>> {
    let mut sectors: Vec<u32> = Vec::new();

    // 文件头里最多 109 个 FAT 扇区号
    for i in 0..109 {
        let sector = u32_at(data, 0x4C + i * 4)?;
        if sector != FREE_SECT && sector != END_OF_CHAIN {
            sectors.push(sector);
        }
    }

    // 超过 109 个时，剩下的在 DIFAT 扇区里（每个扇区最后一个 u32 指向下一个 DIFAT）
    let per_sector = sector_size / 4 - 1;
    let mut current = first_difat;
    for _ in 0..num_difat {
        if current == END_OF_CHAIN || current == FREE_SECT {
            break;
        }
        let offset = sector_offset(current, sector_size);
        if offset + sector_size > data.len() {
            break;
        }
        for i in 0..per_sector {
            let sector = u32_at(data, offset + i * 4)?;
            if sector != FREE_SECT && sector != END_OF_CHAIN {
                sectors.push(sector);
            }
        }
        current = u32_at(data, offset + per_sector * 4)?;
    }

    sectors.truncate(num_fat as usize);

    let mut fat = Vec::with_capacity(sectors.len() * (sector_size / 4));
    for sector in sectors {
        let offset = sector_offset(sector, sector_size);
        if offset + sector_size > data.len() {
            return None;
        }
        for i in 0..sector_size / 4 {
            fat.push(u32_at(data, offset + i * 4)?);
        }
    }
    Some(fat)
}

/// mini FAT（结构同 FAT，但索引的是 mini 扇区）
fn read_mini_fat(
    data: &[u8],
    sector_size: usize,
    fat: &[u32],
    first: u32,
    count: u32,
) -> Option<Vec<u32>> {
    if count == 0 {
        return Some(Vec::new());
    }
    let bytes = read_chain(data, sector_size, fat, first, usize::MAX)?;
    let mut out: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    out.truncate(count as usize * (sector_size / 4));
    Some(out)
}

/// 沿 mini FAT 链取一个小流
fn read_mini_chain(
    mini: &[u8],
    mini_sector_size: usize,
    mini_fat: &[u32],
    start: u32,
    size: usize,
) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(size.min(mini.len()));
    let mut sector = start;
    let mut guard = 0usize;
    while sector != END_OF_CHAIN && sector != FREE_SECT {
        let offset = sector as usize * mini_sector_size;
        if offset + mini_sector_size > mini.len() {
            break;
        }
        out.extend_from_slice(&mini[offset..offset + mini_sector_size]);
        if out.len() >= size {
            break;
        }
        sector = *mini_fat.get(sector as usize)?;
        guard += 1;
        if guard > mini_fat.len() + 1 {
            break;
        }
    }
    Some(out)
}

/// 沿 FAT 链把若干扇区拼起来
fn read_chain(
    data: &[u8],
    sector_size: usize,
    fat: &[u32],
    start: u32,
    limit: usize,
) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut sector = start;
    let mut guard = 0usize;
    while sector != END_OF_CHAIN && sector != FREE_SECT {
        let offset = sector_offset(sector, sector_size);
        if offset + sector_size > data.len() {
            break;
        }
        out.extend_from_slice(&data[offset..offset + sector_size]);
        if out.len() >= limit {
            break;
        }
        let next = *fat.get(sector as usize)?;
        if next == sector {
            break;
        }
        sector = next;
        guard += 1;
        if guard > fat.len() + 1 {
            break;
        }
    }
    Some(out)
}

/// 目录项（128 字节一个）
struct DirEntry {
    name: String,
    kind: u8,
    start: u32,
    size: u64,
}

fn parse_directory(data: &[u8]) -> Vec<DirEntry> {
    let mut out = Vec::new();
    for chunk in data.as_chunks::<128>().0 {
        let name_len = u16::from_le_bytes([chunk[0x40], chunk[0x41]]) as usize;
        let kind = chunk[0x42];
        if kind == 0 || !(2..=64).contains(&name_len) {
            continue;
        }
        let units: Vec<u16> = chunk[..name_len - 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let name = String::from_utf16_lossy(&units);
        let start = u32::from_le_bytes([chunk[0x74], chunk[0x75], chunk[0x76], chunk[0x77]]);
        let size = u64::from_le_bytes([
            chunk[0x78], chunk[0x79], chunk[0x7A], chunk[0x7B], chunk[0x7C], chunk[0x7D], chunk[0x7E],
            chunk[0x7F],
        ]);
        out.push(DirEntry {
            name,
            kind,
            start,
            size,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// DestList 解析
// ---------------------------------------------------------------------------

/// 解析 `DestList` 流。返回**保持文件顺序**（也就是时间倒序）的条目。
pub fn parse_dest_list(data: &[u8]) -> Vec<Entry> {
    // 一个条目前缀最少 42 字节（见文件头注释），所以从 0x2A 起才可能命中
    const PREFIX: usize = 0x2A;
    const MAX_CHARS: usize = 400;

    let mut out: Vec<Entry> = Vec::new();
    let mut i = PREFIX;

    while i + 2 <= data.len() {
        let chars = u16::from_le_bytes([data[i], data[i + 1]]) as usize;

        // 快筛：路径的首字符必须是盘符（`C`）或反斜杠（`\\`），或是 UTF-16 高字节 0
        let first = data.get(i + 2).copied().unwrap_or(0);
        let plausible_first = first.is_ascii_alphabetic() || first == b'\\';

        if plausible_first && (3..=MAX_CHARS).contains(&chars) {
            let end = i + 2 + chars * 2;
            if end <= data.len()
                && let Some(entry) = entry_at(data, i, chars)
            {
                out.push(entry);
                i = end;
                continue;
            }
        }
        i += 2;
    }

    out
}

/// 在 `pos`（路径长度字段的位置）尝试解出一条记录
fn entry_at(data: &[u8], pos: usize, chars: usize) -> Option<Entry> {
    if pos < 0x28 {
        return None;
    }

    let entry_id = u64_at(data, pos - 0x28)?;
    let weight = f32::from_le_bytes([
        data[pos - 0x20],
        data[pos - 0x1F],
        data[pos - 0x1E],
        data[pos - 0x1D],
    ]);
    let filetime = u64_at(data, pos - 0x1C)?;
    let sentinel = i32::from_le_bytes([
        data[pos - 0x14],
        data[pos - 0x13],
        data[pos - 0x12],
        data[pos - 0x11],
    ]);
    let stream_number = u32_at(data, pos - 0x10)?;
    let pinned = u32_at(data, pos - 0x0C)?;

    // 交叉校验：这几个字段同时「像那么回事」才认
    if entry_id == 0 || entry_id > 10_000_000 {
        return None;
    }
    if !(weight > 0.0 && weight <= 1.0001) {
        return None;
    }
    if sentinel != -1 || stream_number > 100_000 || pinned > 1 {
        return None;
    }

    let last_access = filetime_to_time(filetime)?;

    let raw = data.get(pos + 2..pos + 2 + chars * 2)?;
    let units: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    // UTF-16 里不该出现控制字符；出现了说明位置没对准
    if units
        .iter()
        .any(|&u| (u < 0x20 && u != 0x09) || u >= 0x2000)
    {
        return None;
    }
    let text = String::from_utf16_lossy(&units);

    if !looks_like_path(&text) {
        return None;
    }

    Some(Entry {
        path: PathBuf::from(text),
        last_access: Some(last_access),
        entry_id,
    })
}

/// `C:\…` 或 `\\server\share…`
fn looks_like_path(text: &str) -> bool {
    if text.starts_with("\\\\") {
        return text.len() > 2;
    }
    let bytes = text.as_bytes();
    bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

/// FILETIME（1601 起 100ns）→ `SystemTime`。早于 1970 的返回 `None`。
fn filetime_to_time(filetime: u64) -> Option<SystemTime> {
    const UNIX_EPOCH_FILETIME: u64 = 11_644_473_600 * 10_000_000;
    if filetime < UNIX_EPOCH_FILETIME {
        return None;
    }
    let ticks = filetime - UNIX_EPOCH_FILETIME;
    Some(UNIX_EPOCH + Duration::new(ticks / 10_000_000, ((ticks % 10_000_000) * 100) as u32))
}

// ---------------------------------------------------------------------------
// 小工具（越界一律返回 None，不 panic）
// ---------------------------------------------------------------------------

fn u16_at(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn u64_at(data: &[u8], offset: usize) -> Option<u64> {
    let bytes = data.get(offset..offset + 8)?;
    Some(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在缓冲区末尾追加一条 DestList 记录（调用前缓冲区必须已经对齐到记录起点）
    fn append_entry(buf: &mut Vec<u8>, entry_id: u64, unix_secs: u64, path: &str) {
        buf.extend_from_slice(&entry_id.to_le_bytes());
        buf.extend_from_slice(&1.0f32.to_le_bytes());
        let filetime = (unix_secs + 11_644_473_600) * 10_000_000;
        buf.extend_from_slice(&filetime.to_le_bytes());
        buf.extend_from_slice(&(-1i32).to_le_bytes());
        buf.extend_from_slice(&26u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());

        let units: Vec<u16> = path.encode_utf16().collect();
        buf.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for unit in units {
            buf.extend_from_slice(&unit.to_le_bytes());
        }
    }

    /// 造一段 `DestList` 流：32 字节头 + 两条记录 + 填充
    fn sample_dest_list() -> Vec<u8> {
        let mut data = vec![0u8; 32];
        data[..4].copy_from_slice(&6u32.to_le_bytes()); // version
        append_entry(&mut data, 2657, 1_760_000_000, r"C:\Users\Test\Alpha");
        append_entry(&mut data, 2650, 1_759_000_000, r"D:\Projects\Beta");
        data.resize(4200, 0);
        data
    }

    fn write_dir_entry(file: &mut [u8], offset: usize, name: &str, kind: u8, start: u32, size: u64) {
        let units: Vec<u16> = name.encode_utf16().collect();
        for (i, unit) in units.iter().enumerate() {
            file[offset + i * 2..offset + i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
        let name_len = ((units.len() + 1) * 2) as u16;
        file[offset + 0x40..offset + 0x42].copy_from_slice(&name_len.to_le_bytes());
        file[offset + 0x42] = kind;
        file[offset + 0x74..offset + 0x78].copy_from_slice(&start.to_le_bytes());
        file[offset + 0x78..offset + 0x80].copy_from_slice(&size.to_le_bytes());
    }

    /// 造一个最小复合文档：扇区 0 = FAT、扇区 1 = 目录、扇区 2.. = 数据
    fn build_cfb(stream_name: &str, content: &[u8]) -> Vec<u8> {
        const SECTOR: usize = 512;
        let data_sectors = content.len().div_ceil(SECTOR);
        let mut file = vec![0u8; HEADER_LEN + (2 + data_sectors) * SECTOR];

        file[..8].copy_from_slice(&SIGNATURE);
        file[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes()); // 512 字节扇区
        file[0x20..0x22].copy_from_slice(&6u16.to_le_bytes()); // 64 字节 mini 扇区
        file[0x2C..0x30].copy_from_slice(&1u32.to_le_bytes()); // 1 个 FAT 扇区
        file[0x30..0x34].copy_from_slice(&1u32.to_le_bytes()); // 目录从扇区 1 开始
        file[0x38..0x3C].copy_from_slice(&4096u32.to_le_bytes()); // mini stream 阈值
        file[0x3C..0x40].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
        file[0x44..0x48].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
        file[0x4C..0x50].copy_from_slice(&0u32.to_le_bytes()); // DIFAT[0] → FAT 在扇区 0
        for i in 1..109 {
            let at = 0x4C + i * 4;
            file[at..at + 4].copy_from_slice(&FREE_SECT.to_le_bytes());
        }

        let fat = HEADER_LEN;
        let mut put = |index: usize, value: u32| {
            let at = fat + index * 4;
            file[at..at + 4].copy_from_slice(&value.to_le_bytes());
        };
        put(0, 0xFFFF_FFFD); // 自己就是 FAT
        put(1, END_OF_CHAIN); // 目录只占一个扇区
        for k in 0..data_sectors {
            let next = if k + 1 < data_sectors {
                (3 + k) as u32
            } else {
                END_OF_CHAIN
            };
            put(2 + k, next);
        }
        drop(put);

        let dir = HEADER_LEN + SECTOR;
        write_dir_entry(&mut file, dir, "Root Entry", 5, END_OF_CHAIN, 0);
        write_dir_entry(&mut file, dir + 128, stream_name, 2, 2, content.len() as u64);

        let data_offset = HEADER_LEN + 2 * SECTOR;
        file[data_offset..data_offset + content.len()].copy_from_slice(content);
        file
    }

    #[test]
    fn reads_named_stream_out_of_compound_file() {
        let stream = sample_dest_list();
        let cfb = build_cfb("DestList", &stream);
        let got = read_stream(&cfb, "DestList").expect("应当能取出流");
        assert_eq!(got, stream);

        // 不存在的流 → None
        assert!(read_stream(&cfb, "Nope").is_none());
        // 不是复合文档 → None
        assert!(read_stream(b"not a cfb file at all", "DestList").is_none());
    }

    #[test]
    fn parses_entries_in_file_order() {
        let entries = parse_dest_list(&sample_dest_list());
        assert_eq!(entries.len(), 2, "应当解出两条：{entries:?}");

        assert_eq!(entries[0].path, PathBuf::from(r"C:\Users\Test\Alpha"));
        assert_eq!(entries[0].entry_id, 2657);
        assert_eq!(entries[1].path, PathBuf::from(r"D:\Projects\Beta"));
        assert_eq!(entries[1].entry_id, 2650);

        let time = entries[0].last_access.expect("2026 年的时间应当能转出来");
        let secs = time
            .duration_since(UNIX_EPOCH)
            .expect("晚于 1970")
            .as_secs();
        assert_eq!(secs, 1_760_000_000);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_dest_list(&[]).is_empty());
        assert!(parse_dest_list(&[0u8; 128]).is_empty());
        // 这一串看起来像路径，但前缀字段（entry_id / weight / -1）对不上
        let mut fake = vec![0u8; 0x2A];
        fake.extend_from_slice(&20u16.to_le_bytes());
        for unit in r"C:\Somewhere\Else".encode_utf16() {
            fake.extend_from_slice(&unit.to_le_bytes());
        }
        assert!(parse_dest_list(&fake).is_empty());
    }

    #[test]
    fn recognises_paths_only() {
        assert!(looks_like_path(r"C:\Users"));
        assert!(looks_like_path("D:/Projects"));
        assert!(looks_like_path(r"\\wsl.localhost\Ubuntu\home"));
        assert!(!looks_like_path("wooyun"));
        assert!(!looks_like_path("https://example.com"));
        assert!(!looks_like_path("C:"));
    }

    #[test]
    fn filetime_before_1970_is_none() {
        assert!(filetime_to_time(0).is_none());
        assert!(filetime_to_time(11_644_473_600 * 10_000_000 - 1).is_none());
        let epoch = filetime_to_time(11_644_473_600 * 10_000_000).expect("正好是 1970");
        assert_eq!(epoch, UNIX_EPOCH);
    }
}
