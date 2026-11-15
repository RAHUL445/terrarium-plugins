//! Quick add: reads dates, times, tags, priority and repeats out of a task
//! title, e.g. "Call mum tomorrow 6pm #family !high". Calendar maths is done
//! here without the host so it can be tested natively.

#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub title: String,
    pub due: Option<String>,
    /// Local `HH:MM`.
    pub time: Option<String>,
    pub tags: Vec<String>,
    pub priority: Option<i64>,
    pub repeat: Option<&'static str>,
}

pub const REPEATS: [&str; 4] = ["daily", "weekdays", "weekly", "monthly"];

const WEEKDAYS: [&[&str]; 7] = [
    &["monday", "mon"],
    &["tuesday", "tue", "tues"],
    &["wednesday", "wed"],
    &["thursday", "thu", "thur", "thurs"],
    &["friday", "fri"],
    &["saturday"],
    &["sunday"],
];

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// `today` is the local date, `YYYY-MM-DD`.
pub fn parse(text: &str, today: &str) -> Parsed {
    let words: Vec<&str> = text.split_whitespace().collect();
    let lower: Vec<String> = words
        .iter()
        .map(|w| w.trim_end_matches([',', '.', ';']).to_lowercase())
        .collect();
    let mut used = vec![false; words.len()];
    let mut out = Parsed::default();
    let today_days = to_days(today).unwrap_or(0);

    let mut i = 0;
    while i < words.len() {
        let w = lower[i].as_str();
        let next = lower.get(i + 1).map(String::as_str);
        let mut take = 0;

        if let Some(tag) = w
            .strip_prefix('#')
            .filter(|t| !t.is_empty() && !t.starts_with('#'))
        {
            if !out.tags.iter().any(|t| t == tag) {
                out.tags.push(tag.to_string());
            }
            take = 1;
        } else if let Some(p) = priority(w) {
            out.priority = Some(p);
            take = 1;
        } else if w == "every" && next.is_some() && out.repeat.is_none() {
            let n = next.unwrap_or_default();
            if let Some(repeat) = match n {
                "day" => Some("daily"),
                "weekday" => Some("weekdays"),
                "week" => Some("weekly"),
                "month" => Some("monthly"),
                _ => None,
            } {
                out.repeat = Some(repeat);
                take = 2;
            } else if let Some(day) = weekday_name(n) {
                out.repeat = Some("weekly");
                out.due
                    .get_or_insert_with(|| next_weekday(today_days, day, true));
                take = 2;
            }
        } else if out.due.is_none() {
            // Dates, optionally after "on", "by" or "due".
            let lead = usize::from(matches!(w, "on" | "by" | "due") && next.is_some());
            let (word, after) = (
                lower[i + lead].as_str(),
                lower.get(i + lead + 1).map(String::as_str),
            );
            if let Some((due, n)) = date(word, after, today_days) {
                out.due = Some(due);
                take = lead + n;
            }
        }
        if take == 0 && out.time.is_none() {
            let lead = usize::from(w == "at" && next.is_some());
            let word = lower[i + lead].as_str();
            let after = lower.get(i + lead + 1).map(String::as_str);
            if let Some((time, n)) = time(word, after, lead == 1) {
                out.time = Some(time);
                take = lead + n;
            }
        }
        for slot in used.iter_mut().skip(i).take(take) {
            *slot = true;
        }
        i += take.max(1);
    }

    let title: Vec<&str> = words
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|(w, _)| *w)
        .collect();
    out.title = if title.is_empty() {
        text.trim().to_string()
    } else {
        title.join(" ")
    };
    out
}

fn priority(w: &str) -> Option<i64> {
    Some(match w {
        "!" | "!1" | "!low" | "!l" => 1,
        "!!" | "!2" | "!med" | "!medium" | "!m" => 2,
        "!!!" | "!3" | "!high" | "!h" | "!urgent" => 3,
        _ => return None,
    })
}

fn weekday_name(w: &str) -> Option<u8> {
    WEEKDAYS
        .iter()
        .position(|names| names.contains(&w))
        .map(|i| i as u8)
}

