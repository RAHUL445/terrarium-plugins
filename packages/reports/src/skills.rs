//! Skills: saved prompts that shape how AI writes a report, for a reader.
//!
//! Starters ship in `skills.json` and are addressed as `builtin:<key>`.
//! Editing a starter saves an override row with its `builtin_key`; deleting
//! one hides it. User skills are plain rows.

use serde_json::{json, Map, Value};
use terrarium_guest::{clock, storage, Args, Error, Result};

const TABLE: &str = "skills";
const BUILTINS: &str = include_str!("../skills.json");

pub(crate) struct Skill {
    /// `builtin:<key>`, a skill row id, or `custom` for a one-off prompt.
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub model: Option<String>,
    pub period: Option<String>,
    /// Default data sources, comma-separated.
    pub sources: String,
    pub tags: Option<String>,
}

const ALL_SOURCES: &str = "tasks,task_notes,open_tasks,notes,habits";

fn text(row: &Map<String, Value>, key: &str) -> Option<String> {
    row.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn builtin(key: &str) -> Option<Map<String, Value>> {
    let list: Vec<Map<String, Value>> = serde_json::from_str(BUILTINS).ok()?;
    list.into_iter().find(|s| s["key"].as_str() == Some(key))
}

fn from_row(id: String, row: &Map<String, Value>) -> Skill {
    Skill {
        id,
        name: text(row, "name").unwrap_or_else(|| "Skill".into()),
        prompt: text(row, "prompt").unwrap_or_default(),
        model: text(row, "model"),
        period: text(row, "period"),
        sources: text(row, "sources").unwrap_or_else(|| ALL_SOURCES.into()),
        tags: text(row, "tags"),
    }
}

/// A skill by id, as it is now.
pub(crate) fn find(id: &str) -> Result<Option<Skill>> {
    if let Some(key) = id.strip_prefix("builtin:") {
        let rows = storage::query(
            "SELECT name, prompt, model, period, sources, tags, hidden FROM skills \
             WHERE _deleted IS NULL AND builtin_key = ?1 ORDER BY created_at LIMIT 1",
            &[json!(key)],
        )?;
        if let Some(row) = rows.first() {
            if row["hidden"].as_bool() == Some(true) || row["hidden"].as_i64() == Some(1) {
                return Ok(None);
            }
            return Ok(Some(from_row(id.to_string(), row)));
        }
        return Ok(builtin(key).map(|row| from_row(id.to_string(), &row)));
    }
    let rows = storage::query(
        "SELECT name, prompt, model, period, sources, tags FROM skills WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    Ok(rows.first().map(|row| from_row(id.to_string(), row)))
}

/// The skill a `generate` call asks for, or `None` for a report without AI.
/// Refreshing a saved report (`id` without `skill`) keeps how it was made.
pub(crate) fn resolve(args: &Args) -> Result<Option<Skill>> {
    let extra = args
        .opt_str("prompt")
        .map(str::trim)
        .filter(|p| !p.is_empty());
    if extra.is_some_and(|p| p.chars().count() > 8000) {
        return Err(Error::invalid(
            "Keep the instructions under 8000 characters",
        ));
    }
    let skill = args.opt_str("skill").map(str::trim).unwrap_or("");
    if skill.is_empty() {
        if let Some(id) = args.opt_str("id").filter(|id| !id.is_empty()) {
            let rows = storage::query(
                "SELECT skill_id, skill_name AS name, prompt, model, sources, tag_filter AS tags FROM reports \
                 WHERE id = ?1 AND _deleted IS NULL AND skill_id IS NOT NULL",
                &[json!(id)],
            )?;
            return Ok(rows.first().map(|row| {
                let mut skill = from_row(text(row, "skill_id").unwrap_or_default(), row);
                skill.period = None;
                skill
            }));
        }
        return Ok(None);
    }
    if skill == "none" {
        return Ok(None);
    }
    if skill == "custom" {
        let prompt = extra.ok_or_else(|| Error::invalid("Write the instructions for the AI"))?;
        let sources = args
            .opt_str("sources")
            .filter(|s| !s.is_empty())
            .unwrap_or(ALL_SOURCES);
        if let Some(name) = args
            .opt_str("save_as")
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            let id = insert(name, prompt, sources, args.opt_str("tags"), None)?;
            return find(&id);
        }
        return Ok(Some(Skill {
            id: "custom".into(),
            name: "One-off".into(),
            prompt: prompt.to_string(),
            model: None,
            period: None,
            sources: sources.to_string(),
            tags: None,
        }));
    }
    let mut found =
        find(skill)?.ok_or_else(|| Error::invalid("That skill is gone; pick another"))?;
    if let Some(extra) = extra {
        found.prompt = format!("{}\n\nAlso: {extra}", found.prompt);
    }
    Ok(Some(found))
}

fn clean(args: &Args, key: &str, max: usize, label: &str) -> Result<Option<String>> {
    match args.opt_str(key).map(str::trim) {
        Some(v) if v.chars().count() > max => Err(Error::invalid(format!(
            "{label} must be {max} characters or fewer"
        ))),
        Some("") => Ok(None),
        Some(v) => Ok(Some(v.to_string())),
        None => Ok(None),
    }
}

fn check_period(period: &Option<String>) -> Result<()> {
    match period.as_deref() {
        None | Some("week" | "last_week" | "7d" | "month" | "last_month") => Ok(()),
        Some(_) => Err(Error::invalid(
            "Range must be week, last_week, 7d, month or last_month",
        )),
    }
}

fn insert(
    name: &str,
    prompt: &str,
    sources: &str,
    tags: Option<&str>,
    period: Option<&str>,
) -> Result<String> {
    let mut values = Map::new();
    values.insert("name".into(), json!(name));
    values.insert("prompt".into(), json!(prompt));
    values.insert("sources".into(), json!(sources));
    if let Some(tags) = tags.filter(|t| !t.trim().is_empty()) {
        values.insert("tags".into(), json!(tags));
    }
    if let Some(period) = period {
        values.insert("period".into(), json!(period));
    }
    values.insert("hidden".into(), json!(false));
    values.insert("created_at".into(), json!(clock::now_ms() as i64));
    storage::insert(TABLE, &values)
}

/// Creates or changes a skill: `id` (a row, or `builtin:<key>` to edit a
/// starter), name, prompt, model, period, sources, tags.
pub(crate) fn save(args: &Args) -> Result<Value> {
    let name = clean(args, "name", 60, "Name")?.ok_or_else(|| Error::invalid("Name the skill"))?;
    let prompt = clean(args, "prompt", 8000, "The prompt")?
        .ok_or_else(|| Error::invalid("Write what the AI should do"))?;
    let model = clean(args, "model", 200, "The model")?;
    let period = clean(args, "period", 20, "Range")?;
    check_period(&period)?;
    let sources = crate::ai::Sources::parse(args.opt_str("sources"), ALL_SOURCES).to_string();
    let tags = clean(args, "tags", 300, "Tags")?;
    let mut values = Map::new();
    values.insert("name".into(), json!(name));
    values.insert("prompt".into(), json!(prompt));
    values.insert("model".into(), model.map_or(Value::Null, |m| json!(m)));
    values.insert("period".into(), period.map_or(Value::Null, |p| json!(p)));
    values.insert("sources".into(), json!(sources));
    values.insert("tags".into(), tags.map_or(Value::Null, |t| json!(t)));
    values.insert("hidden".into(), json!(false));
    values.insert("updated_at".into(), json!(clock::now_ms() as i64));
    let id = args.opt_str("id").map(str::trim).unwrap_or("");
    if let Some(key) = id.strip_prefix("builtin:") {
        if builtin(key).is_none() {
            return Err(Error::invalid("That starter skill does not exist"));
        }
        let rows = storage::query(
            "SELECT id FROM skills WHERE _deleted IS NULL AND builtin_key = ?1 ORDER BY created_at LIMIT 1",
            &[json!(key)],
        )?;
        match rows.first().and_then(|r| r["id"].as_str()) {
            Some(row) => storage::update(TABLE, row, &values)?,
            None => {
                values.insert("builtin_key".into(), json!(key));
                values.insert("created_at".into(), json!(clock::now_ms() as i64));
                storage::insert(TABLE, &values)?;
            }
        }
        return Ok(json!({ "id": id, "message": "Skill saved" }));
    }
    if id.is_empty() {
        values.insert("created_at".into(), json!(clock::now_ms() as i64));
        let id = storage::insert(TABLE, &values)?;
        return Ok(json!({ "id": id, "message": "Skill added" }));
    }
    storage::update(TABLE, id, &values)?;
    Ok(json!({ "id": id, "message": "Skill saved" }))
}

/// Deletes a skill. A starter is hidden instead; `reset` on an edited
/// starter brings back the original.
pub(crate) fn delete(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let reset = args.get("reset").is_some_and(|v| v.as_bool() == Some(true));
    let Some(key) = id.strip_prefix("builtin:") else {
        storage::delete(TABLE, id)?;
        return Ok(json!({ "message": "Skill deleted" }));
    };
    let rows = storage::query(
        "SELECT id FROM skills WHERE _deleted IS NULL AND builtin_key = ?1",
        &[json!(key)],
    )?;
    if reset {
        for row in &rows {
            if let Some(row) = row["id"].as_str() {
                storage::delete(TABLE, row)?;
            }
        }
        return Ok(json!({ "message": "Starter restored" }));
    }
    let mut values = Map::new();
    values.insert("hidden".into(), json!(true));
    match rows.first().and_then(|r| r["id"].as_str()) {
        Some(row) => storage::update(TABLE, row, &values)?,
        None => {
            let skill = builtin(key).ok_or_else(|| Error::invalid("No such starter"))?;
            values.insert("builtin_key".into(), json!(key));
            values.insert("name".into(), skill["name"].clone());
            values.insert("created_at".into(), json!(clock::now_ms() as i64));
            storage::insert(TABLE, &values)?;
        }
    }
    Ok(json!({ "message": "Skill removed" }))
}
