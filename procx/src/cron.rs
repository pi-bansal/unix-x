use regex::Regex;
use serde::Serialize;
use std::fs;
use std::process::Command;

#[derive(Serialize, Clone)]
pub struct CronJob {
    pub source: String,        // "user:root", "system", "/etc/cron.d/foo"
    pub schedule: String,      // raw cron expression
    pub schedule_human: String, // English description
    pub command: String,
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<String>>,
}

pub fn collect_cron() -> Vec<CronJob> {
    let mut jobs = Vec::new();

    // User crontabs via `crontab -l`
    if let Ok(out) = Command::new("crontab").arg("-l").output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            let user = whoami();
            for job in parse_crontab(&text, &format!("user:{}", user)) {
                jobs.push(job);
            }
        }
    }

    // /etc/crontab
    if let Ok(text) = fs::read_to_string("/etc/crontab") {
        for job in parse_crontab_system(&text, "/etc/crontab") {
            jobs.push(job);
        }
    }

    // /etc/cron.d/*
    if let Ok(entries) = fs::read_dir("/etc/cron.d") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Ok(text) = fs::read_to_string(&path) {
                    let label = path.to_string_lossy().to_string();
                    for job in parse_crontab_system(&text, &label) {
                        jobs.push(job);
                    }
                }
            }
        }
    }

    // /etc/cron.{hourly,daily,weekly,monthly}
    for (dir, schedule, human) in &[
        ("/etc/cron.hourly", "0 * * * *", "every hour"),
        ("/etc/cron.daily", "0 0 * * *", "daily"),
        ("/etc/cron.weekly", "0 0 * * 0", "weekly"),
        ("/etc/cron.monthly", "0 0 1 * *", "monthly"),
    ] {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    jobs.push(CronJob {
                        source: dir.to_string(),
                        schedule: schedule.to_string(),
                        schedule_human: human.to_string(),
                        command: path.to_string_lossy().to_string(),
                        user: Some("root".to_string()),
                        env: None,
                    });
                }
            }
        }
    }

    jobs
}

fn parse_crontab(text: &str, source: &str) -> Vec<CronJob> {
    let mut jobs = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() || line.starts_with('@') {
            continue;
        }
        let parts: Vec<&str> = line.splitn(6, ' ').collect();
        if parts.len() >= 6 {
            let schedule = parts[..5].join(" ");
            let command = parts[5..].join(" ");
            let human = describe_schedule(&schedule);
            jobs.push(CronJob {
                source: source.to_string(),
                schedule,
                schedule_human: human,
                command,
                user: None,
                env: None,
            });
        }
    }
    jobs
}

fn parse_crontab_system(text: &str, source: &str) -> Vec<CronJob> {
    let mut jobs = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(7, ' ').collect();
        if parts.len() >= 7 {
            let schedule = parts[..5].join(" ");
            let user = parts[5].to_string();
            let command = parts[6..].join(" ");
            let human = describe_schedule(&schedule);
            jobs.push(CronJob {
                source: source.to_string(),
                schedule,
                schedule_human: human,
                command,
                user: Some(user),
                env: None,
            });
        }
    }
    jobs
}

pub fn describe_schedule(expr: &str) -> String {
    // Common patterns → human readable
    match expr {
        "* * * * *"   => return "every minute".to_string(),
        "0 * * * *"   => return "every hour".to_string(),
        "0 0 * * *"   => return "daily at midnight".to_string(),
        "0 0 * * 0"   => return "weekly on Sunday".to_string(),
        "0 0 1 * *"   => return "monthly on the 1st".to_string(),
        "0 0 1 1 *"   => return "yearly on Jan 1".to_string(),
        "@reboot"     => return "at reboot".to_string(),
        "@hourly"     => return "every hour".to_string(),
        "@daily"      => return "daily".to_string(),
        "@weekly"     => return "weekly".to_string(),
        "@monthly"    => return "monthly".to_string(),
        "@yearly"     => return "yearly".to_string(),
        _ => {}
    }

    let parts: Vec<&str> = expr.split(' ').collect();
    if parts.len() != 5 {
        return expr.to_string();
    }

    format!(
        "min={} hour={} dom={} month={} dow={}",
        parts[0], parts[1], parts[2], parts[3], parts[4]
    )
}

fn whoami() -> String {
    Command::new("whoami")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}