fn month_name(w: &str) -> Option<u32> {
    if w.len() < 3 {
        return None;
    }
    let full = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    MONTHS
        .iter()
        .zip(full)
        .position(|(short, long)| w == *short || long.starts_with(w) && w.len() >= 3)
        .map(|i| i as u32 + 1)
}

fn day_number(w: &str) -> Option<u32> {
    let digits = w
        .trim_end_matches("st")
        .trim_end_matches("nd")
        .trim_end_matches("rd")
        .trim_end_matches("th");
    let n: u32 = digits.parse().ok()?;
    (1..=31).contains(&n).then_some(n)
}

/// A date starting at `w`; returns it and how many words it used.
fn date(w: &str, next: Option<&str>, today: i64) -> Option<(String, usize)> {
    match w {
        "today" | "tod" | "tonight" => return Some((from_days(today), 1)),
        "tomorrow" | "tmr" | "tmrw" => return Some((from_days(today + 1), 1)),
        _ => {}
    }
    if w == "next" {
        let n = next?;
        if n == "week" {
            return Some((next_weekday(today, 0, false), 2));
        }
        if n == "month" {
            let (y, m, _) = civil(today);
            let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            return Some((format!("{y:04}-{m:02}-01"), 2));
        }
        return weekday_name(n).map(|d| (next_weekday(today, d, false), 2));
    }
    if let Some(day) = weekday_name(w) {
        return Some((next_weekday(today, day, true), 1));
    }
    if w.len() == 10 && to_days(w).is_some() {
        return Some((w.to_string(), 1));
    }
    // "5 nov" / "5th november" / "nov 5".
    let (day, month) = match (day_number(w), next.and_then(month_name)) {
        (Some(d), Some(m)) => (d, m),
        _ => match (month_name(w), next.and_then(day_number)) {
            (Some(m), Some(d)) => (d, m),
            _ => return None,
        },
    };
    let (year, _, _) = civil(today);
    for y in [year, year + 1] {
        if day <= days_in_month(y, month) {
            let days = days_from_civil(y, month, day);
            if days >= today {
                return Some((from_days(days), 2));
            }
        }
    }
    None
}

/// "6pm", "6:30 pm", "18:00", or a bare hour after "at".
fn time(w: &str, next: Option<&str>, after_at: bool) -> Option<(String, usize)> {
    let (body, suffix, used) = if let Some(b) = w.strip_suffix("am") {
        (b, Some(false), 1)
    } else if let Some(b) = w.strip_suffix("pm") {
        (b, Some(true), 1)
    } else if matches!(next, Some("am" | "pm")) {
        (w, Some(next == Some("pm")), 2)
    } else {
        (w, None, 1)
    };
    let (h, m) = match body.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        Some(_) => return None,
        None if suffix.is_some() || after_at => (body.parse::<u32>().ok()?, 0),
        None => return None,
    };
    if body.is_empty() || !body.chars().all(|c| c.is_ascii_digit() || c == ':') || m > 59 {
        return None;
    }
    let h = match suffix {
        Some(pm) if (1..=12).contains(&h) => (h % 12) + if pm { 12 } else { 0 },
        Some(_) => return None,
        None if h <= 23 => h,
        None => return None,
    };
    Some((format!("{h:02}:{m:02}"), used))
}

/// The next `weekday` (Monday = 0) after `today`, or today itself when
/// `include_today`.
fn next_weekday(today: i64, weekday: u8, include_today: bool) -> String {
    let now = weekday_of(today) as i64;
    let mut ahead = (weekday as i64 - now).rem_euclid(7);
    if ahead == 0 && !include_today {
        ahead = 7;
    }
    from_days(today + ahead)
}

/// The next due date of a repeating task after `due`, past `today`.
pub fn next_due(repeat: &str, due: &str, today: &str) -> Option<String> {
    let today = to_days(today)?;
    let mut day = to_days(due).unwrap_or(today);
    let start = day;
    loop {
        day = match repeat {
            "daily" => day + 1,
            "weekly" => day + 7,
            "weekdays" => {
                let mut d = day + 1;
                while weekday_of(d) >= 5 {
                    d += 1;
                }
                d
            }
            "monthly" => {
                let (_, _, want) = civil(start);
                let (y, m, _) = civil(day);
                let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                days_from_civil(y, m, want.min(days_in_month(y, m)))
            }
            _ => return None,
        };
        if day > today {
            return Some(from_days(day));
        }
    }
}

