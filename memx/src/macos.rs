/// macOS memory inspection via `vmmap -wide <pid>`
///
/// vmmap output columns:
///   Region Type    Start-End    [ Vsize  Rsize  Dirty  Swap ]  perms  Details
///
/// We parse the key region types:
///   MALLOC_*     → heap allocations
///   Stack        → thread stacks
///   __TEXT       → code segments (from dylibs or exe)
///   __DATA       → data segments
///   mapped file  → file-backed mappings

use serde::Serialize;
use std::process::Command;

#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum RegionKind {
    Heap,
    Stack,
    Text,
    Data,
    MappedFile,
    Anon,
    Other,
}

#[derive(Serialize, Clone)]
pub struct MemRegion {
    pub start_addr:   String,
    pub end_addr:     String,
    pub size_bytes:   u64,
    pub kind:         RegionKind,
    pub permissions:  String,
    pub label:        String,
    pub dirty_bytes:  u64,
    pub swap_bytes:   u64,
}

#[derive(Serialize)]
pub struct ProcessMemory {
    pub pid:           u32,
    pub exe:           Option<String>,
    pub total_vss:     u64,
    pub total_rss:     u64,
    pub total_dirty:   u64,
    pub total_swap:    u64,
    pub heap_bytes:    u64,
    pub stack_bytes:   u64,
    pub text_bytes:    u64,
    pub region_count:  usize,
    pub regions_truncated: bool,
    pub regions:       Vec<MemRegion>,
}

pub fn read_process_memory(pid: u32, include_regions: bool) -> Result<ProcessMemory, String> {
    let out = Command::new("vmmap")
        .args(["-wide", &pid.to_string()])
        .output()
        .map_err(|e| format!("vmmap failed: {}", e))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("vmmap error: {}", stderr.trim()));
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let regions = parse_vmmap(&text);

    let total_vss:   u64 = regions.iter().map(|r| r.size_bytes).sum();
    let total_dirty: u64 = regions.iter().map(|r| r.dirty_bytes).sum();
    let total_swap:  u64 = regions.iter().map(|r| r.swap_bytes).sum();
    let heap_bytes:  u64 = regions.iter().filter(|r| r.kind == RegionKind::Heap).map(|r| r.size_bytes).sum();
    let stack_bytes: u64 = regions.iter().filter(|r| r.kind == RegionKind::Stack).map(|r| r.size_bytes).sum();
    let text_bytes:  u64 = regions.iter().filter(|r| r.kind == RegionKind::Text).map(|r| r.size_bytes).sum();
    let region_count = regions.len();

    // vmmap doesn't directly give RSS — use dirty as a proxy
    let total_rss = total_dirty;

    // Try to get exe name from ps
    let exe = get_exe(pid);

    Ok(ProcessMemory {
        pid,
        exe,
        total_vss,
        total_rss,
        total_dirty,
        total_swap,
        heap_bytes,
        stack_bytes,
        text_bytes,
        region_count,
        regions_truncated: false,
        regions: if include_regions { regions } else { vec![] },
    })
}

fn parse_vmmap(text: &str) -> Vec<MemRegion> {
    let mut regions = Vec::new();
    let mut in_regions = false;

    for line in text.lines() {
        let line = line.trim();

        if line.starts_with("==== Non-writable") || line.starts_with("==== Writable") {
            in_regions = true;
            continue;
        }

        if !in_regions || line.is_empty() || line.starts_with("==") {
            continue;
        }

        if let Some(region) = parse_vmmap_line(line) {
            regions.push(region);
        }
    }

    regions
}

fn parse_vmmap_line(line: &str) -> Option<MemRegion> {
    // Format: LABEL   START-END  [ VSIZE  RSIZE  DIRTY  SWAP ]  perms  ...
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 6 { return None; }

    let label = parts[0].to_string();

    // Find the addr-addr part
    let addr_part = parts.iter().find(|p| p.contains('-') && p.len() > 8)?;
    let addrs: Vec<&str> = addr_part.split('-').collect();
    if addrs.len() != 2 { return None; }

    let start = format!("0x{}", addrs[0]);
    let end   = format!("0x{}", addrs[1]);

    let start_val = u64::from_str_radix(addrs[0].trim_start_matches("0x"), 16).ok()?;
    let end_val   = u64::from_str_radix(addrs[1].trim_start_matches("0x"), 16).ok()?;
    let size = end_val.saturating_sub(start_val);

    // Extract numbers from [ vsize rsize dirty swap ] block
    let bracket_content: Option<&str> = {
        let open = line.find('[')?;
        let close = line.find(']')?;
        Some(&line[open+1..close])
    };

    let nums: Vec<u64> = bracket_content
        .unwrap_or("")
        .split_whitespace()
        .filter_map(|s| parse_size(s))
        .collect();

    let dirty = nums.get(2).copied().unwrap_or(0);
    let swap  = nums.get(3).copied().unwrap_or(0);

    // Permissions: look for r/w/x/- pattern
    let perms = parts.iter()
        .find(|p| p.len() == 4 && p.chars().all(|c| "rwx-s".contains(c)))
        .copied()
        .unwrap_or("----")
        .to_string();

    let kind = classify_vmmap(&label);

    Some(MemRegion {
        start_addr: start,
        end_addr: end,
        size_bytes: size,
        kind,
        permissions: perms,
        label,
        dirty_bytes: dirty,
        swap_bytes: swap,
    })
}

fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.ends_with('K') {
        s.trim_end_matches('K').parse::<u64>().ok().map(|n| n * 1024)
    } else if s.ends_with('M') {
        s.trim_end_matches('M').parse::<u64>().ok().map(|n| n * 1024 * 1024)
    } else if s.ends_with('G') {
        s.trim_end_matches('G').parse::<u64>().ok().map(|n| n * 1024 * 1024 * 1024)
    } else {
        s.parse().ok()
    }
}

fn classify_vmmap(label: &str) -> RegionKind {
    let l = label.to_uppercase();
    if l.starts_with("MALLOC") || l.contains("HEAP") {
        RegionKind::Heap
    } else if l.contains("STACK") {
        RegionKind::Stack
    } else if l.contains("__TEXT") || l.contains("TEXT") {
        RegionKind::Text
    } else if l.contains("__DATA") || l.contains("DATA") {
        RegionKind::Data
    } else if l.contains("MAPPED") || l.contains("FILE") {
        RegionKind::MappedFile
    } else if l.contains("ANON") {
        RegionKind::Anon
    } else {
        RegionKind::Other
    }
}

fn get_exe(pid: u32) -> Option<String> {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}
