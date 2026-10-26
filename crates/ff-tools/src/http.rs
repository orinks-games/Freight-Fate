//! HTTP for the tools: the platform's certificate store, statuses returned
//! as data, and errors worded for the person running the tool.

use std::io::Read;
use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig};
use ureq::Agent;

/// One answer: the status and the body.
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// A client. `timeout` bounds the whole exchange when given; a download of
/// a large pack passes `None` and is bounded only on connecting.
pub fn agent(timeout: Option<Duration>) -> Agent {
    let mut config = Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(30)))
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        );
    if let Some(t) = timeout {
        config = config.timeout_global(Some(t));
    }
    Agent::new_with_config(config.build())
}

fn describe(err: &ureq::Error) -> String {
    match err {
        ureq::Error::Timeout(t) => format!("timed out ({t})"),
        ureq::Error::HostNotFound => "host not found".into(),
        ureq::Error::ConnectionFailed => "connection failed".into(),
        other => other.to_string(),
    }
}

fn collect(mut response: ureq::http::Response<ureq::Body>) -> Result<Response, String> {
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .with_config()
        .limit(256 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| describe(&e))?;
    Ok(Response { status, body })
}

/// `method url` with these headers and an optional body.
pub fn request(
    agent: &Agent,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<Response, String> {
    let response = match (method, body) {
        ("GET", _) => {
            let mut req = agent.get(url);
            for (k, v) in headers {
                req = req.header(*k, *v);
            }
            req.call()
        }
        ("DELETE", _) => {
            let mut req = agent.delete(url);
            for (k, v) in headers {
                req = req.header(*k, *v);
            }
            req.call()
        }
        ("PATCH", data) => {
            let mut req = agent.patch(url);
            for (k, v) in headers {
                req = req.header(*k, *v);
            }
            req.send(data.unwrap_or(&[]))
        }
        (_, data) => {
            let mut req = agent.post(url);
            for (k, v) in headers {
                req = req.header(*k, *v);
            }
            req.send(data.unwrap_or(&[]))
        }
    };
    collect(response.map_err(|e| describe(&e))?)
}

/// Stream a GET to `writer` with bounded memory; returns the status. The
/// body is copied only for a 2xx answer.
pub fn download(
    agent: &Agent,
    url: &str,
    headers: &[(&str, &str)],
    writer: &mut dyn std::io::Write,
) -> Result<u16, String> {
    let mut req = agent.get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut response = req.call().map_err(|e| describe(&e))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Ok(status);
    }
    let mut reader = response.body_mut().as_reader();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).map_err(|e| e.to_string())?;
    }
    Ok(status)
}
