//! Custom fields (ClickUp-style): `task_fields` defines a field (text,
//! number, date, select, checkbox or url) for one list or for all lists, and
//! `task_field_values` holds one value per (task, field).
//!
//! Values are stored as text: numbers as a decimal string, dates as
//! `YYYY-MM-DD`, checkboxes as `"1"` (unchecked has no row), selects as the
//! option id. Setting an empty value removes the row. The table is
//! delete-wins like `task_deps`, so uniqueness is enforced here: `set_field`
//! updates the newest live row and removes older duplicates, and queries read
//! the newest live row if two devices wrote one each while offline.
//!
//! Defining a field never edits tasks. Archiving hides a field and keeps its
//! values; removing a select option clears the values that used it.
use serde_json::{json, Map, Value};
use terrarium_guest::{clock, storage, Args, Error, Result};

use crate::TABLE;

const FIELDS: &str = "task_fields";
const VALUES: &str = "task_field_values";
const TYPES: [&str; 6] = ["text", "number", "date", "select", "checkbox", "url"];
const MAX_OPTIONS: usize = 50;
const MAX_TEXT: usize = 2000;

type Row = Map<String, Value>;

fn truth(value: Option<&Value>) -> bool {
    value.is_some_and(|v| v.as_bool().unwrap_or_else(|| v.as_i64() == Some(1)))
}

fn text_of(row: &Row, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[derive(Clone)]
struct Choice {
    id: String,
    label: String,
    color: Option<String>,
}

impl Choice {
    fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("id".into(), json!(self.id));
        o.insert("label".into(), json!(self.label));
        if let Some(color) = &self.color {
            o.insert("color".into(), json!(color));
        }
        Value::Object(o)
    }
}

fn stored_choices(field: &Row) -> Vec<Choice> {
    let Some(Value::Array(items)) = field
        .get("options")
        .and_then(Value::as_str)
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
    else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|o| {
            Some(Choice {
                id: o.get("id")?.as_str()?.to_string(),
                label: o.get("label")?.as_str()?.to_string(),
                color: o.get("color").and_then(Value::as_str).map(str::to_string),
            })
        })
        .collect()
}

fn choices_json(choices: &[Choice]) -> Value {
    json!(
        serde_json::to_string(&choices.iter().map(Choice::to_json).collect::<Vec<_>>())
            .unwrap_or_else(|_| "[]".into())
    )
}

fn live_field(id: &str) -> Result<Row> {
    storage::query(
        "SELECT * FROM task_fields WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| Error::new("not-found", "That field no longer exists"))
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

fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(Error::invalid("Name must contain 1 to 100 characters"));
    }
    Ok(name.to_string())
}

fn clean_color(color: &str) -> Result<Option<String>> {
    let color = color.trim();
    if color.is_empty() {
        return Ok(None);
    }
    let valid = color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|c| c.is_ascii_hexdigit());
    if !valid {
        return Err(Error::invalid("Color must be #RRGGBB or empty"));
    }
    Ok(Some(color.to_ascii_lowercase()))
}

/// A field name is unique, ignoring case, among the live, unarchived fields a
/// task of the same list could see (the list's own and all-lists ones).
fn check_name_free(name: &str, list: &Value, except: Option<&str>) -> Result<()> {
    let rows = storage::query(
        "SELECT id FROM task_fields WHERE _deleted IS NULL AND COALESCE(archived, 0) = 0 \
         AND lower(name) = lower(?1) AND id <> COALESCE(?2, '') \
         AND (COALESCE(list_id, '') = '' OR ?3 IS NULL OR list_id = ?3) LIMIT 1",
        &[json!(name), json!(except), list.clone()],
    )?;
    if rows.is_empty() {
        return Ok(());
    }
    Err(Error::invalid(format!(
        "A field named “{name}” already exists here"
    )))
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn new_choice_id(taken: &[String]) -> String {
    let stem = format!("o{}", base36(clock::now_ms()));
    let mut id = stem.clone();
    let mut n = 2;
    while taken.contains(&id) {
        id = format!("{stem}{n}");
        n += 1;
    }
    id
}

fn clean_label(label: &str) -> Result<String> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 100 {
        return Err(Error::invalid("An option needs 1 to 100 characters"));
    }
    Ok(label.to_string())
}

