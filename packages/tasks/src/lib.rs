//! Task manager plugin (id `tasks`, stored as `todo`): tasks with due dates, reminders, priority, repeats, tags,
//! steps, subtasks (one level), dependencies, custom fields, time estimates and
//! time tracking. Lists are host-run queries
//! declared in the manifest; this component handles the write operations.
//!
//! Finished tasks can be archived (`archive.rs`), which hides them from the
//! lists but keeps them in the Archive and in reviews.
//!
//! Finishing a task can save a note of what came of it (`done_note`). It is
//! also kept in Notes, in the Tasks project: one note per task, or per
//! repeating series, with an entry for each time it was done.

use serde_json::{json, Map, Value};
use terrarium_guest::{clock, events, interop, storage, Args, Error, Plugin, Result};

mod archive;
mod fields;
mod organization;
mod quick;
mod structure;
mod time;

struct Todo;

pub(crate) const TABLE: &str = "tasks";
pub(crate) const STEPS: &str = "steps";

impl Plugin for Todo {
    fn handle(operation: &str, args: Args) -> Result<Value> {
        match operation {
            "create" => create(&args),
            "import" => import(&args),
            "update" => update(&args),
            "move" => move_task(&args),
            "organize" => organization::organize(&args),
            "create_list" => organization::save_definition(&args, "list", true),
            "update_list" => organization::save_definition(&args, "list", false),
            "create_group" => organization::save_definition(&args, "group", true),
            "update_group" => organization::save_definition(&args, "group", false),
            "create_status" => organization::save_definition(&args, "status", true),
            "update_status" => organization::save_definition(&args, "status", false),
            "add_tag" => add_tag(&args),
            "toggle" => toggle(&args),
            "delete" => structure::trash(args.str("id")?),
            "restore" => structure::restore(args.str("id")?),
            "set_parent" => structure::set_parent(&args),
            "promote_step" => structure::promote_step(&args),
            "unpromote_step" => structure::unpromote_step(&args),
            "add_dependency" => structure::add_dependency(&args),
            "remove_dependency" => structure::remove_dependency(&args),
            "create_field" => fields::create_field(&args),
            "update_field" => fields::update_field(&args),
            "add_field_option" => fields::add_field_option(&args),
            "update_field_option" => fields::update_field_option(&args),
            "remove_field_option" => fields::remove_field_option(&args),
            "set_field" => fields::set_field(&args),
            "set_estimate" => time::set_estimate(&args),
            "start_timer" => time::start_timer(&args),
            "stop_timer" => time::stop_timer(),
            "resume_timer" => time::resume_timer(&args),
            "undo_start_timer" => time::undo_start_timer(&args),
            "add_time" => time::add_time(&args),
            "update_time_entry" => time::update_time_entry(&args),
            "delete_time_entry" => time::delete_time_entry(&args),
            "restore_time_entry" => time::restore_time_entry(&args),
            "clear_done" | "archive_done" => archive::archive_done(),
            "archive" => archive::archive(&args),
            "unarchive" => archive::unarchive(&args),
            "archive_settings" => archive::archive_settings(&args),
            "auto_archive" => archive::auto_archive(&args),
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
/// title are read out of it; explicit `due`, `tags` and `notes` win. With
/// `parent_id` it is a subtask, in its parent's list and group unless given.
fn create(args: &Args) -> Result<Value> {
    create_task(args, true)
}

/// Imports a literal title and preserves whether the task is finished.
fn import(args: &Args) -> Result<Value> {
    let out = create_task(args, false)?;
    if args.get("done").and_then(Value::as_bool) == Some(true) {
        let mut values = Map::new();
        values.insert("done".into(), json!(true));
        values.insert("in_progress".into(), json!(false));
        let current = storage::query("SELECT * FROM tasks WHERE id = ?1", &[out["id"].clone()])?;
        organization::completion_fields(&current[0], true, &mut values)?;
        values.insert("completed_at".into(), json!(clock::now_ms() as i64));
        storage::update(TABLE, out["id"].as_str().unwrap_or_default(), &values)?;
    }
    Ok(out)
}

/// Creates a task, optionally reading quick-add fields out of its title.
fn create_task(args: &Args, parse: bool) -> Result<Value> {
    let today = clock::today("")?;
    let parsed = if parse {
        quick::parse(args.str("title")?, &today)
    } else {
        quick::Parsed {
            title: args.str("title")?.to_string(),
            ..Default::default()
        }
    };
    let title = clean_title(&parsed.title)?;
    let parent = match args
        .opt_str("parent_id")
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        Some(id) => Some(structure::parent_for(id)?),
        None => None,
    };
    let mut values = Map::new();
    values.insert("title".into(), json!(title));
    if let Some(parent) = &parent {
        // Empty organization fields mean "not given" for a subtask.
        let given: Map<String, Value> = args
            .0
            .iter()
            .filter(|(k, v)| {
                !["list_id", "group_id"].contains(&k.as_str())
                    || v.as_str().is_some_and(|s| !s.trim().is_empty())
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        structure::inherit(parent, &Args(given), &mut values);
    }

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
    let mut result = json!({ "id": id, "title": title, "due": due });
    if let Some(parent) = &parent {
        result["parent_id"] = parent["id"].clone();
    }
    // A subtask keeps its inherited list and group unless a value is given.
    let given = |key: &str| match args.get(key) {
        Some(Value::String(s)) if parent.is_some() => !s.trim().is_empty(),
        Some(_) => true,
        None => false,
    };
    if ["list_id", "group_id", "status_id"]
        .iter()
        .any(|key| given(key))
    {
        let mut fields = Map::new();
        fields.insert("id".into(), json!(id));
        for key in ["list_id", "group_id", "status_id"] {
            if given(key) {
                fields.insert(key.into(), args.get(key).cloned().unwrap_or(Value::Null));
            }
        }
        result["organization"] = organization::organize(&Args(fields))?;
    }
    Ok(result)
}

/// Changes any of title, notes, due, remind_time, priority, repeat, tags.
/// An empty value clears the field.
fn update(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let current = storage::query(
        "SELECT * FROM tasks WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let current = current
        .first()
        .ok_or_else(|| Error::invalid("That task no longer exists"))?;
    let mut values = Map::new();
    if args.get("planned_for").is_some() {
        values.insert(
            "planned_for".into(),
            optional_date(args, "planned_for")?.map_or(Value::Null, |d| json!(d)),
        );
    }
    if let Some(order) = args.get("sort_order") {
        let order = order
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or_else(|| Error::invalid("Order must be a nonnegative integer"))?;
        values.insert("sort_order".into(), json!(order));
    }
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
    if args.get("in_progress").is_some() {
        organization::progress_fields(current, args.bool("in_progress")?, &mut values)?;
    }
    if values.is_empty() {
        return Err(Error::invalid("nothing to update"));
    }
    storage::update(TABLE, id, &values)?;
    Ok(Value::Null)
}

/// Adds one tag to a task's tags (normalized, no duplicates).
fn add_tag(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let tag = normalize_tags(args.str("tag")?);
    if tag.is_empty() || tag.contains(',') {
        return Err(Error::invalid("Give one tag"));
    }
    let rows = storage::query(
        "SELECT tags FROM tasks WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let row = rows
        .first()
        .ok_or_else(|| Error::new("not-found", "That task no longer exists"))?;
    let current = row["tags"].as_str().unwrap_or("");
    let tags = normalize_tags(&format!("{current}, {tag}"));
    let mut values = Map::new();
    values.insert("tags".into(), json!(tags));
    storage::update(TABLE, id, &values)?;
    Ok(json!({ "tags": tags }))
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

/// Board lanes set done/in progress (finishing goes through `toggle`, so
/// repeats and notes behave the same); quadrants set urgent/important.
fn move_task(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let lane = args.str("lane")?;
    let rows = storage::query(
        "SELECT done FROM tasks WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let task = rows
        .first()
        .ok_or_else(|| Error::invalid("That task no longer exists"))?;
    let is_done = task["done"]
        .as_bool()
        .unwrap_or_else(|| task["done"].as_i64() == Some(1));
    match lane {
        "todo" | "doing" => {
            if is_done {
                toggle(&Args(
                    json!({"id": id, "done": false})
                        .as_object()
                        .cloned()
                        .unwrap_or_default(),
                ))?;
            }
            let mut values = Map::new();
            values.insert("in_progress".into(), json!(lane == "doing"));
            let current = storage::query("SELECT * FROM tasks WHERE id = ?1", &[json!(id)])?;
            organization::progress_fields(&current[0], lane == "doing", &mut values)?;
            storage::update(TABLE, id, &values)?;
            Ok(Value::Null)
        }
        "done" => toggle(&Args(
            json!({"id": id, "done": true})
                .as_object()
                .cloned()
                .unwrap_or_default(),
        )),
        "do" | "schedule" | "delegate" | "eliminate" => {
            let (urgent, important) = match lane {
                "do" => (true, true),
                "schedule" => (false, true),
                "delegate" => (true, false),
                _ => (false, false),
            };
            let mut values = Map::new();
            values.insert("urgent".into(), json!(urgent));
            values.insert("important".into(), json!(important));
            storage::update(TABLE, id, &values)?;
            Ok(Value::Null)
        }
        _ => Err(Error::invalid("Unknown lane")),
    }
}

/// Marks a task done or not done. Finishing a repeating task also creates
/// its next occurrence, steps unticked. Its subtasks and dependencies are
/// not copied (a repeating subtask's next occurrence stays under its parent).
fn toggle(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let done = args.bool("done")?;
    let message = if done { "Task done" } else { "Task reopened" };
    let current = storage::query(
        "SELECT * FROM tasks WHERE id = ?1 AND _deleted IS NULL",
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
        return Ok(json!({ "message": message }));
    }
    // Finishing a blocked task is allowed; the message says so.
    let blockers = if done {
        structure::open_blockers(id)?
    } else {
        0
    };
    let message = match blockers {
        0 => message.to_string(),
        1 => "Done — was waiting on 1 task".to_string(),
        n => format!("Done — was waiting on {n} tasks"),
    };
    // Finishing a task stops its running timer (the time is kept).
    let message = match done
        .then(|| time::stop_for_tasks(&[id]))
        .transpose()?
        .flatten()
    {
        Some(minutes) => format!(
            "{message} · timer stopped at {}",
            time::format_minutes(minutes)
        ),
        None => message,
    };
    let mut values = Map::new();
    values.insert("done".into(), json!(done));
    values.insert("in_progress".into(), json!(false));
    organization::completion_fields(current, done, &mut values)?;
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
        return Ok(json!({ "message": message }));
    }
    events::emit(
        "task_done",
        &json!({
            "task_id": id,
            "title": current["title"],
            "tags": current["tags"],
            "completed_date": clock::today("")?,
        }),
    )?;
    let ask_note = asks_note(id)?.then(|| id.to_string());
    let rows = storage::query(
        "SELECT *, COALESCE(series, id) AS repeat_series FROM tasks \
         WHERE id = ?1 AND repeat IS NOT NULL",
        &[json!(id)],
    )?;
    let Some(task) = rows.first() else {
        return Ok(json!({ "message": message, "ask_note": ask_note }));
    };
    let today = clock::today("")?;
    let repeat = task["repeat"].as_str().unwrap_or_default();
    let Some(next) = quick::next_due(repeat, task["due"].as_str().unwrap_or(&today), &today) else {
        return Ok(json!({ "message": message, "ask_note": ask_note }));
    };
    let mut copy = Map::new();
    for key in [
        "title",
        "notes",
        "tags",
        "priority",
        "repeat",
        "remind_time",
        "parent_id",
        "estimate_min",
    ] {
        if !task[key].is_null() {
            copy.insert(key.into(), task[key].clone());
        }
    }
    copy.insert("series".into(), task["repeat_series"].clone());
    organization::repeat_fields(task, &mut copy)?;
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
    Ok(json!({ "next": { "id": new_id, "due": next }, "message": message, "ask_note": ask_note }))
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
