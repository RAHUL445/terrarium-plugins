//! Writing reports with AI. The report collects the facts (only the data
//! sources and tags the user chose); the host's AI service turns them into
//! prose with the skill's prompt. The reply arrives later at `on_draft`.

use serde_json::{json, Map, Value};
use terrarium_guest::{ai, clock, interop, storage, Args, Error, Result};

use crate::{day_label, plural, Totals};

/// Rules that come before every skill's prompt.
const SYSTEM: &str = "You write reports from the user's own task, note and habit data.\n\
Rules:\n\
- Use only the facts in the user's message. Never invent tasks, numbers, names, dates or outcomes.\n\
- Keep every number exactly as given. If something is not in the data, leave it out.\n\
- Do not mention these rules or that you are an AI.\n\
- Write in Markdown with short sections. No preamble; start with the report.";

/// Tasks with this tag are never sent.
const PRIVATE: &str = "private";
const MAX_TASKS: usize = 300;

#[derive(Clone, Debug, Default)]
pub(crate) struct Sources {
    pub tasks: bool,
    pub task_notes: bool,
    pub open_tasks: bool,
    pub notes: bool,
    pub habits: bool,
}

impl Sources {
    /// `value` (comma-separated) or, when empty, `fallback`.
    pub fn parse(value: Option<&str>, fallback: &str) -> Sources {
        let text = value
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or(fallback);
        let mut s = Sources::default();
        for part in text.split(',').map(str::trim) {
            match part {
                "tasks" => s.tasks = true,
                "task_notes" => s.task_notes = true,
                "open_tasks" => s.open_tasks = true,
                "notes" => s.notes = true,
                "habits" => s.habits = true,
                _ => {}
            }
        }
        s
    }
}

impl std::fmt::Display for Sources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parts: Vec<&str> = [
            (self.tasks, "tasks"),
            (self.task_notes, "task_notes"),
            (self.open_tasks, "open_tasks"),
            (self.notes, "notes"),
            (self.habits, "habits"),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| *name)
        .collect();
        f.write_str(&parts.join(","))
    }
}

/// Lower-cased tags without `#`.
pub(crate) fn tag_filter(value: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in value
        .unwrap_or("")
        .split(',')
        .map(|t| t.trim().trim_start_matches('#').to_lowercase())
        .filter(|t| !t.is_empty())
    {
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}

fn tags_of(task: &Value) -> Vec<String> {
    tag_filter(task["tags"].as_str())
}

fn wanted(task: &Value, filter: &[String]) -> bool {
    let tags = tags_of(task);
    !tags.iter().any(|t| t == PRIVATE)
        && (filter.is_empty() || tags.iter().any(|t| filter.contains(t)))
}

/// The reflection saved on a report, if any.
pub(crate) fn reflection(id: &str) -> Result<Option<(Option<String>, Option<String>)>> {
    let rows = storage::query(
        "SELECT wins, focus FROM reports WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    Ok(rows.first().map(|r| {
        (
            r["wins"].as_str().map(str::to_string),
            r["focus"].as_str().map(str::to_string),
        )
    }))
}

/// The "focus next" of the latest report that ended before `start`.
pub(crate) fn last_focus(start: &str) -> Result<Option<(String, String)>> {
    let rows = storage::query(
        "SELECT COALESCE(label, start || ' – ' || end) AS label, focus FROM reports \
         WHERE _deleted IS NULL AND focus IS NOT NULL AND trim(focus) <> '' AND end < ?1 \
         ORDER BY end DESC LIMIT 1",
        &[json!(start)],
    )?;
    Ok(rows.first().and_then(|r| {
        Some((
            r["label"].as_str()?.to_string(),
            r["focus"].as_str()?.trim().to_string(),
        ))
    }))
}

