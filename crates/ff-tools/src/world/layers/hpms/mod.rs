//! FHWA's Highway Performance Monitoring System (HPMS), read from the
//! national layer hosted on ArcGIS Living Atlas (Federal User Community
//! `hpms_v2_view`): traffic volumes (`hpms-traffic`) and terrain class
//! (`hpms-terrain`).
//!
//! Every answer is cached under `.route-cache/hpms/`, keyed by the query,
//! so a re-run is free and an interrupted crawl resumes where it stopped.
//! `--refresh` asks the server again.

pub mod terrain;
pub mod traffic;

use std::path::PathBuf;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::http;
use crate::util::{self, Result};

pub const QUERY_URL: &str = "https://services2.arcgis.com/FiaPA4ga0iQKduv3/arcgis/rest/services/\
                             hpms_v2_view/FeatureServer/0/query";

/// A query client with its cache.
pub struct Client {
    agent: ureq::Agent,
    cache: PathBuf,
    refresh: bool,
    pub fetched: std::sync::atomic::AtomicUsize,
}

fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

impl Client {
    pub fn new(refresh: bool) -> Self {
        Self {
            agent: http::agent(Some(Duration::from_secs(120))),
            cache: util::repo_root().join(".route-cache").join("hpms"),
            refresh,
            fetched: Default::default(),
        }
    }

    /// One query (`params` in order), from the cache when it has it.
    pub fn query(&self, params: &[(&str, String)]) -> Result<serde_json::Value> {
        let query: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{k}={}", encode(v)))
            .collect();
        let url = format!("{QUERY_URL}?{}", query.join("&"));
        let key = hex::encode(Sha256::digest(url.as_bytes()));
        let path = self.cache.join(format!("{}.json", &key[..24]));
        if !self.refresh {
            if let Some(text) = util::read_text_lf(&path) {
                if let Ok(v) = serde_json::from_str(&text) {
                    return Ok(v);
                }
            }
        }
        let mut last = String::new();
        for attempt in 0..4 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_secs(2 << attempt));
            }
            match http::request(&self.agent, "GET", &url, &[], None) {
                Ok(r) if r.status == 200 => {
                    let text = String::from_utf8_lossy(&r.body).into_owned();
                    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
                        format!("HPMS answered with something that is not JSON: {e}")
                    })?;
                    if v.get("error").is_some() {
                        last = format!("HPMS error: {}", v["error"]);
                        continue;
                    }
                    std::fs::create_dir_all(&self.cache).map_err(|e| e.to_string())?;
                    util::write_text(&path, &text)?;
                    self.fetched
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return Ok(v);
                }
                Ok(r) => last = format!("HPMS answered {}", r.status),
                Err(e) => last = e,
            }
        }
        Err(last)
    }
}

/// Today's date, for the "accessed" in a source note.
pub fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// `xmin,ymin,xmax,ymax` the way the Python wrote it.
pub fn envelope(b: (f64, f64, f64, f64)) -> String {
    format!("{:.5},{:.5},{:.5},{:.5}", b.0, b.1, b.2, b.3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(
            encode("F_SYSTEM=1 AND AADT>0"),
            "F_SYSTEM%3D1%20AND%20AADT%3E0"
        );
        assert_eq!(
            envelope((-97.0, 30.0, -96.5, 30.25)),
            "-97.00000,30.00000,-96.50000,30.25000"
        );
    }
}
