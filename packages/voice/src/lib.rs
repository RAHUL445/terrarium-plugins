//! Voice: capture by recording or typing, anywhere. Recordings made on a
//! device without a speech model (the phone) wait in the inbox and are
//! transcribed by the desktop with local Parakeet after sync; the audio is
//! then cleared. Review the text, then send it to To-do (one task, or one
//! per line) or Notes, or keep it in history.

use std::io::Read;

use serde_json::{json, Map, Value};
use terrarium_guest::{clock, interop, speech, storage, Args, Error, Plugin, Result};

struct Voice;

const TABLE: &str = "captures";
/// Base64 of 8 MiB of audio.
const MAX_AUDIO_B64: usize = 11_200_000;
/// Transcriptions per job run, to stay well within the plugin time limit.
const PER_RUN: usize = 3;

impl Plugin for Voice {
    fn handle(operation: &str, args: Args) -> Result<Value> {
        match operation {
            "capture_audio" => capture_audio(&args),
            "capture_text" => capture_text(&args),
            "transcribe_pending" => transcribe_pending(),
            "edit" => {
                let text = clean(args.str("text")?)?;
                set(args.str("id")?, &[("text", json!(text))])
            }
            "retry" => retry(args.str("id")?),
            "archive" => set(
                args.str("id")?,
                &[
                    ("status", json!("archived")),
                    ("sent_to", Value::Null),
                    ("sent_at", json!(clock::now_ms() as i64)),
                ],
            ),
            "reopen" => reopen(args.str("id")?),
            "send" => send(&args),
            "delete" => {
                storage::delete(TABLE, args.str("id")?)?;
                Ok(Value::Null)
            }
            "restore" => {
                storage::restore(TABLE, args.str("id")?)?;
                Ok(Value::Null)
            }
            _ => Err(Error::unknown_operation(operation)),
        }
    }
}

fn set(id: &str, pairs: &[(&str, Value)]) -> Result<Value> {
    let values: Map<String, Value> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    storage::update(TABLE, id, &values)?;
    Ok(Value::Null)
}

fn clean(text: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::invalid("Say or type something first"));
    }
    if text.chars().count() > 20_000 {
        return Err(Error::invalid("Captures are limited to 20,000 characters"));
    }
    Ok(text.to_string())
}

fn capture_text(args: &Args) -> Result<Value> {
    let mut values = Map::new();
    values.insert("text".into(), json!(clean(args.str("text")?)?));
    values.insert("source".into(), json!("typed"));
    values.insert("status".into(), json!("ready"));
    values.insert("created_at".into(), json!(clock::now_ms() as i64));
    Ok(json!({ "id": storage::insert(TABLE, &values)? }))
}

fn validate_audio(audio: &str) -> Result<()> {
    if audio.is_empty() || audio.len() > MAX_AUDIO_B64 {
        return Err(Error::invalid(
            "Recordings are limited to about 8 MB (3 minutes)",
        ));
    }
    let mut decoder = base64::read::DecoderReader::new(
        audio.as_bytes(),
        &base64::engine::general_purpose::STANDARD,
    );
    let mut header = [0u8; 12];
    let count = decoder
        .read(&mut header)
        .map_err(|_| Error::invalid("Recording is not valid base64"))?;
    let bytes = &header[..count];
    if !(bytes.starts_with(b"RIFF")
        || bytes.starts_with(b"OggS")
        || bytes.starts_with(b"ID3")
        || bytes.starts_with(&[0xff, 0xfb])
        || bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        || (bytes.len() >= 8 && &bytes[4..8] == b"ftyp"))
    {
        return Err(Error::invalid(
            "Recording must be WAV, M4A, WebM, Ogg or MP3",
        ));
    }
    let remaining = 8 * 1024 * 1024 - count;
    let decoded = std::io::copy(
        &mut decoder.take(remaining as u64 + 1),
        &mut std::io::sink(),
    )
    .map_err(|_| Error::invalid("Recording is not valid base64"))?;
    if decoded > remaining as u64 {
        return Err(Error::invalid("Recordings are limited to 8 MiB"));
    }
    Ok(())
}