fn valid_choice_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// Reads an option list: a JSON array (strings, or `{id?, label, color?}`)
/// or text, one label per line (a single line may also be comma separated).
/// Options whose id or label matches an existing one keep that id.
fn parse_choices(text: &str, existing: &[Choice]) -> Result<Vec<Choice>> {
    let text = text.trim();
    let mut drafts: Vec<(Option<String>, String, Option<String>)> = Vec::new();
    if text.starts_with('[') {
        let Value::Array(items) = serde_json::from_str::<Value>(text)
            .map_err(|_| Error::invalid("Options must be a JSON array or one label per line"))?
        else {
            return Err(Error::invalid("Options must be a JSON array"));
        };
        for item in &items {
            match item {
                Value::String(label) => drafts.push((None, label.clone(), None)),
                Value::Object(o) => {
                    let label = o
                        .get("label")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::invalid("Each option needs a label"))?;
                    let id = match o.get("id") {
                        None | Some(Value::Null) => None,
                        Some(Value::String(s)) if valid_choice_id(s) => Some(s.clone()),
                        _ => return Err(Error::invalid("Option ids use letters, digits, - and _")),
                    };
                    let color = clean_color(o.get("color").and_then(Value::as_str).unwrap_or(""))?;
                    drafts.push((id, label.to_string(), color));
                }
                _ => return Err(Error::invalid("Each option must be text or an object")),
            }
        }
    } else if !text.is_empty() {
        let lines: Vec<&str> = if text.contains('\n') {
            text.lines().collect()
        } else {
            text.split(',').collect()
        };
        for line in lines.into_iter().map(str::trim).filter(|l| !l.is_empty()) {
            drafts.push((None, line.to_string(), None));
        }
    }
    if drafts.len() > MAX_OPTIONS {
        return Err(Error::invalid(format!(
            "A select field can have up to {MAX_OPTIONS} options"
        )));
    }
    let mut out: Vec<Choice> = Vec::new();
    for (id, label, color) in drafts {
        let label = clean_label(&label)?;
        if out
            .iter()
            .any(|c| c.label.to_lowercase() == label.to_lowercase())
        {
            return Err(Error::invalid(format!("“{label}” is listed twice")));
        }
        let by_id = id
            .as_deref()
            .and_then(|id| existing.iter().find(|c| c.id == id));
        let by_label = existing
            .iter()
            .find(|c| c.label.to_lowercase() == label.to_lowercase());
        let kept = by_id.or(by_label);
        let id = match (&id, kept) {
            (Some(id), _) => id.clone(),
            (None, Some(c)) => c.id.clone(),
            (None, None) => {
                let mut taken: Vec<String> = existing.iter().map(|c| c.id.clone()).collect();
                taken.extend(out.iter().map(|c| c.id.clone()));
                new_choice_id(&taken)
            }
        };
        if out.iter().any(|c| c.id == id) {
            return Err(Error::invalid("Two options share an id"));
        }
        let color = color.or_else(|| kept.and_then(|c| c.color.clone()));
        out.push(Choice { id, label, color });
    }
    Ok(out)
}

/// Removes every value of `field_id` that holds one of `option_ids`.
fn clear_option_values(field_id: &str, option_ids: &[String]) -> Result<usize> {
    let mut cleared = 0;
    for row in storage::query(
        "SELECT id, value FROM task_field_values WHERE field_id = ?1 AND _deleted IS NULL",
        &[json!(field_id)],
    )? {
        if option_ids.contains(&text_of(&row, "value")) {
            storage::delete(VALUES, row["id"].as_str().unwrap_or_default())?;
            cleared += 1;
        }
    }
    Ok(cleared)
}

fn list_value(args: &Args) -> Result<Value> {
    let list = args.opt_str("list_id").map(str::trim).unwrap_or("");
    if list.is_empty() {
        return Ok(Value::Null);
    }
    let rows = storage::query(
        "SELECT archived FROM task_lists WHERE id = ?1 AND _deleted IS NULL",
        &[json!(list)],
    )?;
    match rows.first() {
        Some(row) if !truth(row.get("archived")) => Ok(json!(list)),
        _ => Err(Error::invalid("Choose an active list")),
    }
}

