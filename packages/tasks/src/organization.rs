//! Organization writes. All calls share the host operation transaction.
use serde_json::{json, Map, Value};
use terrarium_guest::{storage, Args, Error, Result};

type Row = Map<String, Value>;

pub fn category(row: &Row) -> &'static str {
    if truth(row.get("done")) {
        "completed"
    } else if truth(row.get("in_progress")) {
        "active"
    } else {
        "not_started"
    }
}

fn truth(value: Option<&Value>) -> bool {
    value.is_some_and(|v| v.as_bool().unwrap_or_else(|| v.as_i64() == Some(1)))
}

fn reference(row: &Row, key: &str) -> Value {
    row.get(key).cloned().unwrap_or(Value::Null)
}

fn task(id: &str) -> Result<Row> {
    storage::query(
        "SELECT * FROM tasks WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| Error::invalid("That task no longer exists"))
}

fn definition(table: &str, id: &str) -> Result<Row> {
    // Table is selected only by internal callers, never interpolated from input.
    storage::query(
        &format!("SELECT * FROM {table} WHERE id = ?1 AND _deleted IS NULL"),
        &[json!(id)],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| Error::invalid("That organization item no longer exists"))
}

fn validate_list(list: &Value) -> Result<()> {
    if let Some(id) = list.as_str() {
        if truth(definition("task_lists", id)?.get("archived")) {
            return Err(Error::invalid("Choose an active list"));
        }
    }
    Ok(())
}

fn scoped(table: &str, id: &str, list: &Value) -> Result<Row> {
    let row = definition(table, id)?;
    if reference(&row, "list_id") != *list || truth(row.get("archived")) {
        return Err(Error::invalid(
            "Choose an active item belonging to the destination list",
        ));
    }
    Ok(row)
}

fn optional_id(args: &Args, key: &str) -> Result<Value> {
    let value = args.str(key)?.trim();
    Ok(if value.is_empty() {
        Value::Null
    } else {
        json!(value)
    })
}

fn attributes(args: &Args, creating: bool) -> Result<Row> {
    let mut values = Row::new();
    if creating || args.get("name").is_some() {
        let name = args.str("name")?.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(Error::invalid("Name must contain 1 to 100 characters"));
        }
        values.insert("name".into(), json!(name));
    }
    if let Some(color) = args.opt_str("color") {
        let valid = color.is_empty()
            || (color.len() == 7
                && color.starts_with('#')
                && color[1..].bytes().all(|c| c.is_ascii_hexdigit()));
        if !valid {
            return Err(Error::invalid("Color must be #RRGGBB or empty"));
        }
        values.insert(
            "color".into(),
            if color.is_empty() {
                Value::Null
            } else {
                json!(color.to_ascii_lowercase())
            },
        );
    }
    if let Some(position) = args.get("position") {
        let n = position
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or_else(|| Error::invalid("Position must be a nonnegative integer"))?;
        values.insert("position".into(), json!(n));
    }
    if args.get("archived").is_some() {
        values.insert("archived".into(), json!(args.bool("archived")?));
    }
    Ok(values)
}

pub fn save_definition(args: &Args, kind: &str, creating: bool) -> Result<Value> {
    let table = match kind {
        "list" => "task_lists",
        "group" => "task_groups",
        _ => "task_statuses",
    };
    let mut values = attributes(args, creating)?;
    let current = if creating {
        None
    } else {
        Some(definition(table, args.str("id")?)?)
    };
    if creating && kind != "list" {
        let list = if args.get("list_id").is_some() {
            optional_id(args, "list_id")?
        } else {
            Value::Null
        };
        validate_list(&list)?;
        values.insert("list_id".into(), list);
    }
    if kind == "status" {
        if creating || args.get("category").is_some() {
            let category = args.str("category")?;
            if !["not_started", "active", "completed"].contains(&category) {
                return Err(Error::invalid("Unknown status category"));
            }
            if let Some(current) = &current {
                if current.get("category").and_then(Value::as_str) != Some(category)
                    && !storage::query("SELECT id FROM tasks WHERE status_id = ?1 OR previous_status_id = ?1 LIMIT 1", &[json!(args.str("id")?)])?.is_empty() {
                    return Err(Error::invalid("Reassign tasks before changing this status category"));
                }
            }
            values.insert("category".into(), json!(category));
        }
        if args.get("ready").is_some() {
            values.insert("ready".into(), json!(args.bool("ready")?));
        }
    }
    if !creating && values.is_empty() {
        return Err(Error::invalid("Nothing to update"));
    }
    let id = if creating {
        storage::insert(table, &values)?
    } else {
        let id = args.str("id")?;
        storage::update(table, id, &values)?;
        id.to_string()
    };
    if creating && kind == "list" {
        for (position, (name, category, ready)) in [
            ("To pick", "not_started", true),
            ("In progress", "active", true),
            ("Waiting", "not_started", false),
            ("Done", "completed", false),
        ]
        .into_iter()
        .enumerate()
        {
            let defaults = json!({"list_id": id, "name": name, "category": category, "ready": ready, "position": position});
            storage::insert("task_statuses", defaults.as_object().unwrap())?;
        }
    }
    Ok(json!({"id": id}))
}

/// Select a compatible status in the destination list, preferring the old name.
fn default_status(list: &Value, category: &str, name: Option<&str>) -> Result<Option<Row>> {
    Ok(storage::query("SELECT * FROM task_statuses WHERE list_id IS ?1 AND category = ?2 AND _deleted IS NULL AND archived = 0 ORDER BY CASE WHEN name = ?3 THEN 0 ELSE 1 END, CASE WHEN ready = 1 THEN 0 ELSE 1 END, position, id LIMIT 1", &[list.clone(), json!(category), json!(name)])?.into_iter().next())
}

