// @group BusinessLogic : Read historical log lines from disk
// @group BusinessLogic > DatedLogs : Read lines from daily-rotated dated log files

use anyhow::Result;
use chrono::{Local, NaiveDate, TimeZone, Utc};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

pub const MAX_LOG_LINES: usize = 5_000;
const MAX_TAIL_BYTES: u64 = 8 * 1024 * 1024;
const TAIL_CHUNK_BYTES: u64 = 64 * 1024;
const MAX_STATS_SCAN_BYTES: u64 = 64 * 1024 * 1024;

/// Read the last `n` lines from a single log file.
pub fn read_last_lines(path: &Path, n: usize) -> Result<Vec<String>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let n = n.clamp(1, MAX_LOG_LINES);
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut position = file_len;
    let mut chunks = Vec::new();
    let mut newline_count = 0usize;
    let mut bytes_read = 0u64;

    while position > 0 && newline_count <= n && bytes_read < MAX_TAIL_BYTES {
        let chunk_len = position
            .min(TAIL_CHUNK_BYTES)
            .min(MAX_TAIL_BYTES - bytes_read);
        position -= chunk_len;
        file.seek(SeekFrom::Start(position))?;
        let mut chunk = vec![0; chunk_len as usize];
        file.read_exact(&mut chunk)?;
        newline_count += chunk.iter().filter(|byte| **byte == b'\n').count();
        bytes_read += chunk_len;
        chunks.push(chunk);
    }

    chunks.reverse();
    let bytes: Vec<u8> = chunks.into_iter().flatten().collect();
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<String> = text.lines().map(ToString::to_string).collect();
    if position > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    let start = lines.len().saturating_sub(n);
    Ok(lines[start..].to_vec())
}

/// Read both out.log and err.log, merge, sort by timestamp, return the last `n` lines.
/// Returns tuples of (stream, timestamp, content).
/// This is today's live logs (current files only).
pub fn read_merged_logs(log_dir: &Path, n: usize) -> Result<Vec<(String, String, String)>> {
    read_merged_logs_for_paths(&log_dir.join("out.log"), &log_dir.join("err.log"), n)
}

/// Read logs for a specific date from the dated rotation files:
///   out.log.YYYY-MM-DD  /  err.log.YYYY-MM-DD
/// Returns merged lines sorted by timestamp as (stream, timestamp, content).
pub fn read_merged_logs_for_date(
    log_dir: &Path,
    date: NaiveDate,
    n: usize,
) -> Result<Vec<(String, String, String)>> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let out_path = log_dir.join(format!("out.log.{date_str}"));
    let err_path = log_dir.join(format!("err.log.{date_str}"));
    read_merged_logs_for_paths(&out_path, &err_path, n)
}

/// List all dates for which rotated log files exist, sorted newest-first.
pub fn list_log_dates(log_dir: &Path) -> Result<Vec<NaiveDate>> {
    let mut dates: Vec<NaiveDate> = Vec::new();

    if !log_dir.exists() {
        return Ok(dates);
    }

    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        let fname = entry.file_name().to_string_lossy().to_string();
        // Match "out.log.YYYY-MM-DD" (avoid counting err.log dates twice)
        if let Some(date_str) = fname.strip_prefix("out.log.") {
            if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                dates.push(date);
            }
        }
    }

    // Newest first
    dates.sort_by(|a, b| b.cmp(a));
    Ok(dates)
}

// @group BusinessLogic > LogStats : One 5-minute bucket of log line counts for today's chart
#[derive(Debug, Clone, serde::Serialize)]
pub struct DayLogBucket {
    /// UTC ISO-8601 start of this 5-minute window
    pub window_start: String,
    pub stdout_count: u64,
    pub stderr_count: u64,
}

