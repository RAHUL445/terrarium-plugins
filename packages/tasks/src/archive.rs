//! Archive ("Logbook") for completed tasks, distinct from the trash.
//!
//! The archive marker is the existing `tasks.cleared_at` (unix ms; non-null
//! means archived), so tasks hidden by the old "Clear completed" are the
//! initial archive and nothing is backfilled. An archived task stays done and
//! keeps counting in reviews, reports and exports; the lists just stop
//! showing it. The Archive section lists them (query `archived`).
//!
//! Every archiving step stamps the tasks it touches with one shared `at`, so
//! the matching undo (`unarchive {at}`) finds exactly that batch.
//!
//! Auto-archive is a setting (`settings.auto_archive`: `never`, `end_of_day`,
//! `7d`, `30d`; empty means `never`) plus a plugin-made daily schedule that
//! runs `auto_archive`. A run is safe to repeat on two devices: it only
//! touches rows that are not archived yet, and uses the schedule slot as its
//! time when the host gives one.
use serde_json::{json, Map, Value};
use terrarium_guest::{clock, schedule, storage, Args, Error, Result};

use crate::TABLE;

const SCHEDULE: &str = "auto-archive";
const DAY_MS: u64 = 86_400_000;
pub(crate) const MODES: [&str; 4] = ["never", "end_of_day", "7d", "30d"];

fn stamp(at: u64) -> Map<String, Value> {
    let mut values = Map::new();
    values.insert("cleared_at".into(), json!(at as i64));
    values
}

fn is_done(row: &Map<String, Value>) -> bool {
    row["done"]
        .as_bool()
        .unwrap_or_else(|| row["done"].as_i64() == Some(1))
}

fn subtasks_phrase(n: usize) -> String {
    match n {
        0 => String::new(),
        1 => " with 1 subtask".to_string(),
        n => format!(" with {n} subtasks"),
    }
}

/// Archives one done task and its done subtasks (open subtasks stay).
pub(crate) fn archive(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let rows = storage::query(
        "SELECT id, done, cleared_at FROM tasks WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let task = rows
        .first()
        .ok_or_else(|| Error::new("not-found", "That task no longer exists"))?;
    if !is_done(task) {
        return Err(Error::invalid("Only completed tasks can be archived"));
    }
    if let Some(at) = task["cleared_at"].as_i64() {
        // Retried or repeated: leave the earlier archive time alone.
        return Ok(json!({ "archived": 0, "at": at, "message": "Already archived" }));
    }
    let at = clock::now_ms();
    let children = storage::query(
        "SELECT id FROM tasks WHERE parent_id = ?1 AND _deleted IS NULL \
         AND done = 1 AND cleared_at IS NULL ORDER BY id",
        &[json!(id)],
    )?;
    let values = stamp(at);
    storage::update(TABLE, id, &values)?;
    for child in &children {
        storage::update(TABLE, child["id"].as_str().unwrap_or_default(), &values)?;
    }
    Ok(json!({
        "archived": 1 + children.len(),
        "at": at,
        "subtasks": children.len(),
        "message": format!("Archived{}", subtasks_phrase(children.len())),
    }))
}

/// Brings archived tasks back to the lists. `{id}`: the task and the done
/// subtasks archived at the same moment as it. `{at}`: everything archived
/// at that moment (the undo of a bulk archive). The tasks stay done.
pub(crate) fn unarchive(args: &Args) -> Result<Value> {
    let by_id = args.opt_str("id").filter(|id| !id.trim().is_empty());
    let at = match args.get("at") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) => Some(
            s.trim()
                .parse::<i64>()
                .map_err(|_| Error::invalid("`at` must be a number"))?,
        ),
        Some(v) => Some(
            v.as_i64()
                .ok_or_else(|| Error::invalid("`at` must be a number"))?,
        ),
    };
    let mut ids: Vec<String> = Vec::new();
    match (by_id, at) {
        (Some(id), _) => {
            let rows = storage::query(
                "SELECT id, cleared_at FROM tasks WHERE id = ?1 AND _deleted IS NULL",
                &[json!(id)],
            )?;
            let task = rows
                .first()
                .ok_or_else(|| Error::new("not-found", "That task no longer exists"))?;
            if let Some(stamped) = task["cleared_at"].as_i64() {
                ids.push(id.to_string());
                let children = storage::query(
                    "SELECT id FROM tasks WHERE parent_id = ?1 AND _deleted IS NULL \
                     AND done = 1 AND cleared_at = ?2 ORDER BY id",
                    &[json!(id), json!(stamped)],
                )?;
                ids.extend(
                    children
                        .iter()
                        .filter_map(|c| c["id"].as_str().map(str::to_string)),
                );
            }
        }
        (None, Some(at)) => {
            let rows = storage::query(
                "SELECT id FROM tasks WHERE _deleted IS NULL AND cleared_at = ?1 ORDER BY id",
                &[json!(at)],
            )?;
            ids.extend(
                rows.iter()
                    .filter_map(|r| r["id"].as_str().map(str::to_string)),
            );
        }
        (None, None) => {
            return Err(Error::invalid("Give a task `id` or the archive time `at`"));
        }
    }
    let mut values = Map::new();
    values.insert("cleared_at".into(), Value::Null);
    for id in &ids {
        storage::update(TABLE, id, &values)?;
    }
    let message = match ids.len() {
        0 => "Nothing to unarchive".to_string(),
        1 => "Unarchived".to_string(),
        n => format!("Unarchived {n} tasks"),
    };
    Ok(json!({ "unarchived": ids.len(), "message": message }))
}

