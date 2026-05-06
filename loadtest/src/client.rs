use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::{StatusCode, redirect::Policy};
use serde::Serialize;
use url::Url;

use crate::metrics::{Metric, Reporter};

pub struct Client {
    http: reqwest::Client,
    base: Url,
    reporter: Reporter,
    csrf: Option<String>,
}

impl Client {
    pub fn new(base: Url, reporter: Reporter, timeout: Duration) -> Result<Self> {
        let http = reqwest::Client::builder()
            .cookie_store(true)
            .redirect(Policy::none())
            .timeout(timeout)
            .build()?;
        Ok(Self {
            http,
            base,
            reporter,
            csrf: None,
        })
    }

    pub fn csrf(&self) -> &str {
        self.csrf.as_deref().unwrap_or("")
    }

    /// GET a path. The CSRF token is stable for the lifetime of a session, so
    /// we only sniff it from the first HTML page we render — subsequent GETs
    /// are pure timing measurements.
    pub async fn get(&mut self, label: &'static str, path: &str) -> Result<GetOutcome> {
        let url = self.base.join(path).with_context(|| format!("join {path}"))?;
        let started = Instant::now();
        let response = self.http.get(url).send().await?;
        let status = response.status();
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let outcome = if status.is_redirection() {
            GetOutcome::Redirect(location.clone().unwrap_or_default())
        } else if status.is_success() {
            let body = response.text().await?;
            if self.csrf.is_none() {
                self.csrf = extract_csrf_token(&body);
            }
            GetOutcome::Page(body)
        } else {
            let body = response.text().await.unwrap_or_default();
            bail!("GET {path} unexpected status {status}: {}", truncate(&body));
        };

        self.reporter.record(Metric {
            label,
            method: "GET",
            status: status.as_u16(),
            duration: started.elapsed(),
        });
        Ok(outcome)
    }

    /// GET a file download (PDF, XML, ZIP). Consumes the body as bytes so the
    /// timing reflects the full transfer, but doesn't parse it.
    pub async fn download(&self, label: &'static str, path: &str) -> Result<()> {
        let url = self.base.join(path).with_context(|| format!("join {path}"))?;
        let started = Instant::now();
        let response = self
            .http
            .get(url)
            .send()
            .await
            .with_context(|| describe_send_error(label, path, started.elapsed()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("{label}: GET {path} body read"))?;
        self.reporter.record(Metric {
            label,
            method: "GET",
            status: status.as_u16(),
            duration: started.elapsed(),
        });
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes);
            let snippet = extract_validation_errors(&body)
                .unwrap_or_else(|| format!("{} bytes", bytes.len()));
            bail!("GET {path} (download) status {status}: {snippet}");
        }
        Ok(())
    }

    /// Follow a 303-style redirect chain, recording each hop, until reaching a
    /// non-redirect response. Returns the final page body (if any).
    pub async fn follow(&mut self, label: &'static str, mut path: String) -> Result<String> {
        for _ in 0..5 {
            match self.get(label, &path).await? {
                GetOutcome::Redirect(next) => path = next,
                GetOutcome::Page(body) => return Ok(body),
            }
        }
        bail!("redirect loop following {label}");
    }

    /// POST a form. The caller is responsible for including `csrf_token` in
    /// `form` (use [`Client::csrf`]).
    pub async fn post<F: Serialize + ?Sized>(
        &mut self,
        label: &'static str,
        path: &str,
        form: &F,
    ) -> Result<PostOutcome> {
        let body = serde_urlencoded::to_string(form).context("encode form")?;
        self.post_raw(label, path, "application/x-www-form-urlencoded", body)
            .await
    }

    /// POST a JSON payload. Used for the few endpoints (e.g. reorder) that take
    /// `application/json` instead of an HTML form. CSRF token, if needed, must
    /// be inside the payload itself.
    pub async fn post_json<P: Serialize + ?Sized>(
        &mut self,
        label: &'static str,
        path: &str,
        payload: &P,
    ) -> Result<PostOutcome> {
        let body = serde_json::to_string(payload).context("encode json")?;
        self.post_raw(label, path, "application/json", body).await
    }

    async fn post_raw(
        &mut self,
        label: &'static str,
        path: &str,
        content_type: &'static str,
        body: String,
    ) -> Result<PostOutcome> {
        let url = self.base.join(path).with_context(|| format!("join {path}"))?;
        let started = Instant::now();
        let response = self
            .http
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body)
            .send()
            .await?;
        let status = response.status();
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        self.reporter.record(Metric {
            label,
            method: "POST",
            status: status.as_u16(),
            duration: started.elapsed(),
        });

        if status == StatusCode::SEE_OTHER || status == StatusCode::FOUND {
            return Ok(PostOutcome::Redirect(location.unwrap_or_default()));
        }
        if status == StatusCode::NO_CONTENT {
            return Ok(PostOutcome::NoContent);
        }
        if status.is_success() {
            // Server re-rendered the form (validation error). Surface the body
            // so the caller can decide whether to fail.
            let body = response.text().await?;
            return Ok(PostOutcome::Rerender(body));
        }
        let body = response.text().await.unwrap_or_default();
        bail!("POST {path} unexpected status {status}: {}", truncate(&body));
    }
}

