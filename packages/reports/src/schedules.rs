//! Report schedules. Each row here is mirrored to the host scheduler
//! (same id), which runs `scheduled` when it falls due, on whichever device
//! is open. The row keeps what the page shows: how it repeats and how the
//! last run went.

use serde_json::{json, Map, Value};
use terrarium_guest::{clock, schedule, storage, Args, Error, Result};

const TABLE: &str = "schedules";
const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

fn text(args: &Args, key: &str) -> String {
    args.opt_str(key).map(str::trim).unwrap_or("").to_string()
}

/// `{every, days?, at, day?}` for the host, from the page's fields.
fn repeat(every: &str, days: &str, at: &str, day: &str) -> Result<Value> {
    let at = if at.is_empty() { "18:00" } else { at };
    let bytes = at.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || ![bytes[0], bytes[1], bytes[3], bytes[4]]
            .iter()
            .all(u8::is_ascii_digit)
        || (bytes[0] - b'0') * 10 + bytes[1] - b'0' >= 24
        || (bytes[3] - b'0') * 10 + bytes[4] - b'0' >= 60
    {
        return Err(Error::invalid("Time must be HH:MM"));
    }
    Ok(match every {
        "day" => json!({ "every": "day", "at": at }),
        "week" => {
            let mut picked = Vec::new();
            for value in days.split(',').map(str::trim) {
                if !DAYS.contains(&value) {
                    return Err(Error::invalid("Days must be mon through sun"));
                }
                if !picked.contains(&value) {
                    picked.push(value);
                }
            }
            if picked.is_empty() {
                return Err(Error::invalid("Pick at least one day of the week"));
            }
            json!({ "every": "week", "days": picked, "at": at })
        }
        "month" => {
            let day = if day == "last" {
                json!("last")
            } else {
                let n: u8 = if day.is_empty() {
                    1
                } else {
                    if !day.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(Error::invalid("Day of the month must be 1–31 or last"));
                    }
                    day.parse()
                        .map_err(|_| Error::invalid("Day of the month must be 1–31 or last"))?
                };
                if !(1..=31).contains(&n) {
                    return Err(Error::invalid("Day of the month must be 1–31 or last"));
                }
                json!(n)
            };
            json!({ "every": "month", "day": day, "at": at })
        }
        _ => return Err(Error::invalid("Repeat every day, week or month")),
    })
}

fn describe(every: &str, days: &str, at: &str, day: &str) -> String {
    match every {
        "day" => format!("Every day at {at}"),
        "week" => {
            let names: Vec<String> = DAYS
                .iter()
                .filter(|d| days.split(',').any(|x| x.trim() == **d))
                .map(|d| d[..1].to_uppercase() + &d[1..])
                .collect();
            format!("Every {} at {at}", names.join(", "))
        }
        _ if day == "last" => format!("Last day of the month at {at}"),
        _ => format!("Day {day} of every month at {at}"),
    }
}

fn mirror(id: &str, row: &Map<String, Value>) -> Result<()> {
    let s = |k: &str| row.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let enabled = row
        .get("enabled")
        .is_none_or(|v| v.as_bool() != Some(false) && v.as_i64() != Some(0));
    schedule::set(
        id,
        &json!({
            "name": format!("Report: {}", s("name")),
            "repeat": repeat(&s("every"), &s("days"), &s("at"), &s("day"))?,
            "operation": "scheduled",
            "args": { "schedule": id },
            "enabled": enabled,
        }),
    )
}

/// Creates or changes a schedule: id?, name, skill (empty for no AI),
/// period, sources, tags, every (day, week, month), days, at, day, enabled.
pub(crate) fn save(args: &Args) -> Result<Value> {
    let name = text(args, "name");
    if name.is_empty() || name.chars().count() > 60 {
        return Err(Error::invalid("Name the schedule (up to 60 characters)"));
    }
    let skill = text(args, "skill");
    if !skill.is_empty() && skill != "none" && crate::skills::find(&skill)?.is_none() {
        return Err(Error::invalid("Pick a skill that still exists"));
    }
    let period = match text(args, "period").as_str() {
        "" => "last_week".to_string(),
        p @ ("week" | "last_week" | "7d" | "month" | "last_month") => p.to_string(),
        _ => {
            return Err(Error::invalid(
                "Range must be week, last_week, 7d, month or last_month",
            ))
        }
    };
    let (every, days, at, day) = (
        text(args, "every"),
        text(args, "days"),
        text(args, "at"),
        text(args, "day"),
    );
    repeat(&every, &days, &at, &day)?;
    let at = if at.is_empty() {
        "18:00".to_string()
    } else {
        at
    };
    let mut values = Map::new();
    values.insert("name".into(), json!(name));
    values.insert(
        "skill".into(),
        json!(if skill == "none" { "" } else { &skill }),
    );
    values.insert("period".into(), json!(period));
    values.insert("sources".into(), json!(text(args, "sources")));
    values.insert("tags".into(), json!(text(args, "tags")));
    values.insert("every".into(), json!(every));
    values.insert("days".into(), json!(days));
    values.insert("at".into(), json!(at));
    values.insert("day".into(), json!(if day.is_empty() { "1" } else { &day }));
    values.insert("describe".into(), json!(describe(&every, &days, &at, &day)));
    let enabled = !matches!(args.get("enabled"), Some(Value::Bool(false)));
    values.insert("enabled".into(), json!(enabled));
    let id = text(args, "id");
    let id = if id.is_empty() {
        values.insert("created_at".into(), json!(clock::now_ms() as i64));
        storage::insert(TABLE, &values)?
    } else {
        storage::update(TABLE, &id, &values)?;
        id
    };
    mirror(&id, &values)?;
    Ok(json!({ "id": id, "message": "Schedule saved" }))
}

