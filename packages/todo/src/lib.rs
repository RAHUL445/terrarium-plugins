//! To-do plugin: tasks with due dates, reminders, priority, repeats, tags
//! and steps. Lists are host-run queries declared in the manifest; this
//! component handles the write operations.
//!
//! Finishing a task can save a note of what came of it (`done_note`). It is
//! also kept in Notes, in the Tasks project: one note per task, or per
//! repeating series, with an entry for each time it was done.

use serde_json::{json, Map, Value};
use terrarium_guest::{clock, interop, storage, Args, Error, Plugin, Result};

mod quick;

struct Todo;

const TABLE: &str = "tasks";
const STEPS: &str = "steps";

impl Plugin for Todo {
    fn handle(operation: &str, args: Args) -> Result<Value> {
        match operation {
            "create" => create(&args),
            "update" => update(&args),
            "toggle" => toggle(&args),
            "delete" => {
                storage::delete(TABLE, args.str("id")?)?;
                Ok(Value::Null)
            }
            "restore" => {
                storage::restore(TABLE, args.str("id")?)?;
                Ok(Value::Null)
            }
            "clear_done" => clear_done(),
            "add_step" => add_step(&args),
            "toggle_step" => {
                let mut values = Map::new();
                values.insert("done".into(), json!(args.bool("done")?));
                storage::update(STEPS, args.str("id")?, &values)?;
                Ok(Value::Null)
            }
            "delete_step" => {
                storage::delete(STEPS, args.str("id")?)?;
                Ok(Value::Null)
            }
            "completed_between" => completed_between(&args),
            "open_tasks" => open_tasks(&args),
            "save_note" => save_note(&args),
            "note_settings" => note_settings(&args),
            _ => Err(Error::unknown_operation(operation)),
        }
    }
}

/// Creates a task. Dates, times, `#tags`, `!priority` and "every …" in the
/// title are read out of it; explicit `due`, `tags` and `notes` win.
fn create(args: &Args) -> Result<Value> {
    let today = clock::today("")?;
    let parsed = quick::parse(args.str("title")?, &today);
    let title = clean_title(&parsed.title)?;
    let mut values = Map::new();
    values.insert("title".into(), json!(title));

    let time = match args.opt_str("remind_time").filter(|t| !t.trim().is_empty()) {
        Some(t) => Some(clean_time(t)?),
        None => parsed.time,
    };
    let repeat = optional_repeat(args)?.or(parsed.repeat.map(str::to_string));
    let mut due = optional_date(args, "due")?.or(parsed.due);
    if due.is_none() && repeat.is_some() {
        due = Some(today.clone());
    }
    if let (None, Some(time)) = (&due, &time) {
        // A time alone means the next time it comes round.
        let at_today = clock::local_to_utc(&today, time, "")?;
        due = Some(if at_today > clock::now_ms() {
            today.clone()
        } else {
            clock::add_days(&today, 1)?
        });
    }
    if let Some(due) = &due {
        values.insert("due".into(), json!(due));
    }
    if let (Some(due), Some(time)) = (&due, &time) {
        values.insert("remind_time".into(), json!(time));
        values.insert(
            "remind_at".into(),
            json!(clock::local_to_utc(due, time, "")? as i64),
        );
    }

    let mut tags = parsed.tags.join(", ");
    if let Some(extra) = args.opt_str("tags") {
        tags = format!("{tags}, {extra}");
    }
    let tags = normalize_tags(&tags);
    if !tags.is_empty() {
        values.insert("tags".into(), json!(tags));
    }
    if let Some(p) = optional_priority(args)?.or(parsed.priority) {
        values.insert("priority".into(), json!(p));
    }
    if let Some(repeat) = repeat {
        values.insert("repeat".into(), json!(repeat));
    }
    if let Some(notes) = args
        .opt_str("notes")
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        values.insert("notes".into(), json!(notes));
    }
    attention_fields(args, &mut values)?;
    let id = storage::insert(TABLE, &values)?;
    Ok(json!({ "id": id, "title": title, "due": due }))
}