/// Archives every done, live task that is not archived yet (the old "Clear
/// completed"). `cleared` is kept for older callers and equals `archived`.
pub(crate) fn archive_done() -> Result<Value> {
    let at = clock::now_ms();
    let archived = archive_where("done = 1", &[], at)?;
    Ok(json!({ "cleared": archived, "archived": archived, "at": at }))
}

/// Archives the not-yet-archived done tasks matching `condition`.
fn archive_where(condition: &str, params: &[Value], at: u64) -> Result<usize> {
    let rows = storage::query(
        &format!(
            "SELECT id FROM tasks WHERE _deleted IS NULL AND cleared_at IS NULL AND {condition} \
             ORDER BY id"
        ),
        params,
    )?;
    let values = stamp(at);
    for row in &rows {
        storage::update(TABLE, row["id"].as_str().unwrap_or_default(), &values)?;
    }
    Ok(rows.len())
}

/// The auto-archive mode as saved (`never` when unset or unreadable).
fn saved_mode() -> Result<String> {
    let rows = storage::query(
        "SELECT auto_archive FROM settings WHERE _deleted IS NULL ORDER BY id LIMIT 1",
        &[],
    )?;
    let mode = rows
        .first()
        .and_then(|r| r["auto_archive"].as_str())
        .unwrap_or("never");
    Ok(if MODES.contains(&mode) { mode } else { "never" }.to_string())
}

/// Latest completion time (unix ms) that auto-archive takes, or `None` for
/// `never`. `end_of_day` takes everything finished before local `today`.
fn cutoff(mode: &str, now: u64, today: &str) -> Result<Option<u64>> {
    Ok(match mode {
        "end_of_day" => Some(clock::local_to_utc(today, "00:00", "")?),
        "7d" => Some(now.saturating_sub(7 * DAY_MS)),
        "30d" => Some(now.saturating_sub(30 * DAY_MS)),
        _ => None,
    })
}

fn run_pass(mode: &str, now: u64, today: &str) -> Result<usize> {
    match cutoff(mode, now, today)? {
        Some(before) => archive_where(
            "done = 1 AND completed_at IS NOT NULL AND completed_at < ?1",
            &[json!(before as i64)],
            now,
        ),
        None => Ok(0),
    }
}

/// The schedule target. Safe to repeat: it only touches rows that are not
/// archived yet. A run's slot time stands for "now" (and is the archive time,
/// so two devices running one slot write the same value); its date stands for
/// today.
pub(crate) fn auto_archive(args: &Args) -> Result<Value> {
    let run = args.get("$run");
    let slot = run.and_then(|r| r["slot_ms"].as_u64()).filter(|ms| *ms > 0);
    let now = slot.unwrap_or_else(clock::now_ms);
    let today = match run.and_then(|r| r["date"].as_str()) {
        Some(date) if slot.is_some() => date.to_string(),
        _ => clock::today("")?,
    };
    let archived = run_pass(&saved_mode()?, now, &today)?;
    if archived == 0 {
        return Ok(json!({ "archived": 0 }));
    }
    let notice = match archived {
        1 => "Archived 1 completed task.".to_string(),
        n => format!("Archived {n} completed tasks."),
    };
    Ok(json!({ "archived": archived, "at": now, "$run": { "notify": notice } }))
}

/// Saves the auto-archive setting, turns the daily schedule on or off, and
/// runs one pass straight away.
pub(crate) fn archive_settings(args: &Args) -> Result<Value> {
    let auto = args.opt_str("auto").map(str::trim).unwrap_or("");
    if !MODES.contains(&auto) {
        return Err(Error::invalid("auto must be never, end_of_day, 7d or 30d"));
    }
    let mut values = Map::new();
    values.insert("auto_archive".into(), json!(auto));
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
    if auto == "never" {
        match schedule::remove(SCHEDULE) {
            Ok(()) => {}
            Err(e) if e.code == "not-found" || e.code == "not_found" => {}
            Err(e) => return Err(e),
        }
    } else {
        schedule::set(
            SCHEDULE,
            &json!({
                "name": "Auto-archive completed tasks",
                "repeat": { "every": "day", "at": "00:05" },
                "operation": "auto_archive",
            }),
        )?;
    }
    let now = clock::now_ms();
    let archived = run_pass(auto, now, &clock::today("")?)?;
    let label = match auto {
        "end_of_day" => "at the end of the day",
        "7d" => "after 7 days",
        "30d" => "after 30 days",
        _ => "off",
    };
    let message = match (auto, archived) {
        ("never", _) => "Auto-archive is off".to_string(),
        (_, 0) => format!("Auto-archive on: {label}"),
        (_, 1) => format!("Auto-archive on: {label}. Archived 1 task"),
        (_, n) => format!("Auto-archive on: {label}. Archived {n} tasks"),
    };
    Ok(json!({ "auto": auto, "archived": archived, "at": now, "message": message }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_are_the_documented_four() {
        assert_eq!(MODES, ["never", "end_of_day", "7d", "30d"]);
    }

    #[test]
    fn subtask_phrase_reads_naturally() {
        assert_eq!(subtasks_phrase(0), "");
        assert_eq!(subtasks_phrase(1), " with 1 subtask");
        assert_eq!(subtasks_phrase(2), " with 2 subtasks");
    }
}
