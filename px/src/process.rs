use serde::Serialize;
use sysinfo::{Pid, Process, ProcessStatus, System};

#[derive(Serialize)]
pub struct ProcessEntry {
    pub pid: u32,
    pub ppid: Option<u32>,
    pub name: String,
    pub exe: Option<String>,
    pub cmd: Vec<String>,
    pub status: String,
    pub cpu_percent: f32,     // 0.0–100.0
    pub memory_bytes: u64,
    pub virtual_memory_bytes: u64,
    pub started_at: u64,      // unix epoch seconds
    pub run_time_secs: u64,
    pub threads: Option<u32>,
    pub user: Option<String>,
    pub cwd: Option<String>,
    pub env: Option<Vec<String>>, // only with --env flag
}

pub fn collect_processes(sys: &System, include_env: bool) -> Vec<ProcessEntry> {
    let mut entries: Vec<ProcessEntry> = sys
        .processes()
        .values()
        .map(|p| process_entry(p, sys, include_env))
        .collect();

    // Sort by CPU desc, then memory desc
    entries.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.memory_bytes.cmp(&a.memory_bytes))
    });

    entries
}

fn process_entry(p: &Process, sys: &System, include_env: bool) -> ProcessEntry {
    let ppid = p.parent().map(|pid| pid.as_u32());

    let status = match p.status() {
        ProcessStatus::Run => "running",
        ProcessStatus::Sleep => "sleeping",
        ProcessStatus::Idle => "idle",
        ProcessStatus::Zombie => "zombie",
        ProcessStatus::Stop => "stopped",
        ProcessStatus::Dead => "dead",
        _ => "unknown",
    }
    .to_string();

    let user = p.user_id().and_then(|uid| {
        sys.get_user_by_id(uid)
            .map(|u| u.name().to_string())
    });

    let env = if include_env {
        Some(p.environ().iter().map(|e| e.to_string_lossy().to_string()).collect())
    } else {
        None
    };

    ProcessEntry {
        pid: p.pid().as_u32(),
        ppid,
        name: p.name().to_string_lossy().to_string(),
        exe: p.exe().map(|e| e.to_string_lossy().to_string()),
        cmd: p.cmd().iter().map(|c| c.to_string_lossy().to_string()).collect(),
        status,
        cpu_percent: p.cpu_usage(),
        memory_bytes: p.memory(),
        virtual_memory_bytes: p.virtual_memory(),
        started_at: p.start_time(),
        run_time_secs: p.run_time(),
        threads: None, // sysinfo doesn't expose this portably
        user,
        cwd: p.cwd().map(|c| c.to_string_lossy().to_string()),
        env,
    }
}