// ---- Calendar ------------------------------------------------------------

pub fn to_days(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || date.len() != 10 || !(1..=12).contains(&m) {
        return None;
    }
    (1..=days_in_month(y, m))
        .contains(&d)
        .then(|| days_from_civil(y, m, d))
}

fn from_days(days: i64) -> String {
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn weekday_of(days: i64) -> u8 {
    // 1970-01-01 was a Thursday (3 with Monday = 0).
    (days + 3).rem_euclid(7) as u8
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

// Howard Hinnant's days_from_civil / civil_from_days.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (
        if m <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        m,
        d,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-02 is a Monday.
    const TODAY: &str = "2026-11-02";

    #[test]
    fn reads_everything() {
        let p = parse("Call mum tomorrow 6pm #family !high", TODAY);
        assert_eq!(p.title, "Call mum");
        assert_eq!(p.due.as_deref(), Some("2026-11-03"));
        assert_eq!(p.time.as_deref(), Some("18:00"));
        assert_eq!(p.tags, ["family"]);
        assert_eq!(p.priority, Some(3));
    }

    #[test]
    fn dates() {
        let due = |t: &str| parse(t, TODAY).due;
        assert_eq!(due("Pay rent friday").as_deref(), Some("2026-11-06"));
        assert_eq!(due("Standup monday").as_deref(), Some("2026-11-02"));
        assert_eq!(due("Plan next monday").as_deref(), Some("2026-11-09"));
        assert_eq!(due("Review next week").as_deref(), Some("2026-11-09"));
        assert_eq!(due("Gift on 5 dec").as_deref(), Some("2026-12-05"));
        assert_eq!(due("Gift nov 1").as_deref(), Some("2027-11-01"));
        assert_eq!(due("Taxes 2027-01-31").as_deref(), Some("2027-01-31"));
        assert_eq!(due("Read the may report"), None);
        assert_eq!(
            parse("Read the may report", TODAY).title,
            "Read the may report"
        );
    }

    #[test]
    fn times() {
        let time = |t: &str| parse(t, TODAY).time;
        assert_eq!(time("Gym at 7").as_deref(), Some("07:00"));
        assert_eq!(time("Gym 7:30 am").as_deref(), Some("07:30"));
        assert_eq!(time("Gym 12am").as_deref(), Some("00:00"));
        assert_eq!(time("Call 18:45").as_deref(), Some("18:45"));
        assert_eq!(time("Buy 2 apples"), None);
        assert_eq!(parse("Buy 2 apples", TODAY).title, "Buy 2 apples");
    }

    #[test]
    fn repeats() {
        let p = parse("Water plants every week", TODAY);
        assert_eq!(
            (p.title.as_str(), p.repeat),
            ("Water plants", Some("weekly"))
        );
        let p = parse("Bins every thursday", TODAY);
        assert_eq!(p.due.as_deref(), Some("2026-11-05"));
        assert_eq!(
            next_due("daily", "2026-10-30", TODAY).as_deref(),
            Some("2026-11-03")
        );
        assert_eq!(
            next_due("weekdays", "2026-11-06", TODAY).as_deref(),
            Some("2026-11-09")
        );
        assert_eq!(
            next_due("weekly", "2026-11-02", TODAY).as_deref(),
            Some("2026-11-09")
        );
        assert_eq!(
            next_due("monthly", "2027-01-31", TODAY).as_deref(),
            Some("2027-02-28")
        );
        assert_eq!(
            next_due("monthly", "2026-10-31", TODAY).as_deref(),
            Some("2026-11-30")
        );
    }

    #[test]
    fn calendar() {
        assert_eq!(from_days(to_days("2024-02-29").unwrap()), "2024-02-29");
        assert_eq!(weekday_of(to_days(TODAY).unwrap()), 0);
        assert_eq!(to_days("2026-02-30"), None);
    }
}
