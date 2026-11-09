//! Habits: scheduled habits, daily check-ins, sleep logs and reminders.
//!
//! Check-ins and sleep logs are append-only: the newest entry for a habit
//! and day wins, so two devices checking in offline never conflict.
//! Reminder rows are planned a week ahead (by a desktop job and on save)
//! and turned into system notifications by the host.

use serde_json::{json, Map, Value};
use terrarium_guest::{clock, storage, Args, Error, Plugin, Result};

struct Habits;

const PLAN_DAYS: i32 = 7;

impl Plugin for Habits {
    fn handle(operation: &str, args: Args) -> Result<Value> {
        match operation {
            "save" => save(&args),
            "delete" => {
                storage::delete("habits", args.str("id")?)?;
                Ok(Value::Null)
            }
            "restore" => {
                storage::restore("habits", args.str("id")?)?;
                plan_reminders()
            }
            "checkin" => checkin(&args),
            "add" => add(&args),
            "sleep_save" => sleep_save(&args),
            "plan_reminders" => plan_reminders(),
            "summary" => summary(&args),
            _ => Err(Error::unknown_operation(operation)),
        }
    }
}

fn text(args: &Args, key: &str, max: usize, label: &str, required: bool) -> Result<Option<String>> {
    match args.opt_str(key).map(str::trim) {
        Some(t) if t.chars().count() > max => Err(Error::invalid(format!(
            "{label} must be {max} characters or fewer"
        ))),
        Some(t) if !t.is_empty() => Ok(Some(t.to_string())),
        _ if required => Err(Error::invalid(format!("{label} is required"))),
        _ => Ok(None),
    }
}

/// "0,1,2" (Monday = 0) or a JSON array of numbers.
fn weekdays(value: Option<&Value>) -> Result<Option<String>> {
    let days: Vec<i64> = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => return Ok(None),
        Some(Value::String(s)) => s
            .split(',')
            .map(|d| {
                d.trim()
                    .parse::<i64>()
                    .map_err(|_| Error::invalid("Days must be numbers 0–6"))
            })
            .collect::<Result<_>>()?,
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_i64()
                    .ok_or_else(|| Error::invalid("Days must be whole numbers 0–6"))
            })
            .collect::<Result<_>>()?,
        _ => return Err(Error::invalid("Days must be a list like 0,1,2")),
    };
    if days.iter().any(|d| !(0..=6).contains(d)) {
        return Err(Error::invalid("Days must be numbers 0–6"));
    }
    let mut days = days;
    days.sort_unstable();
    days.dedup();
    if days.is_empty() {
        return Err(Error::invalid("Choose at least one day"));
    }
    Ok(Some(
        days.iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(","),
    ))
}

fn reminder_time(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let bytes = value.as_bytes();
    let ok = bytes.len() == 5
        && bytes[2] == b':'
        && [bytes[0], bytes[1], bytes[3], bytes[4]]
            .iter()
            .all(u8::is_ascii_digit)
        && (bytes[0] - b'0') * 10 + bytes[1] - b'0' < 24
        && (bytes[3] - b'0') * 10 + bytes[4] - b'0' < 60;
    if !ok {
        return Err(Error::invalid("Reminder time must be HH:MM"));
    }
    Ok(Some(value.to_string()))
}

fn notify_on(value: Option<&str>) -> Result<Option<String>> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v @ ("phone" | "desktop" | "both")) => Ok(Some(v.to_string())),
        Some(_) => Err(Error::invalid("Notify on must be phone, desktop or both")),
    }
}

/// A boolean from JSON or a form ("true", "on", "1").
fn flag(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => Some(n.as_i64() != Some(0)),
        Value::String(s) if s.is_empty() => None,
        Value::String(s) => Some(matches!(s.as_str(), "true" | "on" | "1")),
        _ => None,
    }
}

