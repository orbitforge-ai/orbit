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
            let (_, minute) = time_parts(cfg)?;
            Ok(format!("0 {} */{} * * *", minute, n))
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

/// Computes the next run time after `after` for a cron expression.
/// Uses the `cron` crate for accurate scheduling.
pub fn compute_next_after(cron_expr: &str, after: chrono::DateTime<chrono::Utc>) -> Option<String> {
    use std::str::FromStr;
    let schedule = cron::Schedule::from_str(cron_expr).ok()?;
    let next = schedule.after(&after).next()?;
    Some(next.to_rfc3339())
}

/// Computes the next run time after now for a cron expression.
pub fn compute_next(cron_expr: &str) -> String {
    compute_next_after(cron_expr, chrono::Utc::now()).unwrap_or_else(|| {
        // Fallback: advance 1 hour from now
        (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339()
    })
}

/// Returns the next N future run times for a RecurringConfig.
/// Used by the frontend's "Next 5 runs" preview.
pub fn next_n_runs(cfg: &RecurringConfig, n: usize) -> Vec<String> {
    use std::str::FromStr;

    let cron_expr = match to_cron(cfg) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let schedule = match cron::Schedule::from_str(&cron_expr) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    schedule
        .after(&chrono::Utc::now())
        .take(n)
        .map(|dt| dt.to_rfc3339())
        .collect()
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
            expression: None,
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
            expression: None,
        };
        assert_eq!(to_cron(&cfg).unwrap(), "0 0 9 * * 1,3");
    }

    #[test]
    fn hourly_offset_cron() {
        let cfg = RecurringConfig {
            interval_unit: "hours".to_string(),
            interval_value: 1,
            days_of_week: None,
            time_of_day: Some(TimeOfDay { hour: 0, minute: 10 }),
            timezone: "UTC".to_string(),
            missed_run_policy: "skip".to_string(),
            expression: None,
        };
        assert_eq!(to_cron(&cfg).unwrap(), "0 10 */1 * * *");
    }

    #[test]
    fn compute_next_works() {
        let next = compute_next("0 */15 * * * *");
        assert!(!next.is_empty());
    }
}
