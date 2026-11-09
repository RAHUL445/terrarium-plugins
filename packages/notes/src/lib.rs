//! Notes: titled Markdown notes in projects. The body uses the `text` merge
//! rule, so edits made offline on two devices merge instead of one
//! overwriting the other. A line can be sent to To-do as a task.
//!
//! To-do saves what came of finished tasks here, in the `Tasks` project,
//! which the main list leaves out unless it is picked.

use serde_json::{json, Map, Value};
use terrarium_guest::{clock, interop, storage, Args, Error, Plugin, Result};

struct Notes;

const TABLE: &str = "notes";
const MAX_BODY: usize = 200_000;

impl Plugin for Notes {
    fn handle(operation: &str, args: Args) -> Result<Value> {
        match operation {
            "create" => create(&args),
            "update" => update(&args),
            "delete" => {
                storage::delete(TABLE, args.str("id")?)?;
                Ok(Value::Null)
            }
            "restore" => {
                storage::restore(TABLE, args.str("id")?)?;
                Ok(Value::Null)
            }
            "tidy" => tidy(&args),
            "to_task" => to_task(&args),
            "summary" => summary(&args),
            "task_note" => task_note(&args),
            _ => Err(Error::unknown_operation(operation)),
        }
    }
}

fn body(args: &Args) -> Result<Option<String>> {
    match args.opt_str("body") {
        Some(b) if b.chars().count() > MAX_BODY => {
            Err(Error::invalid("Notes are limited to 200,000 characters"))
        }
        Some(b) => Ok(Some(b.replace("\r\n", "\n"))),
        None => Ok(None),
    }
}

fn short(args: &Args, key: &str, max: usize, label: &str) -> Result<Option<String>> {
    match args.opt_str(key).map(str::trim) {
        Some(v) if v.chars().count() > max => Err(Error::invalid(format!(
            "{label} must be {max} characters or fewer"
        ))),
        Some(v) => Ok(Some(v.to_string())),
        None => Ok(None),
    }
}

/// The first non-empty line, without Markdown heading marks.
fn title_from(body: &str) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Untitled");
    let line = line.trim_start_matches('#').trim();
    line.chars().take(120).collect()
}

fn create(args: &Args) -> Result<Value> {
    let body = body(args)?.unwrap_or_default();
    let title = short(args, "title", 200, "Title")?
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| title_from(&body));
    let mut values = Map::new();
    values.insert("title".into(), json!(title));
    values.insert("body".into(), json!(body));
    values.insert("created_at".into(), json!(clock::now_ms() as i64));
    // A widget's own project wins; otherwise the page focus it sits under.
    let project = match short(args, "project", 60, "Project")?.filter(|p| !p.is_empty()) {
        Some(project) => Some(project),
        None => short(args, "focus", 60, "Focus")?.filter(|p| !p.is_empty()),
    };
    if let Some(project) = project {
        values.insert("project".into(), json!(existing_project(&project)?));
    }
    Ok(json!({ "id": storage::insert(TABLE, &values)?, "title": title }))
}

/// An existing project spelled differently ("pages" for "Pages", as a page
/// focus may be typed) keeps its spelling instead of starting a new one.
fn existing_project(project: &str) -> Result<String> {
    let rows = storage::query(
        "SELECT project FROM notes WHERE _deleted IS NULL AND project = ?1 COLLATE NOCASE \
         GROUP BY project ORDER BY COUNT(*) DESC, project LIMIT 1",
        &[json!(project)],
    )?;
    Ok(rows
        .first()
        .and_then(|r| r["project"].as_str())
        .unwrap_or(project)
        .to_string())
}

/// Moves a note to trash if it was left empty (opened as new, nothing typed).
fn tidy(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let rows = storage::query(
        "SELECT title, body FROM notes WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let empty = rows.first().is_some_and(|r| {
        r["body"].as_str().unwrap_or("").trim().is_empty()
            && matches!(r["title"].as_str().unwrap_or(""), "" | "Untitled")
    });
    if empty {
        storage::delete(TABLE, id)?;
    }
    Ok(json!({ "removed": empty }))
}

fn update(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let mut values = Map::new();
    let body = body(args)?;
    if let Some(title) = short(args, "title", 200, "Title")? {
        // A blank title follows the first line of the body.
        let title = if title.is_empty() {
            match &body {
                Some(b) => title_from(b),
                None => {
                    let rows =
                        storage::query("SELECT body FROM notes WHERE id = ?1", &[json!(id)])?;
                    title_from(rows.first().and_then(|r| r["body"].as_str()).unwrap_or(""))
                }
            }
        } else {
            title
        };
        values.insert("title".into(), json!(title));
    }
    if let Some(body) = body {
        values.insert("body".into(), json!(body));
    }
    if let Some(project) = short(args, "project", 60, "Project")? {
        values.insert(
            "project".into(),
            if project.is_empty() {
                Value::Null
            } else {
                json!(project)
            },
        );
    }
    if let Some(pinned) = args.get("pinned").and_then(|v| match v {
        Value::String(s) => Some(s == "true"),
        v => v.as_bool(),
    }) {
        values.insert("pinned".into(), json!(pinned));
    }
    if values.is_empty() {
        return Err(Error::invalid("nothing to update"));
    }
    values.insert("updated_at".into(), json!(clock::now_ms() as i64));
    storage::update(TABLE, id, &values)?;
    Ok(Value::Null)
}

