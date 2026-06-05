/// Linux memory inspection via /proc/<pid>/smaps
///
/// smaps gives per-region breakdown of:
///   - RSS (resident set — actually in RAM)
///   - PSS (proportional set — RSS divided by sharing count, most accurate)
///   - Private_Clean / Private_Dirty — pages not shared with any other process
///   - Shared_Clean / Shared_Dirty — shared with others
///   - Referenced — recently accessed
///   - Swap — pages swapped out
///
/// We categorize regions by type:
///   heap     — [heap] mapping
///   stack    — [stack] mapping
///   lib      — path ends in .so
///   exe      — first file-backed mapping matching exe name
///   anon     — no backing file, not heap/stack (likely mmap'd allocations)
///   vdso     — [vdso] / [vsyscall]
///   file     — other file-backed mappings

use serde::Serialize;
use std::fs;

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RegionKind {
    Heap,
    Stack,
    Lib,
    Exe,
    Anon,
    Vdso,
    File,
    Other,
}

#[derive(Serialize, Clone)]
pub struct MemRegion {
    pub start_addr:    String,   // hex
    pub end_addr:      String,   // hex
    pub size_bytes:    u64,
    pub kind:          RegionKind,
    pub permissions:   String,   // rwxp
    pub backing_file:  Option<String>,
    pub rss_bytes:     u64,
    pub pss_bytes:     u64,
    pub private_bytes: u64,      // private_clean + private_dirty
    pub shared_bytes:  u64,      // shared_clean + shared_dirty
    pub swap_bytes:    u64,
    pub referenced_bytes: u64,
}

#[derive(Serialize)]
pub struct ProcessMemory {
    pub pid:            u32,
    pub exe:            Option<String>,
    pub total_vss:      u64,   // virtual address space
    pub total_rss:      u64,   // resident (in RAM)
    pub total_pss:      u64,   // proportional (most accurate for "how much RAM does this process really use")
    pub total_private:  u64,
    pub total_shared:   u64,
    pub total_swap:     u64,
    pub heap_bytes:     u64,
    pub stack_bytes:    u64,
    pub lib_bytes:      u64,   // total PSS from shared libraries
    pub anon_bytes:     u64,   // anonymous mappings (likely custom allocators / mmap)
    pub region_count:   usize,
    pub regions:        Vec<MemRegion>,
}

pub fn read_process_memory(pid: u32, include_regions: bool) -> Result<ProcessMemory, String> {
    let smaps_path = format!("/proc/{}/smaps", pid);
    let content = fs::read_to_string(&smaps_path)
        .map_err(|e| format!("cannot read {}: {} (is process running? do you have permission?)", smaps_path, e))?;

    let exe = fs::read_link(format!("/proc/{}/exe", pid))
        .ok()
        .map(|p| p.to_string_lossy().to_string());

    let regions = parse_smaps(&content, exe.as_deref());

    let total_vss:     u64 = regions.iter().map(|r| r.size_bytes).sum();
    let total_rss:     u64 = regions.iter().map(|r| r.rss_bytes).sum();
    let total_pss:     u64 = regions.iter().map(|r| r.pss_bytes).sum();
    let total_private: u64 = regions.iter().map(|r| r.private_bytes).sum();
    let total_shared:  u64 = regions.iter().map(|r| r.shared_bytes).sum();
    let total_swap:    u64 = regions.iter().map(|r| r.swap_bytes).sum();
    let heap_bytes:    u64 = regions.iter().filter(|r| r.kind == RegionKind::Heap).map(|r| r.rss_bytes).sum();
    let stack_bytes:   u64 = regions.iter().filter(|r| r.kind == RegionKind::Stack).map(|r| r.rss_bytes).sum();
    let lib_bytes:     u64 = regions.iter().filter(|r| r.kind == RegionKind::Lib).map(|r| r.pss_bytes).sum();
    let anon_bytes:    u64 = regions.iter().filter(|r| r.kind == RegionKind::Anon).map(|r| r.rss_bytes).sum();
    let region_count = regions.len();

    Ok(ProcessMemory {
        pid,
        exe,
        total_vss,
        total_rss,
        total_pss,
        total_private,
        total_shared,
        total_swap,
        heap_bytes,
        stack_bytes,
        lib_bytes,
        anon_bytes,
        region_count,
        regions: if include_regions { regions } else { vec![] },
    })
}