fn capture_audio(args: &Args) -> Result<Value> {
    let audio = args.str("audio")?;
    validate_audio(audio)?;
    let mime = args.opt_str("mime").unwrap_or("audio/webm");
    if mime.len() > 128
        || mime.chars().any(char::is_control)
        || !matches!(
            mime.split(';').next().unwrap_or("").trim(),
            "audio/webm"
                | "audio/ogg"
                | "audio/wav"
                | "audio/x-wav"
                | "audio/mp4"
                | "audio/mpeg"
                | "audio/mp3"
                | "audio/x-m4a"
        )
    {
        return Err(Error::invalid("Unsupported recording type"));
    }
    let mut values = Map::new();
    values.insert("audio".into(), json!(audio));
    values.insert("mime".into(), json!(mime));
    if let Some(ms) = args.get("duration_ms").and_then(Value::as_i64) {
        values.insert("duration_ms".into(), json!(ms));
    }
    values.insert("source".into(), json!("audio"));
    values.insert("status".into(), json!("queued"));
    values.insert("created_at".into(), json!(clock::now_ms() as i64));
    let id = storage::insert(TABLE, &values)?;
    // Transcribe right away where a model is available; otherwise it waits.
    if speech::available() {
        transcribe(&id, audio);
    }
    Ok(json!({ "id": id }))
}

/// Updates the capture with the transcript or the failure; never fails
/// the operation itself, so the recording is kept either way.
fn transcribe(id: &str, audio: &str) {
    let outcome = match speech::transcribe(audio) {
        Ok(text) if !text.trim().is_empty() => set(
            id,
            &[
                ("text", json!(text)),
                ("status", json!("ready")),
                ("audio", Value::Null),
                ("error", Value::Null),
            ],
        ),
        Ok(_) => set(
            id,
            &[
                ("status", json!("failed")),
                ("error", json!("No speech was recognized")),
            ],
        ),
        Err(e) => set(
            id,
            &[("status", json!("failed")), ("error", json!(e.message))],
        ),
    };
    if let Err(e) = outcome {
        terrarium_guest::log(&format!("could not save transcript: {}", e.message));
    }
}

fn transcribe_pending() -> Result<Value> {
    if !speech::available() {
        return Ok(json!({ "transcribed": 0, "reason": "no speech model on this device" }));
    }
    let queued = storage::query(
        "SELECT id FROM captures WHERE _deleted IS NULL AND status = 'queued' AND audio IS NOT NULL ORDER BY id LIMIT ?1",
        &[json!(PER_RUN as i64)],
    )?;
    for row in &queued {
        let id = row["id"].as_str().unwrap_or_default();
        let clip = storage::query(
            "SELECT audio FROM captures WHERE id = ?1 AND _deleted IS NULL",
            &[json!(id)],
        )?;
        if let Some(audio) = clip.first().and_then(|r| r["audio"].as_str()) {
            transcribe(id, audio);
        }
    }
    Ok(json!({ "transcribed": queued.len() }))
}