/// `create_field {name, type, list_id?, options?, show_on_row?}`.
pub fn create_field(args: &Args) -> Result<Value> {
    let name = clean_name(args.str("name")?)?;
    let kind = args.str("type")?.trim();
    if !TYPES.contains(&kind) {
        return Err(Error::invalid(
            "Type must be text, number, date, select, checkbox or url",
        ));
    }
    let list = list_value(args)?;
    check_name_free(&name, &list, None)?;
    let options = args.opt_str("options").unwrap_or("");
    if kind != "select" && !options.trim().is_empty() {
        return Err(Error::invalid("Only select fields have options"));
    }
    let position = match args.get("position") {
        Some(p) if !p.is_null() => p
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or_else(|| Error::invalid("Position must be a nonnegative integer"))?,
        _ => storage::query(
            "SELECT COALESCE(MAX(position), -1) + 1 AS next FROM task_fields WHERE _deleted IS NULL",
            &[],
        )?
        .first()
        .and_then(|r| r["next"].as_i64())
        .unwrap_or(0),
    };
    let mut values = Row::new();
    values.insert("name".into(), json!(name));
    values.insert("type".into(), json!(kind));
    values.insert("list_id".into(), list.clone());
    if kind == "select" {
        values.insert(
            "options".into(),
            choices_json(&parse_choices(options, &[])?),
        );
    }
    values.insert(
        "show_on_row".into(),
        json!(args.get("show_on_row").is_some() && args.bool("show_on_row")?),
    );
    values.insert("position".into(), json!(position));
    values.insert("archived".into(), json!(false));
    let id = storage::insert(FIELDS, &values)?;
    Ok(json!({"id": id, "name": name, "type": kind, "list_id": list}))
}

/// `update_field {id, name?, options?, show_on_row?, position?, archived?}`.
/// The type and the list a field belongs to never change.
pub fn update_field(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let field = live_field(id)?;
    let mut values = Row::new();
    let list = match field.get("list_id").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => json!(s),
        _ => Value::Null,
    };
    let mut name = text_of(&field, "name");
    if args.get("name").is_some() {
        name = clean_name(args.str("name")?)?;
        values.insert("name".into(), json!(name));
    }
    if args.get("show_on_row").is_some() {
        values.insert("show_on_row".into(), json!(args.bool("show_on_row")?));
    }
    if let Some(position) = args.get("position").filter(|p| !p.is_null()) {
        let n = position
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or_else(|| Error::invalid("Position must be a nonnegative integer"))?;
        values.insert("position".into(), json!(n));
    }
    let mut archived = truth(field.get("archived"));
    if args.get("archived").is_some() {
        archived = args.bool("archived")?;
        values.insert("archived".into(), json!(archived));
    }
    if !archived && (values.contains_key("name") || values.contains_key("archived")) {
        check_name_free(&name, &list, Some(id))?;
    }
    let mut cleared = 0;
    if let Some(options) = args.opt_str("options") {
        if text_of(&field, "type") != "select" {
            return Err(Error::invalid("Only select fields have options"));
        }
        let before = stored_choices(&field);
        let after = parse_choices(options, &before)?;
        let dropped: Vec<String> = before
            .iter()
            .filter(|b| !after.iter().any(|a| a.id == b.id))
            .map(|b| b.id.clone())
            .collect();
        cleared = clear_option_values(id, &dropped)?;
        values.insert("options".into(), choices_json(&after));
    }
    if values.is_empty() {
        return Err(Error::invalid("nothing to update"));
    }
    storage::update(FIELDS, id, &values)?;
    Ok(json!({"id": id, "cleared": cleared}))
}

fn select_field(id: &str) -> Result<(Row, Vec<Choice>)> {
    let field = live_field(id)?;
    if text_of(&field, "type") != "select" {
        return Err(Error::invalid("Only select fields have options"));
    }
    let choices = stored_choices(&field);
    Ok((field, choices))
}