/// A whole number from JSON or a form field; blank is none.
fn number(value: Option<&Value>) -> Result<Option<i64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => s
            .trim()
            .parse::<i64>()
            .map(Some)
            .map_err(|_| Error::invalid("Enter a whole number")),
        Some(v) => v
            .as_i64()
            .map(Some)
            .ok_or_else(|| Error::invalid("Enter a whole number")),
    }
}

fn save(args: &Args) -> Result<Value> {
    let mut values = Map::new();
    let creating = args.opt_str("id").filter(|i| !i.is_empty()).is_none();
    if let Some(title) = text(args, "title", 120, "Habit name", creating)? {
        values.insert("title".into(), json!(title));
    }
    if let Some(category) = text(args, "category", 40, "Category", false)? {
        values.insert("category".into(), json!(category.to_lowercase()));
    }
    if let Some(days) = weekdays(args.get("weekdays"))? {
        values.insert("weekdays".into(), json!(days));
    } else if creating {
        values.insert("weekdays".into(), json!("0,1,2,3,4,5,6"));
    }
    if args.get("reminder").is_some() {
        values.insert(
            "reminder".into(),
            reminder_time(args.opt_str("reminder"))?.map_or(Value::Null, |t| json!(t)),
        );
    }
    if let Some(target) = notify_on(args.opt_str("notify_on"))? {
        values.insert("notify_on".into(), json!(target));
    }
    if let Some(paused) = flag(args.get("paused")) {
        values.insert("paused".into(), json!(paused));
    }
    if let Some(target) = number(args.get("target"))? {
        if !(1..=1000).contains(&target) {
            return Err(Error::invalid("Daily target must be 1 to 1000"));
        }
        values.insert("target".into(), json!(target));
    }
    if args.get("unit").is_some() {
        let unit = text(args, "unit", 20, "Unit", false)?;
        values.insert("unit".into(), unit.map_or(Value::Null, |u| json!(u)));
    }
    if args.get("notes").is_some() {
        let notes = text(args, "notes", 500, "Notes", false)?;
        values.insert("notes".into(), notes.map_or(Value::Null, |n| json!(n)));
    }
    let id = if creating {
        values.entry("category").or_insert(json!("personal"));
        values.insert("started".into(), json!(clock::today("")?));
        storage::insert("habits", &values)?
    } else {
        let id = args.str("id")?.to_string();
        if values.is_empty() {
            return Err(Error::invalid("nothing to update"));
        }
        storage::update("habits", &id, &values)?;
        id
    };
    plan_reminders()?;
    Ok(json!({ "id": id }))
}

fn day_arg(args: &Args) -> Result<String> {
    let today = clock::today("")?;
    let day = args
        .opt_str("day")
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or(&today)
        .to_string();
    clock::weekday(&day).map_err(|_| Error::invalid("Day must be YYYY-MM-DD"))?;
    if day > today || day < clock::add_days(&today, -365)? {
        return Err(Error::invalid(
            "Check in for today or one of the previous 365 days",
        ));
    }
    Ok(day)
}

/// The habit's daily target; errors when it is missing or in trash.
fn target_of(habit: &str) -> Result<i64> {
    let found = storage::query(
        "SELECT COALESCE(target, 1) AS target FROM habits WHERE id = ?1 AND _deleted IS NULL",
        &[json!(habit)],
    )?;
    found
        .first()
        .map(|r| r["target"].as_i64().unwrap_or(1).max(1))
        .ok_or_else(|| Error::new("not-found", "Habit not found"))
}

fn record(
    habit: &str,
    day: String,
    status: &str,
    count: Option<i64>,
    args: &Args,
) -> Result<Value> {
    let mut values = Map::new();
    values.insert("habit_id".into(), json!(habit));
    values.insert("day".into(), json!(day));
    values.insert("status".into(), json!(status));
    if let Some(count) = count {
        values.insert("count".into(), json!(count));
    }
    if let Some(note) = text(args, "note", 2000, "Note", false)? {
        values.insert("note".into(), json!(note));
    }
    let id = storage::insert("checkins", &values)?;
    Ok(json!({ "id": id, "status": status, "count": count }))
}