/// Queues a failed recording again and, where a model is available,
/// transcribes it now. Only possible while its audio is still kept.
fn retry(id: &str) -> Result<Value> {
    let rows = storage::query(
        "SELECT audio FROM captures WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let audio = rows
        .first()
        .ok_or_else(|| Error::new("not-found", "Capture not found"))?["audio"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::invalid("This recording's audio is gone; edit the text instead"))?;
    set(id, &[("status", json!("queued")), ("error", Value::Null)])?;
    if speech::available() {
        transcribe(id, &audio);
    }
    let rows = storage::query("SELECT status FROM captures WHERE id = ?1", &[json!(id)])?;
    Ok(json!({ "status": rows.first().map(|r| r["status"].clone()) }))
}

/// Back to the inbox from history: ready when it has text, otherwise
/// waiting for transcription again.
fn reopen(id: &str) -> Result<Value> {
    let rows = storage::query(
        "SELECT text FROM captures WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let row = rows
        .first()
        .ok_or_else(|| Error::new("not-found", "Capture not found"))?;
    let has_text = row["text"].as_str().is_some_and(|t| !t.trim().is_empty());
    set(
        id,
        &[
            ("status", json!(if has_text { "ready" } else { "queued" })),
            ("sent_to", Value::Null),
            ("sent_at", Value::Null),
        ],
    )
}

/// One task title per non-empty line, without list markers ("- ", "* ",
/// "• ", "1. ", "[ ] ", "[x] "). An unmarked first line above a marked
/// list is a heading, not a task: "Groceries" over "- milk" gives
/// "milk #groceries" (a one-word heading becomes a tag; others are dropped).
fn task_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<(bool, String)> = text
        .lines()
        .map(|line| {
            let mut line = line.trim();
            let original = line;
            // Markers can stack, as in "• [ ] book flights".
            while let Some(rest) = ["[ ] ", "[x] ", "- ", "* ", "• "]
                .iter()
                .find_map(|marker| line.strip_prefix(marker))
            {
                line = rest.trim_start();
            }
            if let Some((num, rest)) = line.split_once(['.', ')']) {
                if !num.is_empty() && num.len() <= 3 && num.bytes().all(|b| b.is_ascii_digit()) {
                    line = rest.trim_start();
                }
            }
            (line != original, line.chars().take(500).collect::<String>())
        })
        .filter(|(_, line)| !line.is_empty())
        .collect();
    let heading = match lines.as_slice() {
        [(false, first), rest @ ..]
            if !rest.is_empty() && rest.iter().all(|(marked, _)| *marked) =>
        {
            Some(first.trim_end_matches(':').to_string())
        }
        _ => None,
    };
    if let Some(heading) = heading {
        lines.remove(0);
        if !heading.is_empty()
            && heading
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            let tag = heading.to_lowercase();
            for (_, line) in &mut lines {
                line.push_str(" #");
                line.push_str(&tag);
            }
        }
    }
    lines.into_iter().map(|(_, line)| line).collect()
}

fn send(args: &Args) -> Result<Value> {
    let id = args.str("id")?;
    let rows = storage::query(
        "SELECT text, status FROM captures WHERE id = ?1 AND _deleted IS NULL",
        &[json!(id)],
    )?;
    let row = rows
        .first()
        .ok_or_else(|| Error::new("not-found", "Capture not found"))?;
    let text = match args.opt_str("text") {
        Some(edited) => clean(edited)?,
        None => clean(row["text"].as_str().unwrap_or(""))?,
    };
    let target = args.str("to")?;
    let split = matches!(args.get("split"), Some(Value::Bool(true)))
        || matches!(args.opt_str("split"), Some("true" | "1"));
    let created = match target {
        "todo" if split => {
            let lines = task_lines(&text);
            for line in &lines {
                interop::call("tasks", "create", &json!({ "title": line }))?;
            }
            json!({ "count": lines.len() })
        }
        "todo" => {
            let title: String = text
                .lines()
                .next()
                .unwrap_or(&text)
                .chars()
                .take(500)
                .collect();
            interop::call("tasks", "create", &json!({ "title": title }))?
        }
        "notes" => interop::call("notes", "create", &json!({ "body": text }))?,
        _ => return Err(Error::invalid("Send to todo or notes")),
    };
    set(
        id,
        &[
            ("text", json!(text)),
            ("status", json!("sent")),
            ("sent_to", json!(target)),
            ("sent_at", json!(clock::now_ms() as i64)),
        ],
    )?;
    let count = created["count"].as_u64().unwrap_or(1);
    Ok(json!({
        "sent_to": target,
        "id": created["id"],
        "count": count,
        "message": match (target, count) {
            ("notes", _) => "Sent to Notes".to_string(),
            (_, 1) => "Sent to To-do".to_string(),
            (_, n) => format!("Sent {n} tasks to To-do"),
        },
    }))
}

terrarium_guest::plugin!(Voice);

#[cfg(test)]
mod security_tests {
    use super::*;
    use base64::Engine;
    #[test]
    fn recordings_reject_bad_encoding_unsupported_data_and_decoded_oversize() {
        let wav = "UklGRiQAAABXQVZFZm10IBAAAAABAAEAgD4AAAB9AAACABAAZGF0YQAAAAA=";
        assert!(validate_audio(wav).is_ok());
        assert!(validate_audio("not base64!").is_err());
        assert!(validate_audio("aGVsbG8=").is_err());
        let mut oversized = b"RIFF".to_vec();
        oversized.resize(8 * 1024 * 1024 + 1, 0);
        assert!(
            validate_audio(&base64::engine::general_purpose::STANDARD.encode(oversized)).is_err()
        );
    }
}
