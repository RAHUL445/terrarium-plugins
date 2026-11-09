//! Reports: reviews of a week, a month or any range, built from To-do
//! (completed tasks) and, when installed, Habits and Notes, through their
//! interfaces. Each report compares itself with the period just before it,
//! keeps per-day, per-tag and per-habit breakdowns as JSON for the page's
//! queries, and holds a short reflection the user writes.
//!
//! A report can also be written by AI with a *skill*: a saved prompt for a
//! reader (a manager, a junior, yourself). The numbers always come from the
//! data; the AI only writes prose from the facts the report collected,
//! limited to the data sources and tags the user picked. Skills ship as
//! starters (`skills.json`) and can be edited or added. Reports can be
//! scheduled through the host scheduler.

mod ai;
mod schedules;
mod skills;

use serde_json::{json, Map, Value};
use terrarium_guest::{clock, interop, storage, Args, Error, Plugin, Result};

struct Reports;

const TABLE: &str = "reports";
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

impl Plugin for Reports {
    fn handle(operation: &str, args: Args) -> Result<Value> {
        match operation {
            "generate" => generate(&args),
            "reflect" => reflect(&args),
            "delete" => {
                storage::delete(TABLE, args.str("id")?)?;
                Ok(Value::Null)
            }
            "send" => ai::send(&args),
            "on_draft" => ai::on_draft(&args),
            "edit_draft" => ai::edit_draft(&args),
            "save_skill" => skills::save(&args),
            "delete_skill" => skills::delete(&args),
            "save_schedule" => schedules::save(&args),
            "delete_schedule" => schedules::delete(&args),
            "toggle_schedule" => schedules::toggle(&args),
            "run_schedule" => schedules::run_now(&args),
            "scheduled" => schedules::run(&args),
            _ => Err(Error::unknown_operation(operation)),
        }
    }
}

/// What one range adds up to.
pub(crate) struct Totals {
    pub tasks: Vec<Value>,
    pub habits: Option<Value>,
    pub habits_rate: Option<f64>,
    pub notes: Option<Value>,
}