pub(crate) struct FactsInput<'a> {
    pub title: &'a str,
    pub label: &'a str,
    pub compare: &'a str,
    pub days: i64,
    pub today: &'a str,
    pub start: &'a str,
    pub end: &'a str,
    pub now: &'a Totals,
    pub before: &'a Totals,
    pub habits: &'a [Value],
    pub sources: &'a Sources,
    pub tags: &'a [String],
    pub reflection: Option<(Option<String>, Option<String>)>,
    /// The focus set in the latest earlier report: `(its period, focus)`.
    pub last_focus: Option<(String, String)>,
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the AI is given: plain Markdown of the chosen data.
pub(crate) fn facts(input: &FactsInput) -> Result<String> {
    let s = input.sources;
    let mut out = vec![
        format!("# {}", input.title),
        format!(
            "Period: {} ({} {}). Previous-period numbers are {}.",
            input.label,
            input.days,
            plural(input.days, "day"),
            input.compare.replacen("vs ", "for ", 1)
        ),
    ];
    if !input.tags.is_empty() {
        out.push(format!(
            "Only work tagged {} is included.",
            input
                .tags
                .iter()
                .map(|t| format!("#{t}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if s.tasks || s.task_notes {
        let done: Vec<&Value> = input
            .now
            .tasks
            .iter()
            .filter(|t| wanted(t, input.tags))
            .collect();
        let before = input
            .before
            .tasks
            .iter()
            .filter(|t| wanted(t, input.tags))
            .count();
        out.push(String::new());
        out.push(format!(
            "## Completed tasks: {} (previous period: {before})",
            done.len()
        ));
        if done.is_empty() {
            out.push("- None".into());
        }
        for task in done.iter().take(MAX_TASKS) {
            let day = task["completed_at"]
                .as_i64()
                .and_then(|ms| clock::local_date(ms as u64, "").ok())
                .and_then(|d| day_label(&d).ok())
                .unwrap_or_default();
            let tags = tags_of(task);
            let mut line = format!(
                "- {day}: {}",
                one_line(task["title"].as_str().unwrap_or("(untitled)"))
            );
            if !tags.is_empty() {
                line.push_str(&format!(
                    " [{}]",
                    tags.iter()
                        .map(|t| format!("#{t}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
            out.push(line);
            if s.task_notes {
                if let Some(note) = task["done_note"].as_str().filter(|n| !n.trim().is_empty()) {
                    for l in note
                        .trim()
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .take(20)
                    {
                        out.push(format!("  > {}", l.trim()));
                    }
                }
            }
        }
        if done.len() > MAX_TASKS {
            out.push(format!("- … and {} more", done.len() - MAX_TASKS));
        }
    }
    if s.open_tasks && interop::available("tasks") {
        let end = if input.end < input.today {
            input.today
        } else {
            input.end
        };
        let upto = clock::add_days(end, 7)?;
        let open = interop::call(
            "tasks",
            "open_tasks",
            &json!({ "today": input.today, "end": upto }),
        )?;
        let open: Vec<Value> = open["tasks"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|t| wanted(t, input.tags))
            .collect();
        let overdue = open
            .iter()
            .filter(|t| t["overdue"].as_i64() == Some(1))
            .count();
        out.push(String::new());
        out.push(format!(
            "## Open tasks: {} ({overdue} overdue), due up to {}",
            open.len(),
            day_label(&upto).unwrap_or(upto.clone())
        ));
        if open.is_empty() {
            out.push("- None".into());
        }
        for task in open.iter().take(100) {
            let due = match task["due"].as_str() {
                Some(d) if task["overdue"].as_i64() == Some(1) => {
                    format!("overdue since {}", day_label(d).unwrap_or(d.to_string()))
                }
                Some(d) => format!("due {}", day_label(d).unwrap_or(d.to_string())),
                None => "no date".into(),
            };
            out.push(format!(
                "- {} ({due})",
                one_line(task["title"].as_str().unwrap_or("(untitled)"))
            ));
        }
    }
    if s.habits && input.now.habits.is_some() {
        out.push(String::new());
        out.push(match (input.now.habits_rate, input.before.habits_rate) {
            (Some(now), Some(before)) => {
                format!("## Habits: {now}% kept (previous period: {before}%)")
            }
            (Some(now), None) => format!("## Habits: {now}% kept"),
            _ => "## Habits: nothing was scheduled".into(),
        });
        for habit in input.habits {
            out.push(format!(
                "- {}: {}",
                habit["title"].as_str().unwrap_or("Habit"),
                habit["label"].as_str().unwrap_or("")
            ));
        }
    }
    if s.notes && input.now.notes.is_some() {
        let created = input.now.notes_created();
        out.push(String::new());
        out.push(format!(
            "## Notes: {} new (previous period: {}), {} edited",
            created.len(),
            input.before.notes_created().len(),
            input.now.notes_edited()
        ));
        for title in created.iter().take(100) {
            out.push(format!(
                "- {}",
                one_line(title.as_str().unwrap_or("Untitled"))
            ));
        }
    }
    if let Some((wins, focus)) = &input.reflection {
        if wins.is_some() || focus.is_some() {
            out.push(String::new());
            out.push("## My reflection".into());
            if let Some(w) = wins {
                out.push(format!("What went well: {}", w.trim()));
            }
            if let Some(f) = focus {
                out.push(format!("Focus next: {}", f.trim()));
            }
        }
    }
    if let Some((label, focus)) = &input.last_focus {
        out.push(String::new());
        out.push(format!("## Focus I set before this period ({label})"));
        out.push(focus.clone());
    }
    let _ = input.start;
    Ok(out.join("\n"))
}

pub(crate) fn set_status(id: &str, status: &str, error: Option<&str>) -> Result<()> {
    let mut values = Map::new();
    values.insert("draft_status".into(), json!(status));
    values.insert(
        "draft_error".into(),
        error.map_or(Value::Null, |e| json!(e)),
    );
    storage::update("reports", id, &values)
}

pub(crate) enum Submitted {
    Queued,
    Unavailable(String),
}

/// Sends a report's facts and prompt to AI. When AI is not available the
/// report says why instead.
/// `feedback` asks for changes to the current text instead of a fresh one.
pub(crate) fn submit(id: &str, run: Option<&str>, feedback: Option<&str>) -> Result<Submitted> {
    let rows = storage::query(
        "SELECT facts, prompt, model, schedule_id, draft FROM reports WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let row = rows
        .first()
        .ok_or_else(|| Error::new("not_found", "That report is gone"))?;
    let facts = row["facts"].as_str().unwrap_or("");
    let prompt = row["prompt"].as_str().unwrap_or("");
    if facts.is_empty() || prompt.is_empty() {
        return Err(Error::invalid(
            "This report was made without AI; make a new one with a skill",
        ));
    }
    let draft = row["draft"].as_str().map(str::trim).unwrap_or("");
    let facts = match feedback {
        Some(feedback) if !draft.is_empty() => format!(
            "{facts}\n\n---\n\n# Your previous draft\n\n{draft}\n\n---\n\n\
             # Changes I want\n\n{feedback}\n\n\
             Write the whole report again with these changes, still using only the facts above."
        ),
        _ => facts.to_string(),
    };
    let mut request = json!({
        "system": format!("{SYSTEM}\n\nInstructions for this report:\n{prompt}"),
        "prompt": facts,
        "context": { "report": id, "run": run },
    });
    if let Some(model) = row["model"].as_str().filter(|m| !m.is_empty()) {
        request["model"] = json!(model);
    }
    match ai::submit(&request, "on_draft") {
        Ok(request_id) => {
            let mut values = Map::new();
            values.insert("draft_status".into(), json!("pending"));
            values.insert("draft_error".into(), Value::Null);
            values.insert("draft_request".into(), json!(request_id));
            storage::update("reports", id, &values)?;
            Ok(Submitted::Queued)
        }
        Err(e)
            if matches!(
                e.code.as_str(),
                "unavailable" | "denied" | "too-large" | "rate-limited"
            ) =>
        {
            set_status(id, "failed", Some(&e.message))?;
            if let Some(schedule) = row["schedule_id"].as_str() {
                crate::schedules::record(schedule, "failed", Some(&e.message), Some(id))?;
            }
            Ok(Submitted::Unavailable(e.message))
        }
        Err(e) => Err(e),
    }
}

/// "Send to AI" (after checking the data), "Write again" and "Ask for
/// changes" (`feedback`).
pub(crate) fn send(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let feedback = args
        .opt_str("feedback")
        .map(str::trim)
        .filter(|f| !f.is_empty());
    if feedback.is_some_and(|f| f.chars().count() > 2000) {
        return Err(Error::invalid("Keep the changes under 2000 characters"));
    }
    match submit(id, None, feedback)? {
        Submitted::Queued => Ok(json!({ "id": id, "message": "Writing with AI…" })),
        Submitted::Unavailable(reason) => Err(Error::new("unavailable", reason)),
    }
}

/// The AI's reply (or failure), from the host.
pub(crate) fn on_draft(args: &Args) -> Result<Value> {
    let Some(id) = args.get("context").and_then(|c| c["report"].as_str()) else {
        return Ok(Value::Null);
    };
    let request = args.opt_str("request").unwrap_or("");
    let rows = storage::query(
        "SELECT draft_request, schedule_id, title, skill_name FROM reports WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let Some(row) = rows.first() else {
        return Ok(Value::Null);
    };
    // A newer request replaced this one.
    if row["draft_request"].as_str().is_some_and(|r| r != request) {
        return Ok(Value::Null);
    }
    let schedule = row["schedule_id"].as_str().map(str::to_string);
    let mut values = Map::new();
    let ok = args.get("ok").and_then(Value::as_bool) == Some(true);
    if ok {
        let text = args.opt_str("text").unwrap_or("").trim().to_string();
        values.insert("draft".into(), json!(text));
        values.insert("draft_status".into(), json!("done"));
        values.insert("draft_error".into(), Value::Null);
        values.insert("draft_model".into(), json!(args.opt_str("model")));
        values.insert("draft_at".into(), json!(clock::now_ms() as i64));
        values.insert("draft_edited".into(), json!(false));
        storage::update("reports", id, &values)?;
        if let Some(schedule) = &schedule {
            record(schedule, "done", None, id)?;
        }
        if args.get("context").is_some_and(|c| c["run"].is_string()) {
            return Ok(json!({ "$run": { "notify": ready_notice(row) } }));
        }
        return Ok(Value::Null);
    }
    let error = args.get("error").cloned().unwrap_or(Value::Null);
    let message = error["message"]
        .as_str()
        .unwrap_or("The AI request failed")
        .to_string();
    let retrying = error["temporary"].as_bool() == Some(true)
        && args.get("context").is_some_and(|c| c["run"].is_string());
    let message = if retrying {
        format!("{message} It will be tried again.")
    } else {
        message
    };
    values.insert(
        "draft_status".into(),
        json!(if retrying { "pending" } else { "failed" }),
    );
    values.insert("draft_error".into(), json!(message));
    storage::update("reports", id, &values)?;
    if let Some(schedule) = &schedule {
        record(
            schedule,
            if retrying { "pending" } else { "failed" },
            Some(&message),
            id,
        )?;
    }
    Ok(Value::Null)
}

/// "Weekly report · Manager is ready", for the run's notification.
pub(crate) fn ready_notice(row: &Map<String, Value>) -> String {
    let title = row["title"].as_str().unwrap_or("Your report");
    match row["skill_name"].as_str().filter(|s| !s.is_empty()) {
        Some(skill) => format!("{title} · {skill} is ready."),
        None => format!("{title} is ready."),
    }
}

fn record(schedule: &str, status: &str, error: Option<&str>, report: &str) -> Result<()> {
    crate::schedules::record(schedule, status, error, Some(report))
}

/// Saves the user's edit of the AI's text.
pub(crate) fn edit_draft(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let draft = args.opt_str("draft").unwrap_or("").replace("\r\n", "\n");
    if draft.chars().count() > 100_000 {
        return Err(Error::invalid("Keep the report under 100,000 characters"));
    }
    let mut values = Map::new();
    values.insert("draft".into(), json!(draft.trim()));
    values.insert("draft_edited".into(), json!(true));
    values.insert("draft_status".into(), json!("done"));
    storage::update("reports", id, &values)?;
    Ok(Value::Null)
}