fn save_choices(id: &str, choices: &[Choice]) -> Result<()> {
    let mut values = Row::new();
    values.insert("options".into(), choices_json(choices));
    storage::update(FIELDS, id, &values)
}

/// `add_field_option {id, label, color?}`: appends an option to a select.
pub fn add_field_option(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let (_, mut choices) = select_field(id)?;
    let label = clean_label(args.str("label")?)?;
    if choices.len() >= MAX_OPTIONS {
        return Err(Error::invalid(format!(
            "A select field can have up to {MAX_OPTIONS} options"
        )));
    }
    if choices
        .iter()
        .any(|c| c.label.to_lowercase() == label.to_lowercase())
    {
        return Err(Error::invalid(format!("“{label}” is already an option")));
    }
    let taken: Vec<String> = choices.iter().map(|c| c.id.clone()).collect();
    let option_id = new_choice_id(&taken);
    choices.push(Choice {
        id: option_id.clone(),
        label,
        color: clean_color(args.opt_str("color").unwrap_or(""))?,
    });
    save_choices(id, &choices)?;
    Ok(json!({"id": id, "option_id": option_id}))
}

/// `update_field_option {id, option_id, label?, color?}`: renames or
/// recolors an option; tasks keep it because values hold the option id.
pub fn update_field_option(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let option_id = args.str("option_id")?;
    let (_, mut choices) = select_field(id)?;
    let at = choices
        .iter()
        .position(|c| c.id == option_id)
        .ok_or_else(|| Error::new("not-found", "That option no longer exists"))?;
    if args.get("label").is_some() {
        let label = clean_label(args.str("label")?)?;
        if choices
            .iter()
            .any(|c| c.id != option_id && c.label.to_lowercase() == label.to_lowercase())
        {
            return Err(Error::invalid(format!("“{label}” is already an option")));
        }
        choices[at].label = label;
    }
    if let Some(color) = args.opt_str("color") {
        choices[at].color = clean_color(color)?;
    }
    save_choices(id, &choices)?;
    Ok(json!({"id": id, "option_id": option_id}))
}

/// `remove_field_option {id, option_id}`: removes an option and clears the
/// values that used it.
pub fn remove_field_option(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let option_id = args.str("option_id")?;
    let (_, mut choices) = select_field(id)?;
    let before = choices.len();
    choices.retain(|c| c.id != option_id);
    if choices.len() == before {
        return Err(Error::new("not-found", "That option no longer exists"));
    }
    let cleared = clear_option_values(id, &[option_id.to_string()])?;
    save_choices(id, &choices)?;
    Ok(json!({"id": id, "option_id": option_id, "cleared": cleared}))
}

/// The value as the user or an assistant wrote it: text, number or boolean.
fn raw_value(value: Option<&Value>) -> Result<String> {
    match value {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.trim().to_string()),
        Some(Value::Bool(true)) => Ok("true".into()),
        Some(Value::Bool(false)) => Ok(String::new()),
        Some(Value::Number(n)) => Ok(n.to_string()),
        Some(_) => Err(Error::invalid(
            "A field value must be text, a number or true/false",
        )),
    }
}

fn normalize_number(raw: &str) -> Result<String> {
    let plain = raw
        .bytes()
        .all(|c| c.is_ascii_digit() || b"+-.eE".contains(&c));
    let n = raw
        .parse::<f64>()
        .ok()
        .filter(|n| plain && n.is_finite())
        .ok_or_else(|| Error::invalid("Give a number, like 1200 or 12.5"))?;
    if n.abs() >= 1e15 {
        return Err(Error::invalid("That number is too large"));
    }
    if n == 0.0 {
        return Ok("0".into());
    }
    Ok(if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    })
}

