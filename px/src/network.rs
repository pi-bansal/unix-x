use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Serialize, Clone)]
pub struct Connection {
    pub protocol: String,         // tcp, tcp6, udp, udp6
    pub local_addr: String,
    pub local_port: u16,
    pub remote_addr: Option<String>,
    pub remote_port: Option<u16>,
    pub state: Option<String>,    // LISTEN, ESTABLISHED, etc.
    pub pid: Option<u32>,
    pub process_name: Option<String>,
}

/// Read /proc/net/{tcp,tcp6,udp,udp6} and parse connections.
/// Falls back to empty vec on non-Linux systems.
pub fn collect_connections(pid_names: &HashMap<u32, String>) -> Vec<Connection> {
    let mut conns = Vec::new();

    for (proto, path) in &[
        ("tcp", "/proc/net/tcp"),
        ("tcp6", "/proc/net/tcp6"),
        ("udp", "/proc/net/udp"),
        ("udp6", "/proc/net/udp6"),
    ] {
        if let Ok(content) = fs::read_to_string(path) {
            // Build inode -> pid map from /proc/<pid>/fd
            let inode_pid = build_inode_pid_map();

            for line in content.lines().skip(1) {
                if let Some(conn) = parse_proc_net_line(line, proto, &inode_pid, pid_names) {
                    conns.push(conn);
                }
            }
        }
    }

    conns
}

/// On macOS/other, return placeholder noting unsupported platform.
pub fn platform_note() -> Option<String> {
    #[cfg(not(target_os = "linux"))]
    return Some("network inspection via /proc not available on this platform".to_string());
    #[cfg(target_os = "linux")]
    return None;
}

fn parse_proc_net_line(
    line: &str,
    proto: &str,
    inode_pid: &HashMap<u64, u32>,
    pid_names: &HashMap<u32, String>,
) -> Option<Connection> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 10 {
        return None;
    }

    let local = parse_addr(fields[1], proto.contains('6'))?;
    let remote = parse_addr(fields[2], proto.contains('6'));
    let state_hex = fields[3];
    let inode: u64 = fields[9].parse().ok()?;

    let state = if proto.starts_with("tcp") {
        Some(tcp_state(state_hex).to_string())
    } else {
        None
    };

    let pid = inode_pid.get(&inode).copied();
    let process_name = pid.and_then(|p| pid_names.get(&p)).cloned();

    let (remote_addr, remote_port) = match remote {
        Some((addr, port)) if addr != "0.0.0.0" && addr != "::" => {
            (Some(addr), Some(port))
        }
        _ => (None, None),
    };

    Some(Connection {
        protocol: proto.to_string(),
        local_addr: local.0,
        local_port: local.1,
        remote_addr,
        remote_port,
        state,
        pid,
        process_name,
    })
}

fn parse_addr(hex: &str, is_v6: bool) -> Option<(String, u16)> {
    let parts: Vec<&str> = hex.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let port = u16::from_str_radix(parts[1], 16).ok()?;

    let addr = if is_v6 {
        let raw = u128::from_str_radix(parts[0], 16).ok()?;
        // Linux stores IPv6 in 4 little-endian u32 words
        let a = (raw & 0xFFFFFFFF) as u32;
        let b = ((raw >> 32) & 0xFFFFFFFF) as u32;
        let c = ((raw >> 64) & 0xFFFFFFFF) as u32;
        let d = ((raw >> 96) & 0xFFFFFFFF) as u32;
        IpAddr::V6(Ipv6Addr::from(
            u128::from(a.swap_bytes()) << 96
                | u128::from(b.swap_bytes()) << 64
                | u128::from(c.swap_bytes()) << 32
                | u128::from(d.swap_bytes()),
        ))
        .to_string()
    } else {
        let raw = u32::from_str_radix(parts[0], 16).ok()?;
        IpAddr::V4(Ipv4Addr::from(raw.swap_bytes())).to_string()
    };

    Some((addr, port))
}

fn tcp_state(hex: &str) -> &'static str {
    match hex {
        "01" => "ESTABLISHED",
        "02" => "SYN_SENT",
        "03" => "SYN_RECV",
        "04" => "FIN_WAIT1",
        "05" => "FIN_WAIT2",
        "06" => "TIME_WAIT",
        "07" => "CLOSE",
        "08" => "CLOSE_WAIT",
        "09" => "LAST_ACK",
        "0A" => "LISTEN",
        "0B" => "CLOSING",
        _ => "UNKNOWN",
    }
}

fn build_inode_pid_map() -> HashMap<u64, u32> {
    let mut map = HashMap::new();
    if let Ok(proc_dir) = fs::read_dir("/proc") {
        for entry in proc_dir.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Ok(pid) = name_str.parse::<u32>() {
                let fd_path = format!("/proc/{}/fd", pid);
                if let Ok(fds) = fs::read_dir(&fd_path) {
                    for fd in fds.flatten() {
                        if let Ok(link) = fs::read_link(fd.path()) {
                            let link_str = link.to_string_lossy();
                            if let Some(inode_str) = link_str.strip_prefix("socket:[") {
                                if let Some(inode_str) = inode_str.strip_suffix(']') {
                                    if let Ok(inode) = inode_str.parse::<u64>() {
                                        map.insert(inode, pid);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    map
}
