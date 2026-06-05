use plist::Value;
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Serialize, Clone)]
pub struct LaunchdJob {
    pub label: String,
    pub program: Option<String>,
    pub program_args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,     // StartCalendarInterval description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_at_load: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<bool>,
    pub domain: String,               // system, user, user-agent
    pub plist_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_exit_status: Option<i32>,
}

pub fn collect_launchd() -> Vec<LaunchdJob> {
    let mut jobs = Vec::new();

    // Plist search paths
    let paths: &[(&str, &str)] = &[
        ("/Library/LaunchDaemons", "system"),
        ("/Library/LaunchAgents", "user"),
        ("/System/Library/LaunchDaemons", "system"),
        ("/System/Library/LaunchAgents", "system"),
    ];

    // Also user-specific
    let home = std::env::var("HOME").unwrap_or_default();
    let user_agents = format!("{}/Library/LaunchAgents", home);

    let mut all_paths: Vec<(String, &str)> = paths
        .iter()
        .map(|(p, d)| (p.to_string(), *d))
        .collect();
    all_paths.push((user_agents, "user-agent"));

    // Build running job map from launchctl list
    let running = running_jobs();

    for (dir, domain) in &all_paths {
        let Ok(entries) = fs::read_dir(dir) else { continue };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e != "plist").unwrap_or(true) {
                continue;
            }

            if let Some(job) = parse_plist(&path, domain, &running) {
                jobs.push(job);
            }
        }
    }

    jobs
}

fn parse_plist(
    path: &Path,
    domain: &str,
    running: &std::collections::HashMap<String, (Option<u32>, Option<i32>)>,
) -> Option<LaunchdJob> {
    let val = Value::from_file(path).ok()?;
    let dict = val.as_dictionary()?;

    let label = dict.get("Label")?.as_string()?.to_string();

    let program = dict
        .get("Program")
        .and_then(|v| v.as_string())
        .map(|s| s.to_string());

    let program_args: Vec<String> = dict
        .get("ProgramArguments")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_string().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let run_at_load = dict
        .get("RunAtLoad")
        .and_then(|v| v.as_boolean());

    let keep_alive = dict
        .get("KeepAlive")
        .and_then(|v| v.as_boolean());

    let schedule = dict
        .get("StartCalendarInterval")
        .map(|v| describe_calendar_interval(v));

    let (pid, last_exit_status) = running
        .get(&label)
        .cloned()
        .unwrap_or((None, None));

    Some(LaunchdJob {
        label,
        program,
        program_args,
        schedule,
        run_at_load,
        keep_alive,
        domain: domain.to_string(),
        plist_path: path.to_string_lossy().to_string(),
        pid,
        last_exit_status,
    })
}

/// Parse `launchctl list` into label -> (pid, last_exit_status)
fn running_jobs() -> std::collections::HashMap<String, (Option<u32>, Option<i32>)> {
    let mut map = std::collections::HashMap::new();

    let Ok(out) = Command::new("launchctl").arg("list").output() else {
        return map;
    };

    for line in String::from_utf8_lossy(&out.stdout).lines().skip(1) {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let pid: Option<u32> = parts[0].parse().ok();
        let exit: Option<i32> = parts[1].parse().ok();
        let label = parts[2].to_string();
        map.insert(label, (pid, exit));
    }

    map
}

fn describe_calendar_interval(val: &Value) -> String {
    // Can be a dict or array of dicts
    match val {
        Value::Dictionary(dict) => describe_interval_dict(dict),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_dictionary().map(describe_interval_dict))
            .collect::<Vec<_>>()
            .join(", "),
        _ => "custom schedule".to_string(),
    }
}

fn describe_interval_dict(
    dict: &plist::dictionary::Dictionary,
) -> String {
    let minute = dict.get("Minute").and_then(|v| v.as_signed_integer());
    let hour   = dict.get("Hour").and_then(|v| v.as_signed_integer());
    let day    = dict.get("Day").and_then(|v| v.as_signed_integer());
    let weekday= dict.get("Weekday").and_then(|v| v.as_signed_integer());
    let month  = dict.get("Month").and_then(|v| v.as_signed_integer());

    match (hour, minute, weekday, day, month) {
        (Some(h), Some(m), None, None, None) => format!("daily at {:02}:{:02}", h, m),
        (Some(h), Some(m), Some(w), None, None) => {
            let dow = ["Sun","Mon","Tue","Wed","Thu","Fri","Sat"]
                .get(w as usize).unwrap_or(&"?");
            format!("weekly on {} at {:02}:{:02}", dow, h, m)
        }
        (Some(h), Some(m), None, Some(d), None) => format!("monthly on day {} at {:02}:{:02}", d, h, m),
        (None, Some(m), None, None, None) => format!("every hour at minute {}", m),
        _ => format!("custom: {:?}", dict.keys().collect::<Vec<_>>()),
    }
}