// @group BusinessLogic > LogStats : Read today's out.log + err.log, bucket lines by 5-min intervals
/// Scans at most the newest 64 MiB from each current log file. This keeps the
/// request bounded if a noisy process produces an unexpectedly large log.
/// Lines whose timestamp does not match today are ignored.
pub fn read_log_stats_today(log_dir: &Path) -> Result<Vec<DayLogBucket>> {
    use std::collections::BTreeMap;

    let today_local = Local::now().date_naive();
    let bucket_secs: i64 = 300; // 5 minutes

    // BTreeMap keyed by bucket_start Unix timestamp — gives us sorted, gapless output
    let mut buckets: BTreeMap<i64, (u64, u64)> = BTreeMap::new();

    for (path, is_stdout) in [
        (log_dir.join("out.log"), true),
        (log_dir.join("err.log"), false),
    ] {
        if !path.exists() {
            continue;
        }
        let mut file = File::open(&path)?;
        let file_len = file.metadata()?.len();
        let was_truncated = file_len > MAX_STATS_SCAN_BYTES;
        if was_truncated {
            file.seek(SeekFrom::Start(file_len - MAX_STATS_SCAN_BYTES))?;
        }
        let mut reader = BufReader::new(file);
        if was_truncated {
            // The seek point may be in the middle of a UTF-8 log line. Discard
            // that partial line before parsing the bounded tail.
            let mut partial = Vec::new();
            reader.read_until(b'\n', &mut partial)?;
            tracing::warn!(
                path = %path.display(),
                max_bytes = MAX_STATS_SCAN_BYTES,
                "log stats scan was truncated to its bounded tail"
            );
        }

        for raw in reader.lines() {
            let raw = raw?;
            let (ts_str, _) = parse_log_line(&raw);
            if ts_str.is_empty() {
                continue;
            }
            // Parse the ISO-8601 UTC timestamp written by the LogWriter
            let Ok(dt_utc) = ts_str.parse::<chrono::DateTime<Utc>>() else {
                continue;
            };
            // Only include lines from today (in local time)
            let dt_local = dt_utc.with_timezone(&Local);
            if dt_local.date_naive() != today_local {
                continue;
            }
            // Floor to the nearest 5-minute bucket
            let secs = dt_utc.timestamp();
            let bucket_key = secs - (secs % bucket_secs);
            let entry = buckets.entry(bucket_key).or_insert((0, 0));
            if is_stdout {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }
    }

    // Convert to serialisable structs, preserving chronological order
    let result = buckets
        .into_iter()
        .map(|(key, (out, err))| {
            let window_start = Utc
                .timestamp_opt(key, 0)
                .single()
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();
            DayLogBucket {
                window_start,
                stdout_count: out,
                stderr_count: err,
            }
        })
        .collect();

    Ok(result)
}

// @group Utilities : Extract [ISO8601] timestamp prefix from a disk log line
fn parse_log_line(raw: &str) -> (String, String) {
    // Disk format: [2026-02-28T14:23:45.123Z] actual content
    if raw.starts_with('[') {
        if let Some(end) = raw.find("] ") {
            return (raw[1..end].to_string(), raw[end + 2..].to_string());
        }
    }
    (String::new(), raw.to_string())
}

// @group Utilities : Shared helper — merge stdout + stderr paths into sorted (stream, timestamp, content) tuples

fn read_merged_logs_for_paths(
    out_path: &Path,
    err_path: &Path,
    n: usize,
) -> Result<Vec<(String, String, String)>> {
    let mut entries: Vec<(String, String, String)> = Vec::new();

    for (path, stream) in [(out_path, "stdout"), (err_path, "stderr")] {
        for line in read_last_lines(path, n)? {
            let (ts, content) = parse_log_line(&line);
            entries.push((stream.to_string(), ts, content));
        }
    }

    // Sort by the ISO timestamp field (index 1)
    entries.sort_by(|a, b| a.1.cmp(&b.1));

    let start = entries.len().saturating_sub(n);
    Ok(entries[start..].to_vec())
}

// @group UnitTests : parse_log_line and read_last_lines
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // @group UnitTests > ParseLogLine : Well-formed disk format splits at "] "
    #[test]
    fn test_parse_log_line_valid() {
        let (ts, content) = parse_log_line("[2026-03-30T12:00:00Z] hello world");
        assert_eq!(ts, "2026-03-30T12:00:00Z");
        assert_eq!(content, "hello world");
    }

    // @group UnitTests > ParseLogLine : Lines without the bracket prefix are returned as-is
    #[test]
    fn test_parse_log_line_no_bracket() {
        let (ts, content) = parse_log_line("plain log line");
        assert!(ts.is_empty());
        assert_eq!(content, "plain log line");
    }

    // @group UnitTests > ParseLogLine : Empty string returns two empty strings
    #[test]
    fn test_parse_log_line_empty() {
        let (ts, content) = parse_log_line("");
        assert!(ts.is_empty());
        assert!(content.is_empty());
    }

    // @group UnitTests > ParseLogLine : Bracket without closing "] " is treated as no-prefix
    #[test]
    fn test_parse_log_line_unclosed_bracket() {
        let (ts, content) = parse_log_line("[no closing bracket");
        assert!(ts.is_empty());
        assert_eq!(content, "[no closing bracket");
    }

    // @group UnitTests > ParseLogLine : Content after "] " may itself contain brackets
    #[test]
    fn test_parse_log_line_content_with_brackets() {
        let (ts, content) = parse_log_line("[2026-01-01T00:00:00Z] [INFO] server started");
        assert_eq!(ts, "2026-01-01T00:00:00Z");
        assert_eq!(content, "[INFO] server started");
    }

    // @group UnitTests > ReadLastLines : Non-existent path returns empty vec (no error)
    #[test]
    fn test_read_last_lines_missing_file() {
        let result = read_last_lines(Path::new("/nonexistent/path/to/log.txt"), 10).unwrap();
        assert!(result.is_empty());
    }

    // @group UnitTests > ReadLastLines : Returns only the last n lines of a file
    #[test]
    fn test_read_last_lines_tail() {
        let mut tmp = tempfile_in_temp_dir("alter_test_tail.log");
        for i in 1..=10u32 {
            writeln!(tmp, "line {i}").unwrap();
        }
        let path = tmp_path("alter_test_tail.log");
        let lines = read_last_lines(&path, 3).unwrap();
        assert_eq!(lines, vec!["line 8", "line 9", "line 10"]);
        let _ = std::fs::remove_file(&path);
    }

    // @group UnitTests > ReadLastLines : Requesting more lines than exist returns all lines
    #[test]
    fn test_read_last_lines_more_than_exist() {
        let mut tmp = tempfile_in_temp_dir("alter_test_all.log");
        writeln!(tmp, "only line").unwrap();
        let path = tmp_path("alter_test_all.log");
        let lines = read_last_lines(&path, 100).unwrap();
        assert_eq!(lines, vec!["only line"]);
        let _ = std::fs::remove_file(&path);
    }

    // @group UnitTests > ReadLastLines : Empty file returns empty vec
    #[test]
    fn test_read_last_lines_empty_file() {
        let _tmp = tempfile_in_temp_dir("alter_test_empty.log");
        let path = tmp_path("alter_test_empty.log");
        let lines = read_last_lines(&path, 10).unwrap();
        assert!(lines.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    // @group UnitTests > ReadLastLines : Untrusted line counts are capped to prevent memory blowups
    #[test]
    fn test_read_last_lines_clamps_large_request() {
        let name = format!("alter_test_clamp_{}.log", uuid::Uuid::new_v4());
        let path = tmp_path(&name);
        let mut tmp = std::fs::File::create(&path).unwrap();
        for index in 0..=MAX_LOG_LINES {
            writeln!(tmp, "line {index}").unwrap();
        }
        drop(tmp);

        let lines = read_last_lines(&path, usize::MAX).unwrap();
        assert_eq!(lines.len(), MAX_LOG_LINES);
        assert_eq!(lines.first().map(String::as_str), Some("line 1"));
        assert_eq!(lines.last().map(String::as_str), Some("line 5000"));
        let _ = std::fs::remove_file(&path);
    }

    // @group TestHelpers : Write a named file in the OS temp dir, return the open handle
    fn tempfile_in_temp_dir(name: &str) -> std::fs::File {
        std::fs::File::create(tmp_path(name)).unwrap()
    }

    fn tmp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(name)
    }
}
