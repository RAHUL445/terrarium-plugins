//! Time estimates and tracking (ClickUp-style).
//!
//! `tasks.estimate_min` is a task's own estimate in minutes. `time_entries`
//! holds the time spent: a timed entry starts with `ended_at` empty (the timer
//! is running) and gets `ended_at` and `minutes` when stopped; a manual entry
//! is written finished, with `ended_at` = `started_at` and its duration in
//! `minutes`. `started_at` and `ended_at` are unix seconds.
//!
//! There is at most one running timer per space: starting one stops the
//! others in the same operation. Two devices can still start a timer while
//! offline, so readers tolerate several running rows (each counts its own
//! elapsed time) and `stop_timer` stops every one of them, whichever device
//! started it. Entries are trashed, never removed, so they can be restored.
//!
//! Stopping rounds the elapsed time to whole minutes with a minimum of 1.
//! Queries show a running entry's elapsed time using the time the caller
//! passes (`now`), so totals are exact whenever a view refreshes.
use serde_json::{json, Map, Value};
use terrarium_guest::{clock, storage, Args, Error, Result};

use crate::TABLE;

const ENTRIES: &str = "time_entries";
/// 1,000 hours: far beyond any sane entry, and keeps arithmetic safe.
const MAX_MINUTES: i64 = 60_000;
const MAX_NOTE: usize = 500;

type Row = Map<String, Value>;

fn bad_duration(text: &str) -> Error {
    Error::invalid(format!(
        "Couldn't read “{}” as a time. Try 1h 30m, 90m, 1.5h or 45 (minutes)",
        text.trim().chars().take(40).collect::<String>()
    ))
}

fn check_range(minutes: i64) -> Result<i64> {
    if minutes < 1 {
        return Err(Error::invalid("Time must be at least 1 minute"));
    }
    if minutes > MAX_MINUTES {
        return Err(Error::invalid("Time is limited to 1,000 hours"));
    }
    Ok(minutes)
}

fn number(text: &str) -> Option<f64> {
    let dots = text.bytes().filter(|c| *c == b'.').count();
    let digits = text.bytes().filter(u8::is_ascii_digit).count();
    if dots > 1 || digits == 0 || text.len() > 9 {
        return None;
    }
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Reads a duration as whole minutes: `45`, `90m`, `1.5h`, `1h 30m`, `1h30`,
/// `2 hours 15 minutes` or `1:30`. A bare number is minutes. Zero, negative
/// and unreadable values are rejected with a message that shows the formats.
pub fn parse_duration(text: &str) -> Result<i64> {
    let lower = text.trim().to_lowercase();
    if lower.is_empty() {
        return Err(bad_duration(text));
    }
    if let Some((h, m)) = lower.split_once(':') {
        let digits = |s: &str, max: usize| {
            !s.is_empty() && s.len() <= max && s.bytes().all(|c| c.is_ascii_digit())
        };
        if !digits(h, 4) || !digits(m, 2) {
            return Err(bad_duration(text));
        }
        let (h, m): (i64, i64) = (h.parse().unwrap_or(0), m.parse().unwrap_or(0));
        if m > 59 {
            return Err(bad_duration(text));
        }
        return check_range(h * 60 + m);
    }
    let chars: Vec<char> = lower.chars().collect();
    let skip = |mut i: usize| {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
            i += 1;
        }
        i
    };
    let (mut total, mut seen_h, mut seen_m) = (0.0_f64, false, false);
    let mut i = skip(0);
    while i < chars.len() {
        let start = i;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
            i += 1;
        }
        let n = number(&chars[start..i].iter().collect::<String>())
            .ok_or_else(|| bad_duration(text))?;
        let after_number = i;
        i = skip(i);
        let unit_start = i;
        while i < chars.len() && chars[i].is_ascii_alphabetic() {
            i += 1;
        }
        let unit: String = chars[unit_start..i].iter().collect();
        match unit.as_str() {
            "h" | "hr" | "hrs" | "hour" | "hours" if !seen_h && !seen_m => {
                total += n * 60.0;
                seen_h = true;
            }
            "m" | "min" | "mins" | "minute" | "minutes" if !seen_m => {
                total += n;
                seen_m = true;
            }
            "" => {
                // A bare number is minutes: alone, or the tail of "1h30".
                let last = skip(after_number) >= chars.len();
                let whole = chars[start..after_number].iter().all(char::is_ascii_digit);
                let alone = start == skip(0) && last;
                if !((alone || (seen_h && !seen_m && last)) && whole) {
                    return Err(bad_duration(text));
                }
                total += n;
                seen_m = true;
            }
            _ => return Err(bad_duration(text)),
        }
        i = skip(i);
    }
    check_range(total.round() as i64)
}