fn checkin(args: &Args) -> Result<Value> {
    let habit = args.str("id")?;
    // A checkbox sends `done` instead of a status.
    let status = match flag(args.get("done")) {
        Some(true) => "done",
        Some(false) => "cleared",
        None => args
            .opt_str("status")
            .filter(|s| !s.is_empty())
            .unwrap_or("done"),
    };
    if !matches!(status, "done" | "skipped" | "cleared") {
        return Err(Error::invalid("Status must be done, skipped or cleared"));
    }
    let target = target_of(habit)?;
    let day = day_arg(args)?;
    // A count below the target is partial progress, not done.
    let count = number(args.get("count"))?.map(|c| c.clamp(0, 100_000));
    let (status, count) = match (status, count) {
        ("done", Some(c)) if c < target => (if c > 0 { "partial" } else { "cleared" }, Some(c)),
        ("done", _) if target > 1 => ("done", Some(target)),
        (s, c) => (s, c),
    };
    record(habit, day, status, count, args)
}

/// Steps a counted habit's progress for a day up or down.
fn add(args: &Args) -> Result<Value> {
    let habit = args.str("id")?;
    let target = target_of(habit)?;
    let day = day_arg(args)?;
    let by = number(args.get("by"))?.unwrap_or(1);
    let latest = storage::query(
        "SELECT status, count FROM checkins WHERE habit_id = ?1 AND day = ?2 ORDER BY id DESC LIMIT 1",
        &[json!(habit), json!(day)],
    )?;
    let now = match latest.first() {
        Some(r) if r["status"] == "done" => r["count"].as_i64().unwrap_or(target),
        Some(r) if r["status"] == "partial" => r["count"].as_i64().unwrap_or(0),
        _ => 0,
    };
    let count = (now + by).clamp(0, 100_000);
    let status = if count >= target {
        "done"
    } else if count > 0 {
        "partial"
    } else {
        "cleared"
    };
    record(habit, day, status, Some(count), args)
}

fn sleep_save(args: &Args) -> Result<Value> {
    let hours = args.get("hours").and_then(Value::as_f64);
    let minutes = args
        .get("minutes")
        .and_then(Value::as_i64)
        .or(hours.map(|h| (h * 60.0).round() as i64));
    let minutes = minutes
        .filter(|m| (1..=1440).contains(m))
        .ok_or_else(|| Error::invalid("Enter 1 minute to 24 hours of sleep"))?;
    let mut values = Map::new();
    values.insert("day".into(), json!(day_arg(args)?));
    values.insert("minutes".into(), json!(minutes));
    if let Some(note) = text(args, "note", 2000, "Note", false)? {
        values.insert("note".into(), json!(note));
    }
    Ok(json!({ "id": storage::insert("sleep", &values)? }))
}

