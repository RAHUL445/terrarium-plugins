//! Task structure: subtasks (one level, `tasks.parent_id`) and dependencies
//! ("waiting on", `task_deps`). Blocked-ness is never stored; queries derive
//! it from `task_deps` and the blockers' done and trash state.
//!
//! Trashing a task also trashes its live subtasks and marks each with
//! `trashed_with` = the parent's id, so restoring the parent restores exactly
//! the subtasks that went to the trash with it (subtasks trashed on their own
//! before stay in the trash).
use serde_json::{json, Map, Value};
use terrarium_guest::{clock, storage, Args, Error, Result};

use crate::{STEPS, TABLE};

const DEPS: &str = "task_deps";

type Row = Map<String, Value>;

fn truth(value: Option<&Value>) -> bool {
    value.is_some_and(|v| v.as_bool().unwrap_or_else(|| v.as_i64() == Some(1)))
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

/// A task counts as a subtask while its parent row still exists, live or in
/// trash. A parent that is gone entirely leaves an orphan, which is treated
/// as a top-level task everywhere.
fn has_parent(task: &Row) -> Result<bool> {
    let Some(parent) = task.get("parent_id").and_then(Value::as_str) else {
        return Ok(false);
    };
    Ok(!storage::query(
        "SELECT 1 FROM tasks WHERE id = ?1 LIMIT 1",
        &[json!(parent)],
    )?
    .is_empty())
}

fn has_live_children(id: &str) -> Result<bool> {
    Ok(!storage::query(
        "SELECT 1 FROM tasks WHERE parent_id = ?1 AND _deleted IS NULL LIMIT 1",
        &[json!(id)],
    )?
    .is_empty())
}

/// The live task that `parent_id` names, if it may take subtasks.
pub fn parent_for(parent_id: &str) -> Result<Row> {
    let parent = storage::query(
        "SELECT * FROM tasks WHERE id = ?1 AND _deleted IS NULL",
        &[json!(parent_id)],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| Error::new("not-found", "That parent task no longer exists"))?;
    if has_parent(&parent)? {
        return Err(Error::invalid(
            "A subtask can't have subtasks of its own (one level only)",
        ));
    }
    Ok(parent)
}

/// Fields a new subtask takes from its parent unless the caller set them.
pub fn inherit(parent: &Row, args: &Args, values: &mut Row) {
    values.insert("parent_id".into(), parent["id"].clone());
    for key in ["list_id", "group_id"] {
        if args.get(key).is_none() {
            if let Some(value) = parent.get(key).filter(|v| !v.is_null()) {
                values.insert(key.into(), value.clone());
            }
        }
    }
}

/// `set_parent {id, parent_id}`: moves a task under another (into the
/// parent's list and group) or, with an empty parent, detaches it.
pub fn set_parent(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let task = live_task(id)?;
    let previous = task.get("parent_id").cloned().unwrap_or(Value::Null);
    let parent_id = args.opt_str("parent_id").map(str::trim).unwrap_or("");
    let mut values = Row::new();
    if parent_id.is_empty() {
        values.insert("parent_id".into(), Value::Null);
        storage::update(TABLE, id, &values)?;
        return Ok(json!({"id": id, "parent_id": null, "previous_parent_id": previous}));
    }
    if parent_id == id {
        return Err(Error::invalid("A task can't be its own subtask"));
    }
    let parent = parent_for(parent_id)?;
    if has_live_children(id)? {
        return Err(Error::invalid(
            "This task has subtasks, so it can't become a subtask (one level only)",
        ));
    }
    values.insert("parent_id".into(), json!(parent_id));
    storage::update(TABLE, id, &values)?;
    let reference = |row: &Row, key: &str| row.get(key).cloned().unwrap_or(Value::Null);
    if reference(&task, "list_id") != reference(&parent, "list_id")
        || reference(&task, "group_id") != reference(&parent, "group_id")
    {
        let as_arg = |v: Value| if v.is_null() { json!("") } else { v };
        let mut fields = Row::new();
        fields.insert("id".into(), json!(id));
        fields.insert("list_id".into(), as_arg(reference(&parent, "list_id")));
        fields.insert("group_id".into(), as_arg(reference(&parent, "group_id")));
        crate::organization::organize(&Args(fields))?;
    }
    Ok(json!({
        "id": id,
        "parent_id": parent_id,
        "parent_title": parent["title"],
        "previous_parent_id": previous,
    }))
}

/// Moves a task to trash with its live subtasks. Already in trash: no-op, so
/// a selection holding both a parent and its subtask trashes cleanly.
pub fn trash(id: &str) -> Result<Value> {
    if storage::query(
        "SELECT 1 FROM tasks WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?
    .is_empty()
    {
        return Ok(json!({"trashed": 0, "subtasks": 0, "message": "Already in the trash"}));
    }
    let children = storage::query(
        "SELECT id FROM tasks WHERE parent_id = ?1 AND _deleted IS NULL ORDER BY id",
        &[json!(id)],
    )?;
    // A timer can't keep running on a task that is in the trash.
    let mut timed: Vec<&str> = vec![id];
    timed.extend(children.iter().filter_map(|c| c["id"].as_str()));
    crate::time::stop_for_tasks(&timed)?;
    let mut mark = Row::new();
    mark.insert("trashed_with".into(), json!(id));
    for child in &children {
        let child = child["id"].as_str().unwrap_or_default();
        storage::update(TABLE, child, &mark)?;
        storage::delete(TABLE, child)?;
    }
    storage::delete(TABLE, id)?;
    let n = children.len();
    let message = match n {
        0 => "Moved to the trash".to_string(),
        1 => "Moved to the trash with 1 subtask".to_string(),
        n => format!("Moved to the trash with {n} subtasks"),
    };
    Ok(json!({"trashed": n + 1, "subtasks": n, "message": message}))
}

/// Restores a task and the subtasks that were trashed with it. Restoring a
/// live task is a no-op, so undoing a bulk delete works in any order.
pub fn restore(id: &str) -> Result<Value> {
    let rows = storage::query(
        "SELECT id, _deleted IS NOT NULL AS trashed, trashed_with FROM tasks WHERE id = ?1",
        &[json!(id)],
    )?;
    let row = rows
        .first()
        .ok_or_else(|| Error::new("not-found", "That task no longer exists"))?;
    let mut restored = 0;
    let clear = || {
        let mut values = Row::new();
        values.insert("trashed_with".into(), Value::Null);
        values
    };
    if truth(row.get("trashed")) {
        storage::restore(TABLE, id)?;
        restored += 1;
    }
    if !row.get("trashed_with").unwrap_or(&Value::Null).is_null() {
        storage::update(TABLE, id, &clear())?;
    }
    let children = storage::query(
        "SELECT id FROM tasks WHERE trashed_with = ?1 AND _deleted IS NOT NULL ORDER BY id",
        &[json!(id)],
    )?;
    for child in &children {
        let child = child["id"].as_str().unwrap_or_default();
        storage::restore(TABLE, child)?;
        storage::update(TABLE, child, &clear())?;
    }
    Ok(json!({"restored": restored + children.len(), "subtasks": children.len()}))
}

/// `promote_step {step_id}`: turns a step into a subtask of its task (same
/// title and done state) and moves the step to trash.
pub fn promote_step(args: &Args) -> Result<Value> {
    let step_id = args.str("step_id")?;
    let step = storage::query(
        "SELECT task_id, title, done FROM steps WHERE id = ?1 AND _deleted IS NULL",
        &[json!(step_id)],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| Error::new("not-found", "That step no longer exists"))?;
    let task_id = step["task_id"].as_str().unwrap_or_default();
    let parent = parent_for(task_id).map_err(|e| {
        if e.code == "not-found" {
            e
        } else {
            Error::invalid("Steps of a subtask can't become subtasks (one level only)")
        }
    })?;
    let mut values = Row::new();
    values.insert("title".into(), step["title"].clone());
    inherit(&parent, &Args(Row::new()), &mut values);
    let id = storage::insert(TABLE, &values)?;
    if truth(step.get("done")) {
        let current = live_task(&id)?;
        let mut done = Row::new();
        done.insert("done".into(), json!(true));
        done.insert("in_progress".into(), json!(false));
        crate::organization::completion_fields(&current, true, &mut done)?;
        done.insert("completed_at".into(), json!(clock::now_ms() as i64));
        storage::update(TABLE, &id, &done)?;
    }
    storage::delete(STEPS, step_id)?;
    Ok(json!({"id": id, "step_id": step_id, "parent_id": task_id, "title": step["title"]}))
}

/// Undo for `promote_step`: brings the step back and trashes the subtask.
pub fn unpromote_step(args: &Args) -> Result<Value> {
    let step_id = args.str("step_id")?;
    let id = args.str("id")?;
    let step = storage::query(
        "SELECT _deleted IS NOT NULL AS trashed FROM steps WHERE id = ?1",
        &[json!(step_id)],
    )?;
    if step.first().is_some_and(|s| truth(s.get("trashed"))) {
        storage::restore(STEPS, step_id)?;
    }
    trash(id)?;
    Ok(json!({"step_id": step_id}))
}

/// Number of tasks that `id` is still waiting on (live, not done).
pub fn open_blockers(id: &str) -> Result<usize> {
    Ok(storage::query(
        "SELECT DISTINCT d.depends_on FROM task_deps d JOIN tasks b ON b.id = d.depends_on \
         WHERE d.task_id = ?1 AND d._deleted IS NULL AND b._deleted IS NULL AND COALESCE(b.done, 0) = 0",
        &[json!(id)],
    )?
    .len())
}

fn pair(args: &Args) -> Result<(String, String)> {
    let task = args.str("task_id")?.trim().to_string();
    let blocker = args.str("depends_on")?.trim().to_string();
    if task.is_empty() || blocker.is_empty() {
        return Err(Error::invalid("Give task_id and depends_on"));
    }
    Ok((task, blocker))
}

/// `add_dependency {task_id, depends_on}`: `task_id` waits on `depends_on`.
/// Adding a pair that exists already returns it unchanged.
pub fn add_dependency(args: &Args) -> Result<Value> {
    let (task, blocker) = pair(args)?;
    if task == blocker {
        return Err(Error::invalid("A task can't wait on itself"));
    }
    live_task(&task)?;
    let blocking = storage::query(
        "SELECT title FROM tasks WHERE id = ?1 AND _deleted IS NULL",
        &[json!(blocker)],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| Error::new("not-found", "The task to wait on no longer exists"))?;
    if let Some(existing) = storage::query(
        "SELECT id FROM task_deps WHERE task_id = ?1 AND depends_on = ?2 AND _deleted IS NULL ORDER BY id LIMIT 1",
        &[json!(task), json!(blocker)],
    )?
    .first()
    {
        return Ok(json!({"id": existing["id"], "task_id": task, "depends_on": blocker, "title": blocking["title"], "existing": true}));
    }
    // Walk what the blocker waits on, transitively (trashed tasks included,
    // since restoring them brings their dependencies back).
    let cycle = storage::query(
        "WITH RECURSIVE up(id) AS (SELECT ?1 UNION SELECT d.depends_on FROM task_deps d JOIN up ON d.task_id = up.id WHERE d._deleted IS NULL) \
         SELECT 1 FROM up WHERE id = ?2 LIMIT 1",
        &[json!(blocker), json!(task)],
    )?;
    if !cycle.is_empty() {
        return Err(Error::invalid(format!(
            "That would make a loop: “{}” already waits on this task",
            blocking["title"].as_str().unwrap_or("that task")
        )));
    }
    let mut values = Row::new();
    values.insert("task_id".into(), json!(task));
    values.insert("depends_on".into(), json!(blocker));
    let id = storage::insert(DEPS, &values)?;
    Ok(
        json!({"id": id, "task_id": task, "depends_on": blocker, "title": blocking["title"], "existing": false}),
    )
}

/// `remove_dependency {task_id, depends_on}`: removes the pair (including
/// duplicates two devices made offline).
pub fn remove_dependency(args: &Args) -> Result<Value> {
    let (task, blocker) = pair(args)?;
    let rows = storage::query(
        "SELECT id FROM task_deps WHERE task_id = ?1 AND depends_on = ?2 AND _deleted IS NULL",
        &[json!(task), json!(blocker)],
    )?;
    if rows.is_empty() {
        return Err(Error::new("not-found", "That task isn't waiting on it"));
    }
    for row in &rows {
        storage::delete(DEPS, row["id"].as_str().unwrap_or_default())?;
    }
    Ok(json!({"removed": rows.len(), "task_id": task, "depends_on": blocker}))
}