pub fn organize(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let current = task(id)?;
    let old_list = reference(&current, "list_id");
    let list = if args.get("list_id").is_some() {
        optional_id(args, "list_id")?
    } else {
        old_list.clone()
    };
    let moved = list != old_list;
    if args.get("list_id").is_some() {
        validate_list(&list)?;
    }
    let group = if args.get("group_id").is_some() {
        optional_id(args, "group_id")?
    } else if moved {
        Value::Null
    } else {
        reference(&current, "group_id")
    };
    if args.get("group_id").is_some() {
        if let Some(id) = group.as_str() {
            scoped("task_groups", id, &list)?;
        }
    }
    let explicit_status = args.get("status_id").is_some();
    let builtin_category = match args.opt_str("status_id") {
        Some("@not_started") => Some("not_started"),
        Some("@active") => Some("active"),
        Some("@completed") => Some("completed"),
        _ => None,
    };
    let status = if explicit_status {
        let status = optional_id(args, "status_id")?;
        if let Some(category) = builtin_category {
            default_status(&list, category, None)?
        } else {
            status
                .as_str()
                .map(|id| scoped("task_statuses", id, &list))
                .transpose()?
        }
    } else if moved {
        let old = current
            .get("status_id")
            .and_then(Value::as_str)
            .and_then(|id| definition("task_statuses", id).ok());
        default_status(
            &list,
            category(&current),
            old.as_ref()
                .and_then(|row| row.get("name"))
                .and_then(Value::as_str),
        )?
    } else {
        // Unchanged assignments may be archived or awaiting sync repair. Do
        // not block an unrelated group edit or silently clear the reference.
        None
    };
    // Validate every target before completion, events, or recurrence writes.
    let target_category = status
        .as_ref()
        .and_then(|s| s.get("category"))
        .and_then(Value::as_str)
        .unwrap_or(builtin_category.unwrap_or(category(&current)));
    let finishing = target_category == "completed";
    let changes_completion = finishing != (category(&current) == "completed");
    let mut values = Row::new();
    values.insert("list_id".into(), list.clone());
    values.insert("group_id".into(), group.clone());
    if moved {
        values.insert("previous_status_id".into(), Value::Null);
    }
    // Move before completing so a new occurrence inherits the chosen destination.
    storage::update("tasks", id, &values)?;
    let completion = if changes_completion {
        super::toggle(&Args(
            json!({"id": id, "done": finishing})
                .as_object()
                .unwrap()
                .clone(),
        ))?
    } else {
        Value::Null
    };
    let status_id = if explicit_status || moved {
        let status_id = status
            .as_ref()
            .map(|s| s["id"].clone())
            .unwrap_or(Value::Null);
        values.insert("status_id".into(), status_id.clone());
        values.insert("in_progress".into(), json!(target_category == "active"));
        status_id
    } else {
        reference(&current, "status_id")
    };
    storage::update("tasks", id, &values)?;
    Ok(
        json!({"id": id, "list_id": list, "group_id": group, "status_id": status_id, "completion": completion}),
    )
}

/// Keep the old checkbox/board commands coherent with custom status labels.
pub fn completion_fields(current: &Row, done: bool, values: &mut Row) -> Result<()> {
    // Synthetic old-schema fixtures and old records have no organization fields.
    if !current.contains_key("list_id") {
        return Ok(());
    }
    let list = reference(current, "list_id");
    let restored = if !done {
        current
            .get("previous_status_id")
            .and_then(Value::as_str)
            .and_then(|id| scoped("task_statuses", id, &list).ok())
            .filter(|s| s.get("category").and_then(Value::as_str) != Some("completed"))
    } else {
        None
    };
    let status = match restored {
        Some(status) => Some(status),
        None => default_status(&list, if done { "completed" } else { "not_started" }, None)?,
    };
    if done {
        values.insert("previous_status_id".into(), reference(current, "status_id"));
    } else {
        values.insert("previous_status_id".into(), Value::Null);
    }
    values.insert(
        "status_id".into(),
        status
            .as_ref()
            .map(|s| s["id"].clone())
            .unwrap_or(Value::Null),
    );
    values.insert(
        "in_progress".into(),
        json!(
            status
                .as_ref()
                .and_then(|s| s.get("category"))
                .and_then(Value::as_str)
                == Some("active")
        ),
    );
    Ok(())
}

pub fn repeat_fields(task: &Row, copy: &mut Row) -> Result<()> {
    if !task.contains_key("list_id") {
        return Ok(());
    }
    for key in ["list_id", "group_id"] {
        copy.insert(key.into(), reference(task, key));
    }
    let status = default_status(&reference(task, "list_id"), "not_started", None)?;
    copy.insert(
        "status_id".into(),
        status
            .as_ref()
            .map(|s| s["id"].clone())
            .unwrap_or(Value::Null),
    );
    Ok(())
}

/// Called by existing progress controls; completion continues through toggle.
pub fn progress_fields(current: &Row, active: bool, values: &mut Row) -> Result<()> {
    if !current.contains_key("list_id") {
        return Ok(());
    }
    if category(current) == "completed" {
        if active {
            return Err(Error::invalid("Reopen the task before starting it"));
        }
        return Ok(());
    }
    let status = default_status(
        &reference(current, "list_id"),
        if active { "active" } else { "not_started" },
        None,
    )?;
    values.insert(
        "status_id".into(),
        status
            .as_ref()
            .map(|s| s["id"].clone())
            .unwrap_or(Value::Null),
    );
    Ok(())
}