/// Changes any of title, notes, due, remind_time, priority, repeat, tags.
/// An empty value clears the field.
fn update(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let current = storage::query(
        "SELECT due, remind_time FROM tasks WHERE id = ?1",
        &[json!(id)],
    )?;
    let current = current
        .first()
        .ok_or_else(|| Error::invalid("That task no longer exists"))?;
    let mut values = Map::new();
    if let Some(title) = args.opt_str("title") {
        values.insert("title".into(), json!(clean_title(title)?));
    }
    let mut due = current["due"].as_str().map(str::to_string);
    if args.get("due").is_some() {
        due = optional_date(args, "due")?;
        values.insert("due".into(), due.clone().map_or(Value::Null, |d| json!(d)));
    }
    let mut time = current["remind_time"].as_str().map(str::to_string);
    if let Some(t) = args.opt_str("remind_time") {
        time = (!t.trim().is_empty()).then(|| clean_time(t)).transpose()?;
        values.insert(
            "remind_time".into(),
            time.clone().map_or(Value::Null, |t| json!(t)),
        );
        if due.is_none() && time.is_some() {
            due = Some(clock::today("")?);
            values.insert("due".into(), json!(due));
        }
    }
    if values.contains_key("due") || values.contains_key("remind_time") {
        let at = match (&due, &time) {
            (Some(d), Some(t)) => json!(clock::local_to_utc(d, t, "")? as i64),
            _ => Value::Null,
        };
        values.insert("remind_at".into(), at);
    }
    if args.get("priority").is_some() {
        values.insert(
            "priority".into(),
            json!(optional_priority(args)?.unwrap_or(0)),
        );
    }
    if args.get("repeat").is_some() {
        values.insert(
            "repeat".into(),
            optional_repeat(args)?.map_or(Value::Null, |r| json!(r)),
        );
    }
    if let Some(tags) = args.opt_str("tags") {
        let tags = normalize_tags(tags);
        values.insert(
            "tags".into(),
            if tags.is_empty() {
                Value::Null
            } else {
                json!(tags)
            },
        );
    }
    if let Some(notes) = args.opt_str("notes") {
        let notes = notes.trim();
        values.insert(
            "notes".into(),
            if notes.is_empty() {
                Value::Null
            } else {
                json!(notes)
            },
        );
    }
    attention_fields(args, &mut values)?;
    if values.is_empty() {
        return Err(Error::invalid("nothing to update"));
    }
    storage::update(TABLE, id, &values)?;
    Ok(Value::Null)
}

/// Explicit classification: independent of deadline and priority.
fn attention_fields(args: &Args, values: &mut Map<String, Value>) -> Result<()> {
    for field in ["in_progress", "urgent", "important"] {
        if args.get(field).is_some() {
            values.insert(field.into(), json!(args.bool(field)?));
        }
    }
    Ok(())
}