fn parse_smaps(content: &str, exe: Option<&str>) -> Vec<MemRegion> {
    let mut regions = Vec::new();
    let mut current: Option<PartialRegion> = None;

    for line in content.lines() {
        // Header line: "addr-addr perms offset dev inode [pathname]"
        if line.contains('-') && !line.starts_with(' ') && !line.contains(':') {
            if let Some(r) = current.take() {
                regions.push(finalize(r, exe));
            }
            current = parse_header(line);
        } else if let Some(ref mut r) = current {
            parse_field(line, r);
        }
    }

    if let Some(r) = current {
        regions.push(finalize(r, exe));
    }

    regions
}

#[derive(Default)]
struct PartialRegion {
    start:       String,
    end:         String,
    size:        u64,
    perms:       String,
    path:        Option<String>,
    rss:         u64,
    pss:         u64,
    priv_clean:  u64,
    priv_dirty:  u64,
    shar_clean:  u64,
    shar_dirty:  u64,
    swap:        u64,
    referenced:  u64,
}

fn parse_header(line: &str) -> Option<PartialRegion> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() { return None; }

    let addrs: Vec<&str> = parts[0].split('-').collect();
    if addrs.len() != 2 { return None; }

    let start_val = u64::from_str_radix(addrs[0], 16).ok()?;
    let end_val   = u64::from_str_radix(addrs[1], 16).ok()?;

    Some(PartialRegion {
        start: format!("0x{}", addrs[0]),
        end:   format!("0x{}", addrs[1]),
        size:  end_val.saturating_sub(start_val),
        perms: parts.get(1).copied().unwrap_or("").to_string(),
        path:  parts.get(5).map(|s| s.to_string()),
        ..Default::default()
    })
}

fn parse_field(line: &str, r: &mut PartialRegion) {
    let kv: Vec<&str> = line.splitn(2, ':').collect();
    if kv.len() != 2 { return; }
    let key = kv[0].trim();
    let val: u64 = kv[1].trim().split_whitespace().next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let val_bytes = val * 1024; // smaps reports in kB

    match key {
        "Rss"           => r.rss        = val_bytes,
        "Pss"           => r.pss        = val_bytes,
        "Private_Clean" => r.priv_clean = val_bytes,
        "Private_Dirty" => r.priv_dirty = val_bytes,
        "Shared_Clean"  => r.shar_clean = val_bytes,
        "Shared_Dirty"  => r.shar_dirty = val_bytes,
        "Swap"          => r.swap       = val_bytes,
        "Referenced"    => r.referenced = val_bytes,
        _ => {}
    }
}

fn finalize(r: PartialRegion, exe: Option<&str>) -> MemRegion {
    let kind = classify(&r.path, exe);
    MemRegion {
        start_addr:      r.start,
        end_addr:        r.end,
        size_bytes:      r.size,
        kind,
        permissions:     r.perms,
        backing_file:    r.path,
        rss_bytes:       r.rss,
        pss_bytes:       r.pss,
        private_bytes:   r.priv_clean + r.priv_dirty,
        shared_bytes:    r.shar_clean + r.shar_dirty,
        swap_bytes:      r.swap,
        referenced_bytes: r.referenced,
    }
}

fn classify(path: &Option<String>, exe: Option<&str>) -> RegionKind {
    match path.as_deref() {
        Some("[heap]")               => RegionKind::Heap,
        Some("[stack]") | Some("[stack:main]") => RegionKind::Stack,
        Some("[vdso]") | Some("[vsyscall]") | Some("[vvar]") => RegionKind::Vdso,
        Some(p) if p.ends_with(".so") || p.contains(".so.") => RegionKind::Lib,
        Some(p) if exe.map(|e| e == p).unwrap_or(false) => RegionKind::Exe,
        Some(_) => RegionKind::File,
        None    => RegionKind::Anon,
    }
}