pub enum GetOutcome {
    Page(String),
    Redirect(String),
}

pub enum PostOutcome {
    Redirect(String),
    Rerender(String),
    NoContent,
}

impl PostOutcome {
    pub fn expect_redirect(self, label: &str) -> Result<String> {
        match self {
            PostOutcome::Redirect(loc) => Ok(loc),
            PostOutcome::NoContent => bail!("{label}: expected 303, got 204"),
            PostOutcome::Rerender(body) => bail!(
                "{label}: validation error: {}",
                extract_validation_errors(&body)
                    .unwrap_or_else(|| format!("(no error spans found): {}", truncate(&body)))
            ),
        }
    }

    pub fn expect_no_content(self, label: &str) -> Result<()> {
        match self {
            PostOutcome::NoContent => Ok(()),
            PostOutcome::Redirect(loc) => bail!("{label}: expected 204, got redirect to {loc}"),
            PostOutcome::Rerender(body) => bail!(
                "{label}: expected 204, got 200: {}",
                truncate(&body)
            ),
        }
    }
}

/// Pull rendered `<span class="error">…</span>` snippets out of a re-rendered
/// form so the loadtest log shows what actually failed. Also picks up the
/// `<p>` body of the generic error page used for 400/500 responses (e.g.
/// "Missing data when generating PDF: …").
fn extract_validation_errors(body: &str) -> Option<String> {
    let mut out = Vec::new();
    let needle = "class=\"error\">";
    for chunk in body.split(needle).skip(1) {
        if let Some((msg, _)) = chunk.split_once("</span>") {
            let msg = msg.trim();
            if !msg.is_empty() {
                out.push(msg.to_string());
            }
        }
        if out.len() >= 5 {
            out.push("…".into());
            break;
        }
    }
    if out.is_empty()
        && let Some(start) = body.find("<p>Error ")
        && let Some(after) = body[start..].find("</p>")
        && let Some(msg_start) = body[start + after..].find("<p>")
        && let Some(msg_end) = body[start + after + msg_start..].find("</p>")
    {
        let msg = &body
            [start + after + msg_start + 3..start + after + msg_start + msg_end];
        let msg = msg.trim();
        if !msg.is_empty() {
            out.push(msg.to_string());
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join(" | "))
    }
}

fn extract_csrf_token(body: &str) -> Option<String> {
    let marker = "name=\"csrf_token\" value=\"";
    body.split_once(marker)
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(token, _)| token.to_string())
}

fn describe_send_error(_label: &str, path: &str, elapsed: Duration) -> String {
    // Caller already prefixes the label, so just describe the path/timing.
    format!("GET {path} send failed after {:.1}s", elapsed.as_secs_f64())
}

fn truncate(s: &str) -> String {
    let mut out: String = s.chars().take(200).collect();
    if s.len() > out.len() {
        out.push('…');
    }
    out
}