/// Checks `raw` against the field's type and returns what is stored; an empty
/// string means "no value".
fn normalize(field: &Row, raw: &str) -> Result<String> {
    if raw.is_empty() {
        return Ok(String::new());
    }
    match text_of(field, "type").as_str() {
        "text" => {
            if raw.chars().count() > MAX_TEXT {
                return Err(Error::invalid(format!(
                    "Text is limited to {MAX_TEXT} characters"
                )));
            }
            Ok(raw.to_string())
        }
        "url" => {
            let lower = raw.to_ascii_lowercase();
            let rest = lower
                .strip_prefix("https://")
                .or_else(|| lower.strip_prefix("http://"));
            if rest.is_none_or(str::is_empty)
                || raw.chars().any(char::is_whitespace)
                || raw.chars().count() > MAX_TEXT
            {
                return Err(Error::invalid(
                    "A link must start with http:// or https:// and have no spaces",
                ));
            }
            Ok(raw.to_string())
        }
        "number" => normalize_number(raw),
        "date" => {
            let shaped = raw.len() == 10
                && raw.bytes().enumerate().all(|(i, c)| {
                    if i == 4 || i == 7 {
                        c == b'-'
                    } else {
                        c.is_ascii_digit()
                    }
                });
            if !shaped || crate::quick::to_days(raw).is_none() {
                return Err(Error::invalid("A date must look like 2026-11-01"));
            }
            Ok(raw.to_string())
        }
        "checkbox" => match raw.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok("1".into()),
            "0" | "false" | "no" | "off" => Ok(String::new()),
            _ => Err(Error::invalid("A checkbox is 1 (checked) or empty")),
        },
        "select" => {
            let choices = stored_choices(field);
            let by_label = || {
                choices
                    .iter()
                    .find(|c| c.label.to_lowercase() == raw.to_lowercase())
            };
            choices
                .iter()
                .find(|c| c.id == raw)
                .or_else(by_label)
                .map(|c| c.id.clone())
                .ok_or_else(|| Error::invalid("That isn't one of this field's options"))
        }
        _ => Err(Error::invalid("This field has an unknown type")),
    }
}

/// `set_field {task_id, field_id, value}`: sets or (empty value) clears one
/// field of one task. Returns the `previous` stored value so Undo can put it
/// back. Setting the value it already has changes nothing.
pub fn set_field(args: &Args) -> Result<Value> {
    let task_id = args.str("task_id")?.trim();
    let field_id = args.str("field_id")?.trim();
    let task = live_task(task_id)?;
    let field = live_field(field_id)?;
    let scope = text_of(&field, "list_id");
    if !scope.is_empty() && task.get("list_id").and_then(Value::as_str) != Some(scope.as_str()) {
        return Err(Error::invalid("That field belongs to another list"));
    }
    let value = normalize(&field, &raw_value(args.get("value"))?)?;
    if !value.is_empty() && truth(field.get("archived")) {
        return Err(Error::invalid(
            "That field is archived; restore it to set values",
        ));
    }
    // Oldest first, so the last row is the newest one.
    let rows = storage::query(
        "SELECT id, value FROM task_field_values WHERE task_id = ?1 AND field_id = ?2 \
         AND _deleted IS NULL ORDER BY id",
        &[json!(task_id), json!(field_id)],
    )?;
    let previous = rows.last().map(|r| text_of(r, "value")).unwrap_or_default();
    let unchanged = previous == value && rows.len() <= 1;
    if !unchanged {
        if value.is_empty() {
            for row in &rows {
                storage::delete(VALUES, row["id"].as_str().unwrap_or_default())?;
            }
        } else {
            let mut values = Row::new();
            values.insert("value".into(), json!(value));
            match rows.last() {
                Some(newest) => {
                    if previous != value {
                        storage::update(
                            VALUES,
                            newest["id"].as_str().unwrap_or_default(),
                            &values,
                        )?;
                    }
                    for old in &rows[..rows.len() - 1] {
                        storage::delete(VALUES, old["id"].as_str().unwrap_or_default())?;
                    }
                }
                None => {
                    values.insert("task_id".into(), json!(task_id));
                    values.insert("field_id".into(), json!(field_id));
                    storage::insert(VALUES, &values)?;
                }
            }
        }
        if previous != value {
            // Marks the task as changed so search indexes it again.
            let mut touch = Row::new();
            touch.insert("fields_updated_at".into(), json!(clock::now_ms() as i64));
            storage::update(TABLE, task_id, &touch)?;
        }
    }
    Ok(json!({
        "task_id": task_id,
        "field_id": field_id,
        "name": field["name"],
        "value": value,
        "previous": previous,
        "changed": previous != value,
    }))
}