pub(crate) fn delete(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    storage::delete(TABLE, id)?;
    match schedule::remove(id) {
        Ok(()) => {}
        Err(e) if e.code == "not-found" => {}
        Err(e) => return Err(e),
    }
    Ok(json!({ "message": "Schedule removed" }))
}

pub(crate) fn toggle(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let enabled = args
        .get("enabled")
        .is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true"));
    let mut values = Map::new();
    values.insert("enabled".into(), json!(enabled));
    storage::update(TABLE, id, &values)?;
    let rows = storage::query(
        "SELECT * FROM schedules WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    if let Some(row) = rows.first() {
        mirror(id, row)?;
    }
    Ok(json!({ "message": if enabled { "Schedule on" } else { "Schedule off" } }))
}

pub(crate) fn run_now(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let rows = storage::query(
        "SELECT * FROM schedules WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let row = rows
        .first()
        .ok_or_else(|| Error::new("not_found", "That schedule is gone"))?;
    // Made before the host knew about it (or the host lost it): tell it again.
    if !schedule::list()?
        .iter()
        .any(|s| s["key"].as_str() == Some(id))
    {
        mirror(id, row)?;
    }
    schedule::run_now(id)?;
    record(id, "queued", None, None)?;
    Ok(json!({ "message": "Running now" }))
}

/// Notes how a schedule's latest run is going, for the page.
pub(crate) fn record(
    id: &str,
    status: &str,
    error: Option<&str>,
    report: Option<&str>,
) -> Result<()> {
    if storage::query(
        "SELECT id FROM schedules WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?
    .is_empty()
    {
        return Ok(());
    }
    let mut values = Map::new();
    values.insert("last_status".into(), json!(status));
    values.insert("last_error".into(), error.map_or(Value::Null, |e| json!(e)));
    values.insert("last_run_at".into(), json!(clock::now_ms() as i64));
    if let Some(report) = report {
        values.insert("last_report".into(), json!(report));
    }
    storage::update(TABLE, id, &values)
}

/// A due run, from the host: makes the schedule's report for the run's date.
pub(crate) fn run(args: &Args) -> Result<Value> {
    let id = args.str("schedule")?;
    let rows = storage::query(
        "SELECT * FROM schedules WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let Some(row) = rows.first() else {
        return Ok(
            json!({ "$run": { "status": "skipped", "message": "This schedule was removed" } }),
        );
    };
    let mut call = Map::new();
    for key in ["skill", "period", "sources", "tags"] {
        if let Some(v) = row
            .get(key)
            .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
        {
            call.insert(key.into(), v.clone());
        }
    }
    if !call.contains_key("skill") {
        call.insert("skill".into(), json!("none"));
    }
    call.insert("schedule_id".into(), json!(id));
    if let Some(run) = args.get("$run") {
        call.insert("$run".into(), run.clone());
    }
    let result = crate::generate(&Args(call))?;
    let report = result["id"].as_str().map(str::to_string);
    let report = report.as_deref();
    let status = match result["$run"]["status"].as_str() {
        Some("failed") => "failed",
        _ if row["skill"].as_str().is_some_and(|s| !s.is_empty()) => "pending",
        _ => "done",
    };
    record(id, status, result["$run"]["message"].as_str(), report)?;
    let mut result = result;
    if status == "done" && result.get("$run").is_none() {
        if let Some(report) = report {
            let rows = storage::query(
                "SELECT title, skill_name FROM reports WHERE id = ?1",
                &[json!(report)],
            )?;
            if let Some(row) = rows.first() {
                result["$run"] = json!({ "notify": crate::ai::ready_notice(row) });
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn malformed_schedule_fields_are_rejected_instead_of_changed_silently() {
        for bad in ["+1:00", "24:00", "01:+1", "é:00"] {
            assert!(repeat("day", "", bad, "").is_err());
        }
        for bad in ["bad", "+1", "0", "32", "999999"] {
            assert!(repeat("month", "", "09:30", bad).is_err());
        }
        assert!(repeat("week", "mon,bad", "09:30", "").is_err());
        assert_eq!(
            repeat("week", "mon,mon,tue", "09:30", "").unwrap()["days"],
            json!(["mon", "tue"])
        );
        assert_eq!(repeat("month", "", "", "").unwrap()["day"], 1);
    }
}