/// Keeps one reminder row per habit and scheduled day for the next week.
fn plan_reminders() -> Result<Value> {
    let now = clock::now_ms();
    let today = clock::today("")?;
    let habits = storage::query(
        "SELECT id, weekdays, reminder FROM habits WHERE _deleted IS NULL AND paused = 0 AND reminder IS NOT NULL",
        &[],
    )?;
    let existing = storage::query(
        "SELECT id, habit_id, day, at_ms FROM reminders WHERE _deleted IS NULL",
        &[],
    )?;
    let mut wanted: Vec<(String, String, u64)> = Vec::new();
    for habit in &habits {
        let id = habit["id"].as_str().unwrap_or_default();
        let days = habit["weekdays"].as_str().unwrap_or_default();
        let at = habit["reminder"].as_str().unwrap_or_default();
        for offset in 0..PLAN_DAYS {
            let day = clock::add_days(&today, offset)?;
            let weekday = clock::weekday(&day)?.to_string();
            if !days.split(',').any(|d| d == weekday) {
                continue;
            }
            let due = clock::local_to_utc(&day, at, "")?;
            if due > now {
                wanted.push((id.to_string(), day, due));
            }
        }
    }
    let (mut added, mut removed) = (0, 0);
    for row in &existing {
        let key = (
            row["habit_id"].as_str().unwrap_or(""),
            row["day"].as_str().unwrap_or(""),
            row["at_ms"].as_u64().unwrap_or(0),
        );
        let still = wanted
            .iter()
            .any(|(h, d, a)| (h.as_str(), d.as_str(), *a) == key);
        let past = key.2 + 24 * 3_600_000 < now;
        if !still && (past || key.2 > now) {
            storage::delete("reminders", row["id"].as_str().unwrap_or_default())?;
            removed += 1;
        }
    }
    for (habit, day, due) in wanted {
        let present = existing.iter().any(|r| {
            r["habit_id"].as_str() == Some(&habit)
                && r["day"].as_str() == Some(&day)
                && r["at_ms"].as_u64() == Some(due)
        });
        if !present {
            let mut values = Map::new();
            values.insert("habit_id".into(), json!(habit));
            values.insert("day".into(), json!(day));
            values.insert("at_ms".into(), json!(due as i64));
            storage::insert("reminders", &values)?;
            added += 1;
        }
    }
    Ok(json!({ "added": added, "removed": removed }))
}

/// Completion per habit for local dates `start..=end`, for other plugins.
fn summary(args: &Args) -> Result<Value> {
    let start = args.str("start")?;
    let end = args.str("end")?;
    let habits = storage::query("SELECT id, title, weekdays FROM habits WHERE _deleted IS NULL AND paused = 0 ORDER BY title", &[])?;
    let latest = storage::query(
        "SELECT c.habit_id, c.day, c.status FROM checkins c WHERE c.day BETWEEN ?1 AND ?2 \
         AND c.id = (SELECT c2.id FROM checkins c2 WHERE c2.habit_id = c.habit_id AND c2.day = c.day ORDER BY c2.id DESC LIMIT 1)",
        &[json!(start), json!(end)],
    )?;
    let mut out = Vec::new();
    let (mut all_scheduled, mut all_done) = (0, 0);
    for habit in &habits {
        let id = habit["id"].as_str().unwrap_or_default();
        let days = habit["weekdays"].as_str().unwrap_or_default();
        let (mut scheduled, mut done, mut skipped) = (0, 0, 0);
        let mut day = start.to_string();
        let mut guard = 0;
        while day.as_str() <= end && guard < 400 {
            let weekday = clock::weekday(&day)?.to_string();
            if days.split(',').any(|d| d == weekday) {
                scheduled += 1;
                match latest
                    .iter()
                    .find(|c| c["habit_id"].as_str() == Some(id) && c["day"].as_str() == Some(&day))
                    .and_then(|c| c["status"].as_str())
                {
                    Some("done") => done += 1,
                    Some("skipped") => skipped += 1,
                    _ => {}
                }
            }
            day = clock::add_days(&day, 1)?;
            guard += 1;
        }
        let counted = scheduled - skipped;
        all_scheduled += counted;
        all_done += done;
        out.push(json!({
            "title": habit["title"],
            "scheduled": scheduled,
            "done": done,
            "skipped": skipped,
            "rate": if counted > 0 { json!((done as f64) / (counted as f64)) } else { Value::Null },
        }));
    }
    Ok(json!({
        "start": start,
        "end": end,
        "habits": out,
        "rate": if all_scheduled > 0 { json!((all_done as f64) / (all_scheduled as f64)) } else { Value::Null },
    }))
}

terrarium_guest::plugin!(Habits);

#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn reminder_times_reject_unicode_and_invalid_hours_without_panicking() {
        for invalid in ["0é:0", "é:00", "24:00", "12:60", "１２:０", "-1:00"] {
            assert!(reminder_time(Some(invalid)).is_err(), "{invalid}");
        }
        assert_eq!(reminder_time(Some("09:30")).unwrap(), Some("09:30".into()));
    }
}