/// Creates a To-do task from a line of text (a leading "- [ ]" is dropped).
fn to_task(args: &Args) -> Result<Value> {
    let line = args.str("text")?.trim();
    let line = line.trim_start_matches(['-', '*']).trim_start();
    let line = line
        .strip_prefix("[ ]")
        .or_else(|| line.strip_prefix("[x]"))
        .unwrap_or(line)
        .trim();
    if line.is_empty() {
        return Err(Error::invalid("Write the task text"));
    }
    if !interop::available("tasks") {
        return Err(Error::new(
            "unavailable",
            "Install the To-do plugin to send tasks",
        ));
    }
    let created = interop::call("tasks", "create", &json!({ "title": line }))?;
    Ok(json!({ "task": created["id"] }))
}

/// The project task notes live in.
const TASKS_PROJECT: &str = "Tasks";

/// Saves an entry in a task's note: one note per `key`, newest entry first.
/// With `replace`, an entry with the same date is replaced (the note of
/// that completion was edited); otherwise a new entry is added.
fn task_note(args: &Args) -> Result<Value> {
    let key = args.str("key")?;
    let date = args.str("date")?;
    if date.len() != 10 || clock::weekday(date).is_err() {
        return Err(Error::invalid("date must be YYYY-MM-DD"));
    }
    let text = args.str("text")?.replace("\r\n", "\n");
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::invalid("Write what came of the task"));
    }
    if text.chars().count() > 20_000 {
        return Err(Error::invalid(
            "A task note is limited to 20,000 characters",
        ));
    }
    let title = short(args, "title", 200, "Title")?
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Task".into());
    let tags: Vec<String> = args
        .opt_str("tags")
        .unwrap_or("")
        .split(',')
        .map(|t| t.trim().trim_start_matches('#'))
        .filter(|t| !t.is_empty())
        .map(|t| format!("#{t}"))
        .collect();
    let heading = format!("## {date}");
    let entry = format!("{heading}\n{text}");
    let existing = storage::query(
        "SELECT id, body FROM notes WHERE task_key = ?1 AND _deleted IS NULL ORDER BY created_at LIMIT 1",
        &[json!(key)],
    )?;
    let now = clock::now_ms() as i64;
    if let Some(row) = existing.first() {
        let id = row["id"].as_str().unwrap_or_default().to_string();
        let body = row["body"].as_str().unwrap_or("");
        // Header (up to the first entry), then entries.
        let (head, rest) = match body.find("\n## ") {
            Some(i) => (&body[..i], &body[i + 1..]),
            None if body.starts_with("## ") => ("", body),
            None => (body, ""),
        };
        let mut entries: Vec<&str> = Vec::new();
        let mut start = 0;
        let bytes = rest.as_bytes();
        for i in 0..rest.len() {
            if i > 0 && bytes[i - 1] == b'\n' && rest[i..].starts_with("## ") {
                entries.push(rest[start..i].trim_end());
                start = i;
            }
        }
        if !rest.is_empty() {
            entries.push(rest[start..].trim_end());
        }
        if args
            .get("replace")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            if let Some(i) = entries
                .iter()
                .position(|e| e.lines().next() == Some(heading.as_str()))
            {
                entries.remove(i);
            }
        }
        let mut body = head.trim_end().to_string();
        for part in std::iter::once(entry.as_str()).chain(entries) {
            body.push_str("\n\n");
            body.push_str(part);
        }
        let mut values = Map::new();
        values.insert("body".into(), json!(body.trim_start()));
        values.insert("title".into(), json!(title));
        values.insert("updated_at".into(), json!(now));
        storage::update(TABLE, &id, &values)?;
        return Ok(json!({ "id": id, "created": false }));
    }
    let mut meta = "_From To-do".to_string();
    if !tags.is_empty() {
        meta.push_str(" · ");
        meta.push_str(&tags.join(" "));
    }
    meta.push('_');
    let mut values = Map::new();
    values.insert("title".into(), json!(title));
    values.insert("body".into(), json!(format!("{meta}\n\n{entry}")));
    values.insert("project".into(), json!(TASKS_PROJECT));
    values.insert("task_key".into(), json!(key));
    values.insert("created_at".into(), json!(now));
    Ok(json!({ "id": storage::insert(TABLE, &values)?, "created": true }))
}

/// Notes created or changed between local dates, for other plugins. Task
/// notes are left out; To-do reports those with the tasks.
fn summary(args: &Args) -> Result<Value> {
    let from = clock::local_to_utc(args.str("start")?, "00:00", "")? as i64;
    let until = clock::local_to_utc(&clock::add_days(args.str("end")?, 1)?, "00:00", "")? as i64;
    let created = storage::query(
        "SELECT title FROM notes WHERE _deleted IS NULL AND COALESCE(project, '') <> 'Tasks' AND created_at >= ?1 AND created_at < ?2 ORDER BY created_at",
        &[json!(from), json!(until)],
    )?;
    let updated = storage::query(
        "SELECT COUNT(*) AS n FROM notes WHERE _deleted IS NULL AND COALESCE(project, '') <> 'Tasks' AND updated_at >= ?1 AND updated_at < ?2",
        &[json!(from), json!(until)],
    )?;
    Ok(json!({
        "created": created.iter().map(|r| r["title"].clone()).collect::<Vec<_>>(),
        "updated": updated.first().map(|r| r["n"].clone()).unwrap_or(json!(0)),
    }))
}

terrarium_guest::plugin!(Notes);
