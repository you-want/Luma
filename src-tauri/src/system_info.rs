// 系统状态 (System Status): a compact CPU / memory / battery snapshot.
// Deliberately minimal:
// one snapshot per call (no background watch), parsed from the platforms'
// own tools so no new dependency is needed. Pure parsers are unit-tested;
// tool failures degrade to `None` per field instead of failing the card.

use serde::Serialize;
use std::process::Command;

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryStatus {
    pub percent: u32,
    /// Plugged in (charging or charged).
    pub charging: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    pub cpu_usage_percent: Option<f32>,
    pub memory_total_bytes: Option<u64>,
    pub memory_used_bytes: Option<u64>,
    pub battery: Option<BatteryStatus>,
    pub platform_supported: bool,
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub fn system_status() -> SystemStatus {
    #[cfg(target_os = "macos")]
    {
        SystemStatus {
            cpu_usage_percent: macos_cpu_usage(),
            memory_total_bytes: macos_memory().map(|(total, _)| total),
            memory_used_bytes: macos_memory().and_then(|(_, used)| used),
            battery: macos_battery(),
            platform_supported: true,
        }
    }

    #[cfg(target_os = "windows")]
    {
        SystemStatus {
            cpu_usage_percent: windows_cpu_usage(),
            memory_total_bytes: windows_memory().map(|(total, _)| total),
            memory_used_bytes: windows_memory().and_then(|(_, used)| used),
            battery: windows_battery(),
            platform_supported: true,
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        SystemStatus {
            cpu_usage_percent: None,
            memory_total_bytes: None,
            memory_used_bytes: None,
            battery: None,
            platform_supported: false,
        }
    }
}

// ── macOS ────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn macos_cpu_usage() -> Option<f32> {
    let output = Command::new("top")
        .args(["-l", "1", "-n", "0"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_top_cpu(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "macos")]
fn macos_memory() -> Option<(u64, Option<u64>)> {
    let output = Command::new("top")
        .args(["-l", "1", "-n", "0"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_top_memory(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "macos")]
fn macos_battery() -> Option<BatteryStatus> {
    let output = Command::new("pmset").arg("-g").arg("batt").output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_pmset_battery(&String::from_utf8_lossy(&output.stdout))
}

/// "CPU usage: 7.50% user, 4.16% sys, 88.33% idle" → user + sys.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_top_cpu(text: &str) -> Option<f32> {
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("CPU usage:") else {
            continue;
        };
        let mut usage = 0.0f32;
        let mut found = false;
        for part in rest.split(',') {
            let part = part.trim();
            let Some(percent) = part
                .strip_suffix("% user")
                .or_else(|| part.strip_suffix("% sys"))
            else {
                continue;
            };
            if let Ok(value) = percent.trim().parse::<f32>() {
                usage += value;
                found = true;
            }
        }
        if found {
            return Some(usage.clamp(0.0, 100.0));
        }
    }
    None
}

/// Size suffix (K/M/G/T, case-insensitive) → bytes.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_size_with_suffix(text: &str) -> Option<u64> {
    let text = text.trim();
    let split_at = text
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(text.len());
    let (number, suffix) = text.split_at(split_at);
    let value = number.parse::<f64>().ok()?;
    let multiplier = match suffix.to_ascii_lowercase().as_str() {
        "k" => 1024.0,
        "m" => 1024.0 * 1024.0,
        "g" => 1024.0 * 1024.0 * 1024.0,
        "t" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        "" => 1.0,
        _ => return None,
    };
    Some((value * multiplier) as u64)
}

/// "PhysMem: 12G used (3G wired, 0K purgeable), 5G unused." → (total, used)
/// where total = used + unused.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_top_memory(text: &str) -> Option<(u64, Option<u64>)> {
    let line = text
        .lines()
        .find(|line| line.trim().starts_with("PhysMem:"))?;
    let rest = line.trim().strip_prefix("PhysMem:")?.trim();
    // "12G used (…)" — the value directly before the first " used".
    let used = parse_size_with_suffix(rest.split(" used").next()?.trim())?;
    // "…, 5G unused." — the trailing segment after the last comma.
    let unused = rest
        .rsplit(", ")
        .find_map(|part| part.trim_end_matches('.').trim().strip_suffix(" unused"))
        .and_then(parse_size_with_suffix);
    Some((used.saturating_add(unused.unwrap_or(0)), Some(used)))
}

/// pmset lines like:
/// "-InternalBattery-0 (id=…)  98%; discharging; 0:00 remaining present: true"
/// "Now drawing from 'AC Power'" + "100%; charged; …"
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_pmset_battery(text: &str) -> Option<BatteryStatus> {
    let mut percent = None;
    let mut charging = false;
    for line in text.lines() {
        if line.contains("AC Power") {
            charging = true;
        }
        for part in line.split(';') {
            let part = part.trim();
            // The percent is the last whitespace token of its segment
            // ("…id=11409375)\t98%", "100%", …).
            if let Some(token) = part.split_whitespace().last() {
                if let Some(raw) = token.strip_suffix('%') {
                    if let Ok(value) = raw.parse::<u32>() {
                        percent = Some(value.clamp(0, 100));
                    }
                }
            }
            // Exact state words: "charging" / "charged" mean plugged in;
            // "discharging" must not match.
            if part == "charging" || part == "charged" {
                charging = true;
            }
        }
    }
    Some(BatteryStatus {
        percent: percent?,
        charging,
    })
}

// ── Windows ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn powershell_output(script: &str) -> Option<String> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(target_os = "windows")]
fn windows_cpu_usage() -> Option<f32> {
    let output = powershell_output(
        "Get-CimInstance Win32_Processor | ForEach-Object { $_.LoadPercentage }",
    )?;
    output
        .lines()
        .next()
        .and_then(|line| line.trim().parse::<f32>().ok())
        .map(|value| value.clamp(0.0, 100.0))
}

#[cfg(target_os = "windows")]
fn windows_memory() -> Option<(u64, Option<u64>)> {
    let output = powershell_output(
        "Get-CimInstance Win32_OperatingSystem | ForEach-Object { '{0}|{1}' -f $_.TotalVisibleMemorySize,$_.FreePhysicalMemory }",
    )?;
    let line = output.lines().next()?;
    let mut fields = line.trim().split('|');
    let total_kb: u64 = fields.next()?.trim().parse().ok()?;
    let free_kb: u64 = fields.next()?.trim().parse().ok()?;
    let used = total_kb.saturating_sub(free_kb).saturating_mul(1024);
    Some((total_kb.saturating_mul(1024), Some(used)))
}

#[cfg(target_os = "windows")]
fn windows_battery() -> Option<BatteryStatus> {
    let output = powershell_output(
        "Get-CimInstance Win32_Battery | ForEach-Object { '{0}|{1}' -f $_.EstimatedChargeRemaining,$_.BatteryStatus }",
    )?;
    let line = output.lines().next()?;
    let mut fields = line.trim().split('|');
    let percent: u32 = fields.next()?.trim().parse().ok()?;
    // BatteryStatus 2 = "The battery is not being discharged... connected to AC".
    let status_code: u32 = fields.next()?.trim().parse().ok()?;
    Some(BatteryStatus {
        percent: percent.clamp(0, 100),
        charging: status_code == 2,
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_cpu_line_parses_user_plus_sys() {
        let sample = concat!(
            "Processes: 500 total, 2 running, 498 sleeping\n",
            "CPU usage: 7.50% user, 4.16% sys, 88.33% idle\n",
            "Load Avg: 1.2, 1.5, 1.8\n"
        );
        let usage = parse_top_cpu(sample).expect("cpu line");
        assert!((usage - 11.66).abs() < 0.05, "got {usage}");

        assert!(parse_top_cpu("no cpu line here").is_none());
        assert!(
            parse_top_cpu("CPU usage: 100.00% user, 0.00% sys, 0.00% idle")
                .map(|value| value == 100.0)
                .unwrap_or(false)
        );
    }

    #[test]
    fn top_memory_line_parses_used_and_total() {
        let sample = concat!(
            "PhysMem: 12G used (3G wired, 2G purgeable), 5G unused.\n",
            "VM: 5.0T vsize, 2.5G framework vsize, 8192 swapins\n"
        );
        let (total, used) = parse_top_memory(sample).expect("physmem line");
        let expected_used: u64 = 12 * 1024 * 1024 * 1024;
        assert_eq!(used, Some(expected_used));
        let expected_unused: u64 = 5 * 1024 * 1024 * 1024;
        assert_eq!(total, expected_used + expected_unused);

        assert!(parse_top_memory("nothing").is_none());
    }

    #[test]
    fn size_suffixes_convert_to_bytes() {
        assert_eq!(parse_size_with_suffix("0K"), Some(0));
        assert_eq!(parse_size_with_suffix("12G"), Some(12 * 1024 * 1024 * 1024));
        assert_eq!(parse_size_with_suffix("512M"), Some(512 * 1024 * 1024));
        assert_eq!(parse_size_with_suffix("7.5G"), Some(8_053_063_680));
        assert_eq!(parse_size_with_suffix("42"), Some(42));
        assert_eq!(parse_size_with_suffix("12X"), None);
    }

    #[test]
    fn pmset_battery_lines_parse_percent_and_state() {
        let discharging =
            "-InternalBattery-0 (id=11409375)\t98%; discharging; 0:47 remaining present: true\n";
        let battery = parse_pmset_battery(discharging).expect("battery line");
        assert_eq!(battery.percent, 98);
        assert!(!battery.charging);

        let charged = "Now drawing from 'AC Power'\n-InternalBattery-0 (id=11409375)\t100%; charged; 0:00 remaining present: true\n";
        let battery = parse_pmset_battery(charged).expect("battery line");
        assert_eq!(battery.percent, 100);
        assert!(battery.charging, "AC power + charged counts as plugged in");

        let charging = "-InternalBattery-0 (id=1)\t55%; charging; 1:12 remaining present: true\n";
        let battery = parse_pmset_battery(charging).expect("battery line");
        assert_eq!(battery.percent, 55);
        assert!(battery.charging);

        assert!(parse_pmset_battery("no battery info\n").is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_memory_output_parses() {
        assert_eq!(
            windows_memory_from("16777216|4194304\n"),
            Some((16_777_216 * 1024, Some((16_777_216 - 4_194_304) * 1024)))
        );
    }

    #[cfg(target_os = "windows")]
    fn windows_memory_from(text: &str) -> Option<(u64, Option<u64>)> {
        let line = text.lines().next()?;
        let mut fields = line.trim().split('|');
        let total_kb: u64 = fields.next()?.trim().parse().ok()?;
        let free_kb: u64 = fields.next()?.trim().parse().ok()?;
        let used = total_kb.saturating_sub(free_kb).saturating_mul(1024);
        Some((total_kb.saturating_mul(1024), Some(used)))
    }
}