/// Whole minutes from a JSON argument: an integer, or text to parse.
fn minutes_value(value: &Value) -> Result<i64> {
    match value {
        Value::String(s) => parse_duration(s),
        Value::Number(n) => match n.as_i64() {
            Some(m) => check_range(m),
            None => Err(Error::invalid("Minutes must be a whole number")),
        },
        _ => Err(Error::invalid(
            "Time must be a number of minutes or text like 1h 30m",
        )),
    }
}

fn blank(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.trim().is_empty(),
        _ => false,
    }
}

/// `1h 20m`, `45m`, `3h`.
pub fn format_minutes(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

fn now_s() -> i64 {
    (clock::now_ms() / 1000) as i64
}

/// Minutes shown for a timer that has run `seconds`: rounded to the nearest.
fn elapsed(seconds: i64) -> i64 {
    (seconds.max(0) + 30) / 60
}

/// Minutes stored when a timer stops after `seconds`: rounded, at least 1.
pub fn stopped_minutes(seconds: i64) -> i64 {
    elapsed(seconds).max(1)
}

fn live_task(id: &str) -> Result<Row> {
    storage::query(
        "SELECT * FROM tasks WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| Error::new("not-found", "That task no longer exists"))
}

fn live_entry(id: &str) -> Result<Row> {
    storage::query(
        "SELECT * FROM time_entries WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| Error::new("not-found", "That time entry no longer exists"))
}

fn int(row: &Row, key: &str) -> Option<i64> {
    row.get(key).and_then(Value::as_i64)
}

fn text(row: &Row, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn clean_note(note: &str) -> Result<Option<String>> {
    let note = note.trim();
    if note.chars().count() > MAX_NOTE {
        return Err(Error::invalid(format!(
            "A note is limited to {MAX_NOTE} characters"
        )));
    }
    Ok((!note.is_empty()).then(|| note.to_string()))
}

/// Noon on a local date as unix seconds, so the entry stays on that date in
/// any time zone the user travels to.
fn noon_of(date: &str) -> Result<i64> {
    let shaped = date.len() == 10
        && date.bytes().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == b'-'
            } else {
                c.is_ascii_digit()
            }
        });
    if !shaped || crate::quick::to_days(date).is_none() {
        return Err(Error::invalid("A date must look like 2026-11-01"));
    }
    Ok((clock::local_to_utc(date, "12:00", "")? / 1000) as i64)
}

fn date_arg<'a>(args: &'a Args, key: &str) -> Result<Option<&'a str>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim())),
        Some(_) => Err(Error::invalid(format!("`{key}` must be a date"))),
    }
}

/// Minutes from `minutes` or `duration` (either may be a number or text).
fn duration_arg(args: &Args) -> Result<Option<i64>> {
    for key in ["minutes", "duration"] {
        if !blank(args.get(key)) {
            return minutes_value(args.get(key).unwrap_or(&Value::Null)).map(Some);
        }
    }
    Ok(None)
}

/// `set_estimate {id, minutes}`: sets a task's own estimate, or clears it
/// with an empty value (or 0). Returns the `previous` estimate (empty when
/// there was none) so Undo can put it back.
pub fn set_estimate(args: &Args) -> Result<Value> {
    let id = args.str("id")?.trim();
    let task = live_task(id)?;
    let raw = args.get("minutes");
    let clear = blank(raw) || matches!(raw, Some(Value::Number(n)) if n.as_i64() == Some(0));
    let estimate = if clear {
        None
    } else {
        Some(minutes_value(raw.unwrap_or(&Value::Null))?)
    };
    let previous = int(&task, "estimate_min");
    if previous != estimate {
        let mut values = Row::new();
        values.insert("estimate_min".into(), json!(estimate));
        storage::update(TABLE, id, &values)?;
    }
    Ok(json!({
        "id": id,
        "estimate_min": estimate,
        "estimate": estimate.map(format_minutes),
        "previous": previous.map_or(json!(""), |m| json!(m)),
        "changed": previous != estimate,
    }))
}

struct Stopped {
    id: String,
    task_id: String,
    title: String,
    minutes: i64,
}

impl Stopped {
    fn to_json(&self) -> Value {
        json!({"id": self.id, "task_id": self.task_id, "title": self.title, "minutes": self.minutes})
    }
}

