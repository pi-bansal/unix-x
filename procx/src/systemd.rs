use serde::Serialize;
use std::process::Command;

#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum UnitState {
    Active,
    Inactive,
    Failed,
    Activating,
    Deactivating,
    Unknown,
}

#[derive(Serialize, Clone)]
pub struct SystemdUnit {
    pub name: String,
    pub description: String,
    pub load_state: String,
    pub active_state: UnitState,
    pub sub_state: String,
    pub unit_type: String,     // service, timer, socket, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timer_schedule: Option<String>,   // for .timer units
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_trigger: Option<String>,     // for .timer units
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_trigger: Option<String>,     // for .timer units
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
}

pub fn collect_systemd() -> Vec<SystemdUnit> {
    let mut units = Vec::new();

    // List all units
    let out = Command::new("systemctl")
        .args(["list-units", "--all", "--no-pager", "--no-legend", "--plain"])
        .output();

    let unit_out = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return units, // systemd not available
    };

    for line in unit_out.lines() {
        let parts: Vec<&str> = line.splitn(5, ' ')
            .filter(|s| !s.is_empty())
            .collect();

        if parts.len() < 4 {
            continue;
        }

        let name = parts[0].to_string();
        let load_state = parts[1].to_string();
        let active_raw = parts[2];
        let sub_state = parts[3].to_string();
        let description = parts.get(4).unwrap_or(&"").to_string();

        let active_state = match active_raw {
            "active"      => UnitState::Active,
            "inactive"    => UnitState::Inactive,
            "failed"      => UnitState::Failed,
            "activating"  => UnitState::Activating,
            "deactivating"=> UnitState::Deactivating,
            _             => UnitState::Unknown,
        };

        let unit_type = name
            .rsplit_once('.')
            .map(|(_, t)| t.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        // For timers, get schedule details
        let (timer_schedule, last_trigger, next_trigger) = if unit_type == "timer" {
            fetch_timer_details(&name)
        } else {
            (None, None, None)
        };

        // For services, get exec and memory
        let (exec_start, pid, memory_bytes) = if unit_type == "service" {
            fetch_service_details(&name)
        } else {
            (None, None, None)
        };

        units.push(SystemdUnit {
            name,
            description,
            load_state,
            active_state,
            sub_state,
            unit_type,
            timer_schedule,
            last_trigger,
            next_trigger,
            exec_start,
            pid,
            memory_bytes,
        });
    }

    units
}

fn fetch_timer_details(name: &str) -> (Option<String>, Option<String>, Option<String>) {
    let out = Command::new("systemctl")
        .args(["show", name, "--no-pager",
               "--property=OnCalendar,LastTriggerUSec,NextElapseUSecRealtime"])
        .output();

    let text = match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => return (None, None, None),
    };

    let mut schedule = None;
    let mut last = None;
    let mut next = None;

    for line in text.lines() {
        if let Some(val) = line.strip_prefix("OnCalendar=") {
            if !val.is_empty() { schedule = Some(val.to_string()); }
        }
        if let Some(val) = line.strip_prefix("LastTriggerUSec=") {
            if !val.is_empty() && val != "0" { last = Some(val.to_string()); }
        }
        if let Some(val) = line.strip_prefix("NextElapseUSecRealtime=") {
            if !val.is_empty() && val != "0" { next = Some(val.to_string()); }
        }
    }

    (schedule, last, next)
}

fn fetch_service_details(name: &str) -> (Option<String>, Option<u32>, Option<u64>) {
    let out = Command::new("systemctl")
        .args(["show", name, "--no-pager",
               "--property=ExecStart,MainPID,MemoryCurrent"])
        .output();

    let text = match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => return (None, None, None),
    };

    let mut exec = None;
    let mut pid = None;
    let mut mem = None;

    for line in text.lines() {
        if let Some(val) = line.strip_prefix("ExecStart=") {
            // ExecStart={ path=... argv[]= ... }
            if let Some(path) = val.split("path=").nth(1) {
                let path = path.split(';').next().unwrap_or("").trim().trim_end_matches('}').trim();
                if !path.is_empty() { exec = Some(path.to_string()); }
            }
        }
        if let Some(val) = line.strip_prefix("MainPID=") {
            pid = val.parse().ok().filter(|&p: &u32| p > 0);
        }
        if let Some(val) = line.strip_prefix("MemoryCurrent=") {
            mem = val.parse().ok().filter(|&m: &u64| m != u64::MAX);
        }
    }

    (exec, pid, mem)
}