impl Totals {
    pub fn read(start: &str, end: &str) -> Result<Self> {
        let range = json!({ "start": start, "end": end });
        let tasks = interop::call("tasks", "completed_between", &range)?["tasks"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let habits = if interop::available("habits") {
            Some(interop::call("habits", "summary", &range)?)
        } else {
            None
        };
        let habits_rate = habits.as_ref().and_then(|h| percent(&h["rate"]));
        let notes = if interop::available("notes") {
            Some(interop::call("notes", "summary", &range)?)
        } else {
            None
        };
        Ok(Totals {
            tasks,
            habits,
            habits_rate,
            notes,
        })
    }

    pub fn notes_created(&self) -> Vec<Value> {
        self.notes
            .as_ref()
            .and_then(|n| n["created"].as_array().cloned())
            .unwrap_or_default()
    }

    pub fn notes_edited(&self) -> i64 {
        self.notes
            .as_ref()
            .and_then(|n| n["updated"].as_i64())
            .unwrap_or(0)
    }
}

/// Builds (or rebuilds) a report. `period` is `week` (Monday to today, the
/// default), `last_week`, `7d` (the last seven days), `month`, `last_month`,
/// or `custom` with `start` and `end`; `start`/`end` alone also mean custom.
/// A report for the same range and skill is refreshed in place, keeping its
/// reflection; so is this week's or this month's report as the days go by.
///
/// `skill` (a skill id, `custom` with `prompt`, or empty for no AI) has the
/// report written by AI from the data in `sources` (comma-separated: tasks,
/// task_notes, open_tasks, notes, habits), limited to tasks tagged with one
/// of `tags` when given. `prompt` adds to a skill's prompt; `save_as` keeps
/// a one-off prompt as a new skill. With `review`, the collected data waits
/// in the report to be checked before it is sent. Scheduled runs pass
/// `$run`, whose date stands for today.
fn generate(args: &Args) -> Result<Value> {
    let run = args.get("$run").cloned().unwrap_or(Value::Null);
    let today = match run["date"].as_str() {
        Some(day) => day.to_string(),
        None => clock::today("")?,
    };
    let skill = skills::resolve(args)?;
    let (period, start, end) = match args.opt_str("id").filter(|id| !id.is_empty()) {
        Some(id) => refresh_range(id, &today)?,
        None => {
            let custom = args.opt_str("start").is_some_and(|s| !s.trim().is_empty())
                || args.opt_str("end").is_some_and(|s| !s.trim().is_empty());
            let wanted = args
                .opt_str("period")
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .or_else(|| skill.as_ref().and_then(|s| s.period.clone()));
            let period = match wanted {
                Some(p) => p,
                None if custom => "custom".to_string(),
                None => "week".to_string(),
            };
            let (start, end) = range_for(&period, &today, args)?;
            (period, start, end)
        }
    };
    let period = period.as_str();
    let days = days_between(&start, &end)? + 1;
    let (prev_start, prev_end, compare) = previous(period, &start, &end, days)?;

    let now = Totals::read(&start, &end)?;
    let before = Totals::read(&prev_start, &prev_end)?;

    // Tasks per day, every day of the range present (for the chart).
    let mut daily: Vec<(String, i64)> = Vec::new();
    let mut day = start.clone();
    while day <= end {
        daily.push((day.clone(), 0));
        day = clock::add_days(&day, 1)?;
    }
    // This week's chart shows the whole week, days still to come empty.
    let chart_end = if period == "week" {
        clock::add_days(&start, 6)?
    } else {
        end.clone()
    };
    let mut task_list = Vec::new();
    let mut tag_counts: Vec<(String, i64)> = Vec::new();
    for task in &now.tasks {
        let done_on = task["completed_at"]
            .as_i64()
            .map(|ms| clock::local_date(ms as u64, ""))
            .transpose()?
            .unwrap_or_else(|| end.clone());
        if let Some(slot) = daily.iter_mut().find(|(d, _)| *d == done_on) {
            slot.1 += 1;
        }
        for tag in task["tags"]
            .as_str()
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            match tag_counts.iter_mut().find(|(t, _)| t == tag) {
                Some(slot) => slot.1 += 1,
                None => tag_counts.push((tag.to_string(), 1)),
            }
        }
        task_list.push(json!({
            "title": task["title"].as_str().unwrap_or("(untitled)"),
            "day": done_on,
            "day_label": day_label(&done_on)?,
        }));
    }
    tag_counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let habit_list: Vec<Value> = now
        .habits
        .as_ref()
        .and_then(|h| h["habits"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|h| {
            let (done, scheduled, skipped) = (
                h["done"].as_i64().unwrap_or(0),
                h["scheduled"].as_i64().unwrap_or(0),
                h["skipped"].as_i64().unwrap_or(0),
            );
            json!({
                "title": h["title"].as_str().unwrap_or("Habit"),
                "done": done,
                "due": scheduled - skipped,
                "rate": percent(&h["rate"]),
                "label": if skipped > 0 {
                    format!("{done}/{} · {skipped} skipped", scheduled - skipped)
                } else {
                    format!("{done}/{scheduled}")
                },
            })
        })
        .collect();

    let notes_created = now.notes_created();
    let highlights = highlights(&daily, &tag_counts, &habit_list, days);

    let mut values = Map::new();
    values.insert("start".into(), json!(start));
    values.insert("end".into(), json!(end));
    values.insert("period".into(), json!(period));
    values.insert("days".into(), json!(days));
    values.insert("compare".into(), json!(compare));
    values.insert("title".into(), json!(title_for(period, &start, &end)));
    values.insert("label".into(), json!(range_label(&start, &end)));
    values.insert("tasks_done".into(), json!(now.tasks.len() as i64));
    values.insert("tasks_prev".into(), json!(before.tasks.len() as i64));
    values.insert("habits_rate".into(), json!(now.habits_rate));
    values.insert("habits_prev".into(), json!(before.habits_rate));
    values.insert("notes_count".into(), json!(notes_created.len() as i64));
    values.insert(
        "notes_prev".into(),
        json!(before.notes_created().len() as i64),
    );
    values.insert("notes_edited".into(), json!(now.notes_edited()));
    values.insert(
        "daily".into(),
        json!(json!(daily
            .iter()
            .cloned()
            .chain(
                std::iter::successors(Some(clock::add_days(&end, 1)?), |d| clock::add_days(d, 1)
                    .ok())
                .take_while(|d| *d <= chart_end)
                .map(|d| (d, 0)),
            )
            .collect::<Vec<_>>()
            .iter()
            .enumerate()
            .map(|(i, (d, n))| json!({
                "day": d,
                "label": short_day(d, i, days).unwrap_or_default(),
                "title": day_label(d).unwrap_or_default(),
                "count": n,
            }))
            .collect::<Vec<_>>())
        .to_string()),
    );
    values.insert(
        "tag_list".into(),
        json!(json!(tag_counts
            .iter()
            .take(12)
            .map(|(t, n)| json!({ "tag": t, "count": n }))
            .collect::<Vec<_>>())
        .to_string()),
    );
    values.insert("habit_list".into(), json!(json!(habit_list).to_string()));
    task_list.truncate(200);
    values.insert("task_list".into(), json!(json!(task_list).to_string()));
    let mut note_list = notes_created.clone();
    note_list.truncate(100);
    values.insert("note_list".into(), json!(json!(note_list).to_string()));
    values.insert("highlights".into(), json!(json!(highlights).to_string()));
    values.insert(
        "summary".into(),
        json!(markdown(
            &title_for(period, &start, &end),
            &range_label(&start, &end),
            &now,
            &habit_list,
            &notes_created
        )),
    );
    let stamp = clock::now_ms() as i64;
    values.insert("updated_at".into(), json!(stamp));

    let skill_id = skill.as_ref().map(|s| s.id.clone()).unwrap_or_default();
    let run_id = run["id"].as_str().map(str::to_string);
    let mut facts = None;
    if let Some(skill) = &skill {
        let sources = ai::Sources::parse(args.opt_str("sources"), &skill.sources);
        let tags = ai::tag_filter(args.opt_str("tags").or(skill.tags.as_deref()));
        let reflection = match args.opt_str("id").filter(|id| !id.is_empty()) {
            Some(id) => ai::reflection(id)?,
            None => None,
        };
        let text = ai::facts(&ai::FactsInput {
            title: &title_for(period, &start, &end),
            label: &range_label(&start, &end),
            compare: &compare,
            days,
            today: &today,
            start: &start,
            end: &end,
            now: &now,
            before: &before,
            habits: &habit_list,
            sources: &sources,
            tags: &tags,
            reflection,
            last_focus: ai::last_focus(&start)?,
        })?;
        values.insert("skill_id".into(), json!(skill.id));
        values.insert("skill_name".into(), json!(skill.name));
        values.insert("prompt".into(), json!(skill.prompt));
        values.insert("model".into(), json!(skill.model));
        values.insert("sources".into(), json!(sources.to_string()));
        values.insert("tag_filter".into(), json!(tags.join(", ")));
        values.insert("facts".into(), json!(text));
        facts = Some(text);
    }
    if let Some(run) = &run_id {
        values.insert("run_id".into(), json!(run));
    }
    if let Some(schedule) = args.opt_str("schedule_id") {
        values.insert("schedule_id".into(), json!(schedule));
    }

    let found = match &run_id {
        Some(run) => by_run(run)?,
        None => None,
    };
    let found = match found {
        Some(id) => Some(id),
        None => existing(period, &start, &end, &skill_id)?,
    };
    let (id, updated) = match found {
        Some(id) => {
            storage::update(TABLE, &id, &values)?;
            (id, true)
        }
        None => {
            values.insert("created_at".into(), json!(stamp));
            (storage::insert(TABLE, &values)?, false)
        }
    };
    let Some(facts) = facts else {
        let message = if updated {
            "Report updated"
        } else {
            "Report ready"
        };
        return Ok(json!({ "id": id, "updated": updated, "message": message }));
    };
    let review = args
        .get("review")
        .is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true"));
    if review && run_id.is_none() {
        ai::set_status(&id, "ready", None)?;
        return Ok(
            json!({ "id": id, "updated": updated, "message": "Check the data, then send it to AI" }),
        );
    }
    let _ = facts;
    let outcome = ai::submit(&id, run_id.as_deref(), None)?;
    Ok(match outcome {
        ai::Submitted::Queued => {
            json!({ "id": id, "updated": updated, "message": "Writing with AI…" })
        }
        ai::Submitted::Unavailable(reason) => json!({
            "id": id,
            "updated": updated,
            "message": format!("Report saved; AI could not write it: {reason}"),
            "$run": { "status": "failed", "message": format!("AI could not write it: {reason}") },
        }),
    })
}

/// The report made by scheduled run `run`, if any.
fn by_run(run: &str) -> Result<Option<String>> {
    let rows = storage::query(
        "SELECT id FROM reports WHERE _deleted IS NULL AND run_id = ?1 LIMIT 1",
        &[json!(run)],
    )?;
    Ok(rows
        .first()
        .and_then(|r| r["id"].as_str())
        .map(String::from))
}

/// A saved report's range, brought up to today while its week or month is
/// still going.
fn refresh_range(id: &str, today: &str) -> Result<(String, String, String)> {
    let rows = storage::query(
        "SELECT period, start, end FROM reports WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let row = rows
        .first()
        .ok_or_else(|| Error::new("not_found", "That report is gone"))?;
    let text = |key: &str| {
        row.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let (period, start, end) = (text("period"), text("start"), text("end"));
    let period = if period.is_empty() {
        "custom".to_string()
    } else {
        period
    };
    if period == "week" || period == "month" {
        let (now_start, now_end) = range_for(&period, today, &Args(Map::new()))?;
        if now_start == start {
            return Ok((period, now_start, now_end));
        }
    }
    Ok((period, start, end))
}

/// The range a report is compared with, and how to say so: the same days
/// of the week or month before for weeks and months, otherwise the span of
/// the same length just before.
fn previous(period: &str, start: &str, end: &str, days: i64) -> Result<(String, String, String)> {
    Ok(match period {
        "week" | "last_week" => (
            clock::add_days(start, -7)?,
            clock::add_days(end, -7)?,
            "vs last week".to_string(),
        ),
        "month" | "last_month" if start.ends_with("-01") => {
            let prev_last = clock::add_days(start, -1)?;
            let prev_first = format!("{}-01", &prev_last[..7]);
            let mut prev_end = clock::add_days(&prev_first, days as i32 - 1)?;
            if prev_end > prev_last {
                prev_end = prev_last;
            }
            (prev_first, prev_end, "vs last month".to_string())
        }
        _ => {
            let prev_end = clock::add_days(start, -1)?;
            (
                clock::add_days(&prev_end, -(days as i32 - 1))?,
                prev_end,
                if days == 1 {
                    "vs the day before".to_string()
                } else {
                    format!("vs the previous {days} days")
                },
            )
        }
    })
}

/// The report this one replaces: the same skill and range, or for this
/// week/month, the one that started on the same day.
fn existing(period: &str, start: &str, end: &str, skill: &str) -> Result<Option<String>> {
    let rows = storage::query(
        "SELECT id FROM reports WHERE _deleted IS NULL AND start = ?1 \
         AND (end = ?2 OR (period = ?3 AND ?3 IN ('week', 'month'))) \
         AND COALESCE(skill_id, '') = ?4 \
         ORDER BY created_at DESC LIMIT 1",
        &[json!(start), json!(end), json!(period), json!(skill)],
    )?;
    Ok(rows
        .first()
        .and_then(|r| r.get("id"))
        .and_then(Value::as_str)
        .map(String::from))
}

/// Saves the reflection: what went well and what to focus on next.
fn reflect(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let mut values = Map::new();
    for key in ["wins", "focus"] {
        if let Some(text) = args.opt_str(key) {
            let text = text.trim();
            if text.chars().count() > 4000 {
                return Err(Error::invalid("Keep each answer under 4000 characters"));
            }
            values.insert(
                key.into(),
                if text.is_empty() {
                    Value::Null
                } else {
                    json!(text)
                },
            );
        }
    }
    if values.is_empty() {
        return Err(Error::invalid("nothing to save"));
    }
    storage::update(TABLE, id, &values)?;
    Ok(Value::Null)
}

pub(crate) fn range_for(period: &str, today: &str, args: &Args) -> Result<(String, String)> {
    let monday = clock::add_days(today, -(clock::weekday(today)? as i32))?;
    let first = format!("{}-01", &today[..7]);
    Ok(match period {
        "week" => (monday, today.to_string()),
        "last_week" => (clock::add_days(&monday, -7)?, clock::add_days(&monday, -1)?),
        "7d" => (clock::add_days(today, -6)?, today.to_string()),
        "month" => (first, today.to_string()),
        "last_month" => {
            let end = clock::add_days(&first, -1)?;
            (format!("{}-01", &end[..7]), end)
        }
        "custom" => {
            let end = date_or(args, "end", today.to_string())?;
            let start = date_or(args, "start", clock::add_days(&end, -6)?)?;
            if start > end {
                return Err(Error::invalid("Start must be on or before the end"));
            }
            if clock::add_days(&start, 366)? < end {
                return Err(Error::invalid("Choose a range of up to a year"));
            }
            (start, end)
        }
        _ => {
            return Err(Error::invalid(
                "`period` is week, last_week, 7d, month, last_month or custom",
            ))
        }
    })
}

fn date_or(args: &Args, key: &str, fallback: String) -> Result<String> {
    match args.opt_str(key).map(str::trim).filter(|d| !d.is_empty()) {
        Some(day) => {
            clock::weekday(day)
                .map_err(|_| Error::invalid(format!("`{key}` must be YYYY-MM-DD")))?;
            Ok(day.to_string())
        }
        None => Ok(fallback),
    }
}

fn days_between(start: &str, end: &str) -> Result<i64> {
    let mut n = 0;
    let mut day = start.to_string();
    while day.as_str() < end && n <= 400 {
        day = clock::add_days(&day, 1)?;
        n += 1;
    }
    Ok(n)
}

fn percent(rate: &Value) -> Option<f64> {
    rate.as_f64().map(|r| (r * 1000.0).round() / 10.0)
}

pub(crate) fn parts(day: &str) -> (&str, usize, u32) {
    let month = day[5..7].parse::<usize>().unwrap_or(1).clamp(1, 12) - 1;
    (&day[..4], month, day[8..10].parse().unwrap_or(1))
}

/// "27 Oct – 2 Nov", "3 – 9 Nov", "28 Dec 2025 – 3 Jan 2026", "5 Nov".
pub(crate) fn range_label(start: &str, end: &str) -> String {
    let (sy, sm, sd) = parts(start);
    let (ey, em, ed) = parts(end);
    if start == end {
        format!("{ed} {} {ey}", MONTHS[em])
    } else if sy != ey {
        format!("{sd} {} {sy} – {ed} {} {ey}", MONTHS[sm], MONTHS[em])
    } else if sm != em {
        format!("{sd} {} – {ed} {} {ey}", MONTHS[sm], MONTHS[em])
    } else {
        format!("{sd} – {ed} {} {ey}", MONTHS[em])
    }
}

/// A title that stays true later; the page shows "This week" / "This
/// month" while the period is current.
pub(crate) fn title_for(period: &str, start: &str, end: &str) -> String {
    let (sy, sm, sd) = parts(start);
    match period {
        "week" | "last_week" => format!("Week of {sd} {}", MONTHS[sm]),
        "month" | "last_month" if sd == 1 => format!("{} {sy}", MONTH_NAMES[sm]),
        _ => range_label(start, end),
    }
}

/// "Mon 3 Nov" for a task's day in the sheet.
pub(crate) fn day_label(day: &str) -> Result<String> {
    let (_, m, d) = parts(day);
    Ok(format!(
        "{} {d} {}",
        WEEKDAYS[clock::weekday(day)? as usize],
        MONTHS[m]
    ))
}

/// The chart's axis label: the weekday for a week or less; for longer
/// ranges the date on every seventh day, so labels don't crowd.
fn short_day(day: &str, index: usize, days: i64) -> Result<String> {
    let (_, m, d) = parts(day);
    Ok(if days <= 7 {
        WEEKDAYS[clock::weekday(day)? as usize].to_string()
    } else if index.is_multiple_of(7) {
        format!("{d} {}", MONTHS[m])
    } else {
        String::new()
    })
}

fn highlights(
    daily: &[(String, i64)],
    tags: &[(String, i64)],
    habits: &[Value],
    days: i64,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some((day, n)) = daily.iter().filter(|(_, n)| *n > 0).max_by_key(|(_, n)| *n) {
        if let Ok(label) = day_label(day) {
            out.push(format!("Busiest day: {label}, {n} {}", plural(*n, "task")));
        }
    }
    let active = daily.iter().filter(|(_, n)| *n > 0).count() as i64;
    if active > 0 && days > 1 {
        out.push(format!("Got something done on {active} of {days} days"));
    }
    if let Some((tag, n)) = tags.first() {
        out.push(format!("Most work tagged #{tag} ({n})"));
    }
    if let Some(best) = habits
        .iter()
        .filter(|h| h["due"].as_i64().unwrap_or(0) > 0 && h["rate"].as_f64() == Some(100.0))
        .map(|h| h["title"].as_str().unwrap_or("Habit"))
        .next()
    {
        out.push(format!("Never missed {best}"));
    }
    out
}

pub(crate) fn plural(n: i64, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

/// The report as Markdown, for copying elsewhere.
fn markdown(title: &str, label: &str, now: &Totals, habits: &[Value], notes: &[Value]) -> String {
    let mut lines = vec![format!("# {title}"), label.to_string()];
    lines.push(format!("## Tasks completed: {}", now.tasks.len()));
    for task in now.tasks.iter().take(50) {
        lines.push(format!(
            "- {}",
            task["title"].as_str().unwrap_or("(untitled)")
        ));
    }
    if now.habits.is_some() {
        lines.push(match now.habits_rate {
            Some(p) => format!("## Habits: {p}% done"),
            None => "## Habits: nothing scheduled".to_string(),
        });
        for habit in habits {
            lines.push(format!(
                "- {}: {} of {}",
                habit["title"].as_str().unwrap_or("Habit"),
                habit["done"],
                habit["due"]
            ));
        }
    }
    if now.notes.is_some() {
        lines.push(format!(
            "## Notes: {} new, {} edited",
            notes.len(),
            now.notes_edited()
        ));
        for title in notes.iter().take(20) {
            lines.push(format!("- {}", title.as_str().unwrap_or("Untitled")));
        }
    }
    lines.join("\n")
}

terrarium_guest::plugin!(Reports);
