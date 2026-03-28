use chrono::{Datelike, Timelike};

use crate::models::schedule::RecurringConfig;

/// Converts a RecurringConfig into a cron expression string.
/// Format: sec min hour day-of-month month day-of-week
/// (tokio-cron-scheduler uses 6-field cron with seconds)
pub fn to_cron(cfg: &RecurringConfig) -> Result<String, String> {
    match cfg.interval_unit.as_str() {
        "minutes" => {
            let n = cfg.interval_value;
            if n == 0 || n > 59 {
                return Err(format!("invalid minute interval: {}", n));
            }
            Ok(format!("0 */{} * * * *", n))
        }
        "hours" => {
            let n = cfg.interval_value;
            if n == 0 || n > 23 {
                return Err(format!("invalid hour interval: {}", n));
            }
            Ok(format!("0 0 */{} * * *", n))
        }
        "days" => {
            let (hour, minute) = time_parts(cfg)?;
            Ok(format!("0 {} {} * * *", minute, hour))
        }
        "weeks" => {
            let (hour, minute) = time_parts(cfg)?;
            let days = cfg
                .days_of_week
                .as_deref()
                .unwrap_or(&[1]) // default Monday
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join(",");
            Ok(format!("0 {} {} * * {}", minute, hour, days))
        }
        "months" => {
            let (hour, minute) = time_parts(cfg)?;
            Ok(format!("0 {} {} 1 * *", minute, hour))
        }
        other => Err(format!("unknown interval_unit: {}", other)),
    }
}

fn time_parts(cfg: &RecurringConfig) -> Result<(u8, u8), String> {
    let tod = cfg.time_of_day.as_ref();
    let hour = tod.map(|t| t.hour).unwrap_or(0);
    let minute = tod.map(|t| t.minute).unwrap_or(0);
    if hour > 23 {
        return Err(format!("invalid hour: {}", hour));
    }
    if minute > 59 {
        return Err(format!("invalid minute: {}", minute));
    }
    Ok((hour, minute))
}

/// Returns the next N future run times for a RecurringConfig.
/// Used by the frontend's "Next 5 runs" preview.
pub fn next_n_runs(cfg: &RecurringConfig, n: usize) -> Vec<String> {
    let cron_expr = match to_cron(cfg) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    // Use cron_parser or manual calculation for preview
    // For now: calculate based on interval from now
    let mut results = Vec::new();
    let mut base = chrono::Utc::now();

    for _ in 0..n {
        base = advance(cfg, base);
        results.push(base.to_rfc3339());
    }

    let _ = cron_expr; // suppress warning — used for validation above
    results
}

fn advance(
    cfg: &RecurringConfig,
    from: chrono::DateTime<chrono::Utc>,
) -> chrono::DateTime<chrono::Utc> {
    use chrono::Duration;

    match cfg.interval_unit.as_str() {
        "minutes" => from + Duration::minutes(cfg.interval_value as i64),
        "hours" => from + Duration::hours(cfg.interval_value as i64),
        "days" => from + Duration::days(cfg.interval_value as i64),
        "weeks" => from + Duration::weeks(cfg.interval_value as i64),
        "months" => {
            let naive = from.naive_utc();
            let month0 = naive.date().month() - 1; // convert to 0-based
            let new_month = (month0 + cfg.interval_value) % 12 + 1; // back to 1-based
            let new_year = naive.date().year() + ((month0 + cfg.interval_value) / 12) as i32;
            chrono::NaiveDate::from_ymd_opt(new_year, new_month, 1)
                .and_then(|d| d.and_hms_opt(naive.hour(), naive.minute(), 0))
                .map(|dt| {
                    chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc)
                })
                .unwrap_or(from)
        }
        _ => from,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::schedule::{RecurringConfig, TimeOfDay};

    #[test]
    fn minutes_cron() {
        let cfg = RecurringConfig {
            interval_unit: "minutes".to_string(),
            interval_value: 30,
            days_of_week: None,
            time_of_day: None,
            timezone: "UTC".to_string(),
            missed_run_policy: "skip".to_string(),
        };
        assert_eq!(to_cron(&cfg).unwrap(), "0 */30 * * * *");
    }

    #[test]
    fn weekly_cron() {
        let cfg = RecurringConfig {
            interval_unit: "weeks".to_string(),
            interval_value: 1,
            days_of_week: Some(vec![1, 3]),
            time_of_day: Some(TimeOfDay { hour: 9, minute: 0 }),
            timezone: "UTC".to_string(),
            missed_run_policy: "skip".to_string(),
        };
        assert_eq!(to_cron(&cfg).unwrap(), "0 0 9 * * 1,3");
    }
}