/// Marks a task done or not done. Finishing a repeating task also creates
/// its next occurrence, steps unticked.
fn toggle(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let done = args.bool("done")?;
    let current = storage::query(
        "SELECT done FROM tasks WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let current = current
        .first()
        .ok_or_else(|| Error::new("not-found", "That task no longer exists"))?;
    // Retried completion must not generate another repeating occurrence.
    if current["done"]
        .as_bool()
        .unwrap_or_else(|| current["done"].as_i64() == Some(1))
        == done
    {
        return Ok(Value::Null);
    }
    let mut values = Map::new();
    values.insert("done".into(), json!(done));
    values.insert("in_progress".into(), json!(false));
    values.insert(
        "completed_at".into(),
        if done {
            json!(clock::now_ms() as i64)
        } else {
            Value::Null
        },
    );
    if !done {
        values.insert("cleared_at".into(), Value::Null);
    }
    storage::update(TABLE, id, &values)?;
    if !done {
        return Ok(Value::Null);
    }
    let ask_note = asks_note(id)?.then(|| id.to_string());
    let rows = storage::query(
        "SELECT title, notes, due, tags, priority, repeat, remind_time, urgent, important, COALESCE(series, id) AS series FROM tasks \
         WHERE id = ?1 AND repeat IS NOT NULL",
        &[json!(id)],
    )?;
    let Some(task) = rows.first() else {
        return Ok(json!({ "ask_note": ask_note }));
    };
    let today = clock::today("")?;
    let repeat = task["repeat"].as_str().unwrap_or_default();
    let Some(next) = quick::next_due(repeat, task["due"].as_str().unwrap_or(&today), &today) else {
        return Ok(json!({ "ask_note": ask_note }));
    };
    let mut copy = Map::new();
    for key in [
        "title",
        "notes",
        "tags",
        "priority",
        "repeat",
        "remind_time",
        "series",
    ] {
        if !task[key].is_null() {
            copy.insert(key.into(), task[key].clone());
        }
    }
    for field in ["urgent", "important"] {
        copy.insert(
            field.into(),
            json!(task[field]
                .as_bool()
                .unwrap_or_else(|| task[field].as_i64() == Some(1))),
        );
    }
    if let Some(time) = task["remind_time"].as_str() {
        copy.insert(
            "remind_at".into(),
            json!(clock::local_to_utc(&next, time, "")? as i64),
        );
    }
    copy.insert("due".into(), json!(next));
    let new_id = storage::insert(TABLE, &copy)?;
    let steps = storage::query(
        "SELECT title FROM steps WHERE task_id = ?1 AND _deleted IS NULL ORDER BY id",
        &[json!(id)],
    )?;
    for step in &steps {
        let mut values = Map::new();
        values.insert("task_id".into(), json!(new_id));
        values.insert("title".into(), step["title"].clone());
        storage::insert(STEPS, &values)?;
    }
    Ok(json!({ "next": { "id": new_id, "due": next }, "ask_note": ask_note }))
}

/// Whether finishing task `id` should offer to save a note, per the
/// note settings: `always` (default), `tagged` (with one of the listed
/// tags) or `never`.
fn asks_note(id: &str) -> Result<bool> {
    let settings = storage::query(
        "SELECT ask_note, note_tags FROM settings WHERE _deleted IS NULL ORDER BY id LIMIT 1",
        &[],
    )?;
    let ask = settings
        .first()
        .and_then(|r| r["ask_note"].as_str())
        .unwrap_or("always");
    match ask {
        "never" => Ok(false),
        "tagged" => {
            let wanted = normalize_tags(
                settings
                    .first()
                    .and_then(|r| r["note_tags"].as_str())
                    .unwrap_or(""),
            );
            let rows = storage::query("SELECT tags FROM tasks WHERE id = ?1", &[json!(id)])?;
            let tags = normalize_tags(rows.first().and_then(|r| r["tags"].as_str()).unwrap_or(""));
            Ok(tags
                .split(", ")
                .any(|t| !t.is_empty() && wanted.split(", ").any(|w| w == t)))
        }
        _ => Ok(true),
    }
}

fn note_settings(args: &Args) -> Result<Value> {
    let ask = args.opt_str("ask").unwrap_or("always");
    if !matches!(ask, "always" | "tagged" | "never") {
        return Err(Error::invalid("ask must be always, tagged or never"));
    }
    let tags = normalize_tags(args.opt_str("tags").unwrap_or(""));
    if ask == "tagged" && tags.is_empty() {
        return Err(Error::invalid(
            "List the tags whose tasks should ask for a note",
        ));
    }
    let mut values = Map::new();
    values.insert("ask_note".into(), json!(ask));
    values.insert("note_tags".into(), json!(tags));
    // One settings row; the oldest wins if two devices made one offline.
    let rows = storage::query(
        "SELECT id FROM settings WHERE _deleted IS NULL ORDER BY id LIMIT 1",
        &[],
    )?;
    match rows.first().and_then(|r| r["id"].as_str()) {
        Some(id) => storage::update("settings", id, &values)?,
        None => {
            storage::insert("settings", &values)?;
        }
    }
    Ok(Value::Null)
}

/// Saves what came of a finished task: on the task, and in its note in
/// Notes (when installed). An empty text removes the task's own note.
fn save_note(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let text = args.opt_str("text").unwrap_or("").replace("\r\n", "\n");
    let text = text.trim();
    if text.chars().count() > 20_000 {
        return Err(Error::invalid(
            "A task note is limited to 20,000 characters",
        ));
    }
    let rows = storage::query(
        "SELECT title, tags, completed_at, done, done_note, COALESCE(series, id) AS series FROM tasks WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let task = rows
        .first()
        .ok_or_else(|| Error::invalid("That task no longer exists"))?;
    let mut values = Map::new();
    values.insert(
        "done_note".into(),
        if text.is_empty() {
            Value::Null
        } else {
            json!(text)
        },
    );
    storage::update(TABLE, id, &values)?;
    if text.is_empty() {
        return Ok(json!({ "saved": false }));
    }
    if !interop::available("notes") {
        return Ok(json!({ "saved": true, "note": null }));
    }
    let at = task["completed_at"].as_u64().unwrap_or_else(clock::now_ms);
    let note = interop::call(
        "notes",
        "task_note",
        &json!({
            "key": task["series"],
            "title": task["title"],
            "date": clock::local_date(at, "")?,
            "text": text,
            "tags": task["tags"],
            "replace": task["done_note"].is_string(),
        }),
    )?;
    let mut values = Map::new();
    values.insert("note_id".into(), note["id"].clone());
    storage::update(TABLE, id, &values)?;
    Ok(json!({ "saved": true, "note": note["id"] }))
}

/// Open tasks, for reviews: overdue, due by `end`, and undated.
fn open_tasks(args: &Args) -> Result<Value> {
    let today = args
        .opt_str("today")
        .map(str::to_string)
        .unwrap_or(clock::today("")?);
    let end = args
        .opt_str("end")
        .map(str::to_string)
        .unwrap_or(clock::add_days(&today, 7)?);
    let rows = storage::query(
        "SELECT id, title, tags, due, priority, CASE WHEN due < ?1 THEN 1 ELSE 0 END AS overdue FROM tasks \
         WHERE _deleted IS NULL AND done = 0 AND (due IS NULL OR due <= ?2) \
         ORDER BY due IS NULL, due, priority DESC LIMIT 200",
        &[json!(today), json!(end)],
    )?;
    Ok(json!({ "tasks": rows }))
}

/// Hides finished tasks from the lists. They stay in the data, so reviews
/// of completed work still count them.
fn clear_done() -> Result<Value> {
    let rows = storage::query(
        "SELECT id FROM tasks WHERE _deleted IS NULL AND done = 1 AND cleared_at IS NULL",
        &[],
    )?;
    let mut values = Map::new();
    values.insert("cleared_at".into(), json!(clock::now_ms() as i64));
    for row in &rows {
        storage::update(TABLE, row["id"].as_str().unwrap_or_default(), &values)?;
    }
    Ok(json!({ "cleared": rows.len() }))
}

fn add_step(args: &Args) -> Result<Value> {
    let task = args.str("task_id")?;
    let title = clean_title(args.str("title")?)?;
    let mut values = Map::new();
    values.insert("task_id".into(), json!(task));
    values.insert("title".into(), json!(title));
    let id = storage::insert(STEPS, &values)?;
    Ok(json!({ "id": id }))
}

/// Tasks completed on local dates `start..=end`, for other plugins.
fn completed_between(args: &Args) -> Result<Value> {
    let start = args.str("start")?;
    let end = args.str("end")?;
    let from = clock::local_to_utc(start, "00:00", "")?;
    let until = clock::local_to_utc(&clock::add_days(end, 1)?, "00:00", "")?;
    let rows = storage::query(
        "SELECT id, title, tags, completed_at, done_note FROM tasks WHERE _deleted IS NULL AND done = 1 \
         AND completed_at >= ?1 AND completed_at < ?2 ORDER BY completed_at",
        &[json!(from as i64), json!(until as i64)],
    )?;
    Ok(json!({ "tasks": rows }))
}

fn clean_title(title: &str) -> Result<String> {
    let title = title.trim();
    if title.is_empty() {
        return Err(Error::invalid("Title is required"));
    }
    if title.chars().count() > 500 {
        return Err(Error::invalid("Title must be 500 characters or fewer"));
    }
    Ok(title.to_string())
}

fn clean_time(time: &str) -> Result<String> {
    let time = time.trim();
    let bytes = time.as_bytes();
    let ok = bytes.len() == 5
        && bytes[2] == b':'
        && [bytes[0], bytes[1], bytes[3], bytes[4]]
            .iter()
            .all(u8::is_ascii_digit)
        && (bytes[0] - b'0') * 10 + bytes[1] - b'0' < 24
        && (bytes[3] - b'0') * 10 + bytes[4] - b'0' < 60;
    if !ok {
        return Err(Error::invalid("Reminder time must look like 18:30"));
    }
    Ok(time.to_string())
}

/// 0 (none) to 3 (high), as a number or a numeric string.
fn optional_priority(args: &Args) -> Result<Option<i64>> {
    let value = match args.get("priority") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => return Ok(None),
        Some(Value::String(s)) => s.trim().parse::<i64>().ok(),
        Some(v) => v.as_i64(),
    };
    match value {
        Some(p @ 0..=3) => Ok(Some(p)),
        _ => Err(Error::invalid("Priority must be 0 to 3")),
    }
}

fn optional_repeat(args: &Args) -> Result<Option<String>> {
    match args.opt_str("repeat").map(str::trim) {
        None | Some("") | Some("never") => Ok(None),
        Some(r) if quick::REPEATS.contains(&r) => Ok(Some(r.to_string())),
        Some(_) => Err(Error::invalid(
            "Repeat must be daily, weekdays, weekly or monthly",
        )),
    }
}

/// Accepts `YYYY-MM-DD` or an empty value (meaning no due date).
fn optional_date(args: &Args, key: &str) -> Result<Option<String>> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    let text = match value {
        Value::Null => return Ok(None),
        Value::String(s) if s.trim().is_empty() => return Ok(None),
        Value::String(s) => s.trim(),
        _ => return Err(Error::invalid(format!("`{key}` must be a date"))),
    };
    if quick::to_days(text).is_none() {
        return Err(Error::invalid(format!(
            "`{key}` must be a date like 2026-11-01"
        )));
    }
    Ok(Some(text.to_string()))
}

/// Lower-cased, trimmed, de-duplicated, comma-separated.
fn normalize_tags(tags: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for tag in tags
        .split(',')
        .map(|t| t.trim().trim_start_matches('#').to_lowercase())
        .filter(|t| !t.is_empty())
    {
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    out.join(", ")
}

terrarium_guest::plugin!(Todo);

#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn explicit_reminder_times_require_ascii_clock_digits() {
        for invalid in ["+1:00", "01:+1", "24:00", "12:60", "é:00", "0é:0"] {
            assert!(clean_time(invalid).is_err(), "{invalid}");
        }
        assert_eq!(clean_time("09:30").unwrap(), "09:30");
    }
}
