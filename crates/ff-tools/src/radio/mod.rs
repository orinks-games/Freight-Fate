//! The radio dial's data tools: the community stations folded into the
//! shipped catalog, and the two stream sweeps that report which stations
//! still play.

pub mod audit_streams;
pub mod check_streams;
pub mod fold_community;

use std::path::PathBuf;

use crate::pyjson::{self, Object, Value};
use crate::util::{self, Result};

fn data_path(name: &str) -> PathBuf {
    util::repo_root().join("data").join(name)
}

/// The `stations` rows of a catalog file, each tagged with its tier.
fn catalog_rows(name: &str, tier: &str) -> Result<Vec<Object>> {
    let catalog = pyjson::load_path(&data_path(name))?;
    Ok(catalog
        .get("stations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .map(|row| {
            let mut row = row.clone();
            row.insert("_tier", Value::from(tier));
            row
        })
        .collect())
}

fn text<'a>(row: &'a Object, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}

/// A row's `stream_url`, when it is an http or https address.
fn web_stream(row: &Object) -> Option<&str> {
    let url = text(row, "stream_url");
    (url.starts_with("http://") || url.starts_with("https://")).then_some(url)
}

/// Run `job` over `items` on `workers` threads; results come back in input
/// order. `progress` is called with the count done after each one.
fn parallel<T: Sync, R: Send>(
    items: &[T],
    workers: usize,
    job: impl Fn(&T) -> R + Sync,
    progress: impl Fn(usize) + Sync,
) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..workers.max(1) {
            scope.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::SeqCst);
                let Some(item) = items.get(k) else { break };
                let result = job(item);
                if let Ok(mut slots) = results.lock() {
                    slots[k] = Some(result);
                }
                progress(done.fetch_add(1, Ordering::SeqCst) + 1);
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .collect()
}

/// The status and headers (names lowercased) of the LAST response in a
/// curl `-D` dump, so a redirect's own answer is passed over.
fn parse_headers(raw: &str) -> (u16, Vec<(String, String)>) {
    let normalized = raw.replace("\r\n", "\n");
    let Some(block) = normalized
        .split("\n\n")
        .filter(|b| !util::py_strip(b).is_empty())
        .last()
    else {
        return (0, Vec::new());
    };
    let mut lines = block.lines();
    let first = util::py_strip(lines.next().unwrap_or(""));
    let mut words = first.split_whitespace();
    let status = match words.next() {
        Some(w) if w.starts_with("HTTP/") || w == "ICY" => words
            .next()
            .filter(|c| c.len() == 3)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0),
        _ => 0,
    };
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| {
            (
                util::py_strip(k).to_lowercase(),
                util::py_strip(v).to_string(),
            )
        })
        .collect();
    (status, headers)
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .rev()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_response_in_a_header_dump_wins() {
        let raw = "HTTP/1.1 302 Found\r\nLocation: x\r\n\r\nICY 200 OK\r\nicy-name: KBOO\r\nContent-Type: audio/mpeg\r\n\r\n";
        let (status, headers) = parse_headers(raw);
        assert_eq!(status, 200);
        assert_eq!(header(&headers, "icy-name"), Some("KBOO"));
        assert_eq!(header(&headers, "location"), None);
        assert_eq!(parse_headers("").0, 0);
    }
}