/// Stops every running timer except the entry `keep`, returning what stopped.
fn stop_running(keep: Option<&str>) -> Result<Vec<Stopped>> {
    let rows = storage::query(
        "SELECT e.id, e.task_id, e.started_at, COALESCE((SELECT t.title FROM tasks t WHERE t.id = e.task_id), '') AS title \
         FROM time_entries e WHERE e.ended_at IS NULL AND e._deleted IS NULL ORDER BY e.started_at, e.id",
        &[],
    )?;
    let now = now_s();
    let mut out = Vec::new();
    for row in rows {
        let id = text(&row, "id");
        if keep == Some(id.as_str()) {
            continue;
        }
        let started = int(&row, "started_at").unwrap_or(now);
        let minutes = stopped_minutes(now - started);
        let mut values = Row::new();
        values.insert("ended_at".into(), json!(now.max(started)));
        values.insert("minutes".into(), json!(minutes));
        storage::update(ENTRIES, &id, &values)?;
        out.push(Stopped {
            id,
            task_id: text(&row, "task_id"),
            title: text(&row, "title"),
            minutes,
        });
    }
    Ok(out)
}

fn ids(stopped: &[Stopped]) -> String {
    stopped
        .iter()
        .map(|s| s.id.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// `start_timer {task_id}`: starts a timer on a task, stopping the running
/// one first. Starting the task that already runs changes nothing.
pub fn start_timer(args: &Args) -> Result<Value> {
    let task_id = args.str("task_id")?.trim();
    let task = live_task(task_id)?;
    let title = text(&task, "title");
    if task
        .get("done")
        .is_some_and(|d| d.as_bool().unwrap_or_else(|| d.as_i64() == Some(1)))
    {
        return Err(Error::invalid(
            "That task is done. Reopen it to track more time",
        ));
    }
    let running = storage::query(
        "SELECT id, started_at FROM time_entries WHERE task_id = ?1 AND ended_at IS NULL \
         AND _deleted IS NULL ORDER BY started_at, id LIMIT 1",
        &[json!(task_id)],
    )?;
    if let Some(entry) = running.first() {
        let keep = text(entry, "id");
        let stopped = stop_running(Some(&keep))?;
        return Ok(json!({
            "id": keep,
            "task_id": task_id,
            "title": title,
            "started_at": entry["started_at"],
            "already_running": true,
            "stopped": stopped.iter().map(Stopped::to_json).collect::<Vec<_>>(),
            "stopped_ids": ids(&stopped),
            "message": "Timer already running",
        }));
    }
    let stopped = stop_running(None)?;
    let started = now_s();
    let mut values = Row::new();
    values.insert("task_id".into(), json!(task_id));
    values.insert("started_at".into(), json!(started));
    let id = storage::insert(ENTRIES, &values)?;
    let message = match stopped.first() {
        Some(prev) => format!(
            "Timer started · stopped “{}” at {}",
            prev.title,
            format_minutes(stopped.iter().map(|s| s.minutes).sum())
        ),
        None => "Timer started".to_string(),
    };
    Ok(json!({
        "id": id,
        "task_id": task_id,
        "title": title,
        "started_at": started,
        "already_running": false,
        "stopped": stopped.iter().map(Stopped::to_json).collect::<Vec<_>>(),
        "stopped_ids": ids(&stopped),
        "message": message,
    }))
}

/// `stop_timer {}`: stops whatever timer runs (on any device). Nothing
/// running is not an error. Returns `ids` of the stopped entries for Undo.
pub fn stop_timer() -> Result<Value> {
    let stopped = stop_running(None)?;
    let Some(first) = stopped.first() else {
        return Ok(json!({"stopped": 0, "ids": "", "message": "No timer is running"}));
    };
    let minutes: i64 = stopped.iter().map(|s| s.minutes).sum();
    Ok(json!({
        "stopped": stopped.len(),
        "id": first.id,
        "ids": ids(&stopped),
        "task_id": first.task_id,
        "title": first.title,
        "minutes": minutes,
        "entries": stopped.iter().map(Stopped::to_json).collect::<Vec<_>>(),
        "message": format!("Timer stopped · {} on “{}”", format_minutes(minutes), first.title),
    }))
}

/// Stops the timer(s) running on one of `tasks`, quietly, for finishing or
/// trashing a task. Returns the minutes logged, if any timer ran.
pub fn stop_for_tasks(tasks: &[&str]) -> Result<Option<i64>> {
    let now = now_s();
    let mut total = None;
    for task in tasks {
        let rows = storage::query(
            "SELECT id, started_at FROM time_entries WHERE task_id = ?1 AND ended_at IS NULL \
             AND _deleted IS NULL ORDER BY started_at, id",
            &[json!(task)],
        )?;
        for row in rows {
            let started = int(&row, "started_at").unwrap_or(now);
            let minutes = stopped_minutes(now - started);
            let mut values = Row::new();
            values.insert("ended_at".into(), json!(now.max(started)));
            values.insert("minutes".into(), json!(minutes));
            storage::update(ENTRIES, &text(&row, "id"), &values)?;
            total = Some(total.unwrap_or(0) + minutes);
        }
    }
    Ok(total)
}

fn id_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// `resume_timer {ids}`: Undo for `stop_timer`; reopens stopped entries (the
/// time they ran counts again) and stops any other timer first.
pub fn resume_timer(args: &Args) -> Result<Value> {
    let wanted = id_list(args.str("ids")?);
    if wanted.is_empty() {
        return Err(Error::invalid("Give the timer entries to resume"));
    }
    let mut entries = Vec::new();
    for id in &wanted {
        entries.push(live_entry(id)?);
    }
    for entry in &entries {
        stop_running(Some(&text(entry, "id")))?;
    }
    let mut reopened = 0;
    for entry in &entries {
        if entry.get("ended_at").is_some_and(|v| !v.is_null()) {
            let mut values = Row::new();
            values.insert("ended_at".into(), Value::Null);
            values.insert("minutes".into(), Value::Null);
            storage::update(ENTRIES, &text(entry, "id"), &values)?;
            reopened += 1;
        }
    }
    Ok(json!({"resumed": reopened}))
}

/// `undo_start_timer {id, stopped_ids}`: Undo for `start_timer`; drops the new
/// entry and reopens the timers it stopped, as one step.
pub fn undo_start_timer(args: &Args) -> Result<Value> {
    let id = args.str("id")?.trim();
    if live_entry(id).is_ok() {
        storage::delete(ENTRIES, id)?;
    }
    let previous = id_list(args.opt_str("stopped_ids").unwrap_or(""));
    for old in &previous {
        if let Ok(entry) = live_entry(old) {
            if entry.get("ended_at").is_some_and(|v| !v.is_null()) {
                let mut values = Row::new();
                values.insert("ended_at".into(), Value::Null);
                values.insert("minutes".into(), Value::Null);
                storage::update(ENTRIES, old, &values)?;
            }
        }
    }
    Ok(json!({"id": id, "resumed": previous.len()}))
}

/// `add_time {task_id, minutes|duration, date?, note?}`: logs time by hand.
pub fn add_time(args: &Args) -> Result<Value> {
    let task_id = args.str("task_id")?.trim();
    live_task(task_id)?;
    let minutes = duration_arg(args)?
        .ok_or_else(|| Error::invalid("Give the time spent, like 1h 30m or 45"))?;
    let started = match date_arg(args, "date")? {
        Some(date) => noon_of(date)?,
        None => now_s(),
    };
    let mut values = Row::new();
    values.insert("task_id".into(), json!(task_id));
    values.insert("started_at".into(), json!(started));
    values.insert("ended_at".into(), json!(started));
    values.insert("minutes".into(), json!(minutes));
    if let Some(note) = clean_note(args.opt_str("note").unwrap_or(""))? {
        values.insert("note".into(), json!(note));
    }
    let id = storage::insert(ENTRIES, &values)?;
    Ok(json!({
        "id": id,
        "task_id": task_id,
        "minutes": minutes,
        "duration": format_minutes(minutes),
        "message": format!("Added {}", format_minutes(minutes)),
    }))
}

/// `update_time_entry {id, minutes|duration?, date?, note?}`: edits an entry.
/// A running one can only get a note. Returns the previous values for Undo.
pub fn update_time_entry(args: &Args) -> Result<Value> {
    let id = args.str("id")?.trim();
    let entry = live_entry(id)?;
    let running = entry.get("ended_at").is_none_or(Value::is_null);
    let started = int(&entry, "started_at").unwrap_or(0);
    let span = int(&entry, "ended_at").map_or(0, |e| (e - started).max(0));
    let previous_minutes = int(&entry, "minutes");
    let previous_note = text(&entry, "note");
    let previous_date = clock::local_date((started.max(0) as u64) * 1000, "")?;
    let mut values = Row::new();
    if let Some(minutes) = duration_arg(args)? {
        if running {
            return Err(Error::invalid("Stop the timer before changing its time"));
        }
        values.insert("minutes".into(), json!(minutes));
    }
    if let Some(date) = date_arg(args, "date")? {
        if running {
            return Err(Error::invalid("Stop the timer before changing its date"));
        }
        let moved = noon_of(date)?;
        values.insert("started_at".into(), json!(moved));
        values.insert("ended_at".into(), json!(moved + span));
    }
    if let Some(note) = args.opt_str("note") {
        values.insert(
            "note".into(),
            clean_note(note)?.map_or(Value::Null, |n| json!(n)),
        );
    }
    if values.is_empty() {
        return Err(Error::invalid("nothing to update"));
    }
    storage::update(ENTRIES, id, &values)?;
    Ok(json!({
        "id": id,
        "task_id": entry["task_id"],
        "minutes": values.get("minutes").cloned().unwrap_or(json!(previous_minutes)),
        "previous_minutes": previous_minutes,
        "previous_date": previous_date,
        "previous_note": previous_note,
    }))
}

/// `delete_time_entry {id}`: moves an entry to the trash (a running timer
/// ends with it). Deleting one that is gone changes nothing.
pub fn delete_time_entry(args: &Args) -> Result<Value> {
    let id = args.str("id")?.trim();
    let Ok(entry) = live_entry(id) else {
        return Ok(json!({"id": id, "deleted": 0, "message": "Already deleted"}));
    };
    storage::delete(ENTRIES, id)?;
    Ok(json!({
        "id": id,
        "task_id": entry["task_id"],
        "deleted": 1,
        "minutes": entry["minutes"],
        "message": "Time entry deleted",
    }))
}

/// `restore_time_entry {id}`: Undo for `delete_time_entry`. Restoring one
/// that is live changes nothing.
pub fn restore_time_entry(args: &Args) -> Result<Value> {
    let id = args.str("id")?.trim();
    let rows = storage::query(
        "SELECT _deleted IS NOT NULL AS trashed FROM time_entries WHERE id = ?1",
        &[json!(id)],
    )?;
    let row = rows
        .first()
        .ok_or_else(|| Error::new("not-found", "That time entry no longer exists"))?;
    let trashed = row
        .get("trashed")
        .is_some_and(|v| v.as_bool().unwrap_or_else(|| v.as_i64() == Some(1)));
    if trashed {
        storage::restore(ENTRIES, id)?;
    }
    Ok(json!({"id": id, "restored": usize::from(trashed)}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minutes(text: &str) -> i64 {
        parse_duration(text).unwrap_or_else(|e| panic!("{text}: {}", e.message))
    }

    #[test]
    fn durations_in_every_supported_shape() {
        for (text, want) in [
            ("45", 45),
            ("  90m ", 90),
            ("90 min", 90),
            ("1.5h", 90),
            ("1h 30m", 90),
            ("1H30M", 90),
            ("1h30", 90),
            ("1h 30", 90),
            ("2 hours 15 minutes", 135),
            ("2h, 15m", 135),
            ("1:30", 90),
            ("0:45", 45),
            ("0.5h", 30),
            ("100h", 6000),
        ] {
            assert_eq!(minutes(text), want, "{text}");
        }
    }

    #[test]
    fn garbage_is_rejected_with_the_accepted_formats() {
        for text in [
            "",
            "   ",
            "abc",
            "h",
            "1x",
            "1h 1h",
            "30m 1h",
            "1.5",
            "1.2.3h",
            "-5",
            "1:75",
            "1:",
            ":30",
            "0",
            "0m",
            "0.4m",
            "1001h",
            "1,5h",
            "٣٠",
            "9999999999",
            "1h30m5",
            "1 30",
        ] {
            let err = parse_duration(text)
                .err()
                .unwrap_or_else(|| panic!("accepted {text:?}"));
            assert_eq!(err.code, "invalid", "{text}");
            assert!(!err.message.is_empty());
        }
        let message = parse_duration("soon")
            .err()
            .map(|e| e.message)
            .unwrap_or_default();
        assert!(
            message.contains("1h 30m") && message.contains("soon"),
            "{message}"
        );
    }

    #[test]
    fn stopping_rounds_to_whole_minutes_with_a_minimum_of_one() {
        for (seconds, shown, stored) in [
            (-5, 0, 1),
            (0, 0, 1),
            (29, 0, 1),
            (30, 1, 1),
            (89, 1, 1),
            (90, 2, 2),
            (125, 2, 2),
            (3600, 60, 60),
        ] {
            assert_eq!(elapsed(seconds), shown, "{seconds}s shown");
            assert_eq!(stopped_minutes(seconds), stored, "{seconds}s stored");
        }
    }

    #[test]
    fn minutes_read_back_as_hours_and_minutes() {
        assert_eq!(format_minutes(0), "0m");
        assert_eq!(format_minutes(45), "45m");
        assert_eq!(format_minutes(60), "1h");
        assert_eq!(format_minutes(80), "1h 20m");
        assert_eq!(format_minutes(3000), "50h");
    }
}
