//! Quick load run: one request sent `total` times, `parallel` at a time.
//! Resolved once (templates, auth, cookies, `$response.body` sources), so
//! every send carries the same values; a summary comes back, plus one answer
//! (the first 2xx) stored in history, its `load_log` listing every send.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::task::JoinSet;
use yuki_core::{HttpResponse, LoadLogEntry, YukiError};
use yuki_http::HttpExecutor;

use super::send::{Pick, Prepared};
use crate::api::{redact, ApiBackend};

pub const LOAD_MAX_TOTAL: usize = 10_000;
pub const LOAD_MAX_PARALLEL: usize = 200;

/// Outcome of `ApiBackend::load_http`.
#[derive(Debug, Clone, Default)]
pub struct LoadReport {
    /// Answered sends by status, ascending.
    pub statuses: Vec<(u16, usize)>,
    /// Failed sends by message, most frequent first.
    pub errors: Vec<(String, usize)>,
    /// Total time of each answered send, ascending.
    pub times_ms: Vec<f64>,
    /// The whole run.
    pub wall_ms: f64,
    /// The first 2xx (else first) answer, in history; its `load_log` lists every send.
    pub sample: Option<HttpResponse>,
}

impl LoadReport {
    pub fn sent(&self) -> usize {
        self.times_ms.len() + self.errors.iter().map(|(_, n)| n).sum::<usize>()
    }

    pub fn mean_ms(&self) -> Option<f64> {
        let n = self.times_ms.len();
        (n > 0).then(|| self.times_ms.iter().sum::<f64>() / n as f64)
    }

    /// Nearest-rank percentile (`p` in 0–100) of the answered sends.
    pub fn percentile_ms(&self, p: f64) -> Option<f64> {
        let n = self.times_ms.len();
        if n == 0 {
            return None;
        }
        let rank = ((p / 100.0) * n as f64).ceil() as usize;
        Some(self.times_ms[rank.clamp(1, n) - 1])
    }

    pub fn per_second(&self) -> Option<f64> {
        (self.wall_ms > 0.0).then(|| self.sent() as f64 * 1000.0 / self.wall_ms)
    }
}

// shortcut: log lines past LOG_CAP are counted, not listed; and only the first
// DETAIL_FIRST sends (plus failures up to DETAIL_CAP) keep their request and
// response, so a history entry stays a few MB at most. Raise them if needed.
const LOG_CAP: usize = 2_000;
const DETAIL_FIRST: usize = 100;
const DETAIL_CAP: usize = 500;
/// Response body kept per send.
const BODY_CAP: usize = 4 * 1024;

/// One send: its index, when it finished (ms from the run's start), outcome.
type Sent = (usize, f64, Result<HttpResponse, YukiError>);

#[derive(Default)]
struct Tally {
    statuses: BTreeMap<u16, usize>,
    errors: BTreeMap<String, usize>,
    times_ms: Vec<f64>,
    sample: Option<HttpResponse>,
    log: Vec<LoadLogEntry>,
    detailed: usize,
    unlisted: usize,
}

/// The start of a body; binary ones are base64 on the response, so only their size.
fn clip_body(resp: &HttpResponse) -> String {
    if !resp.body_is_text {
        return format!("(binary, {} B)", resp.body_size);
    }
    match resp.body.char_indices().nth(BODY_CAP) {
        Some((at, _)) => format!("{}\n… ({} B in all)", &resp.body[..at], resp.body_size),
        None => resp.body.clone(),
    }
}

impl Tally {
    fn add(&mut self, r: Result<Sent, tokio::task::JoinError>) {
        let (kind, text, at_ms, events, body) = match r {
            Ok((i, at_ms, Ok(resp))) => {
                let kind = if resp.status < 400 { "done" } else { "error" };
                *self.statuses.entry(resp.status).or_default() += 1;
                self.times_ms.push(resp.timing.total_ms);
                let line = format!(
                    "#{} {} {} · {:.0} ms",
                    i + 1,
                    resp.status,
                    resp.status_text,
                    resp.timing.total_ms
                );
                let (events, body) = (resp.events.clone(), Some(clip_body(&resp)));
                // The first 2xx, else the first answer: the one worth reading.
                let ok = |r: &HttpResponse| (200..300).contains(&r.status);
                if self.sample.as_ref().is_none_or(|s| !ok(s) && ok(&resp)) {
                    self.sample = Some(resp);
                }
                (kind, line, at_ms, events, body)
            }
            Ok((i, at_ms, Err(e))) => {
                let (msg, events) = match e {
                    YukiError::HttpFailed(f) => (f.message, f.events),
                    YukiError::Http(m) => (m, vec![]),
                    e => (e.to_string(), vec![]),
                };
                let msg = redact::redact_error(&msg);
                *self.errors.entry(msg.clone()).or_default() += 1;
                ("error", format!("#{} {msg}", i + 1), at_ms, events, None)
            }
            Err(e) => {
                *self.errors.entry(e.to_string()).or_default() += 1;
                ("error", e.to_string(), 0.0, vec![], None)
            }
        };
        if self.log.len() >= LOG_CAP {
            self.unlisted += 1;
            return;
        }
        let detail =
            self.detailed < DETAIL_FIRST || (kind == "error" && self.detailed < DETAIL_CAP);
        self.detailed += usize::from(detail);
        self.log.push(LoadLogEntry {
            at_ms,
            kind: kind.into(),
            text,
            events: if detail { events } else { vec![] },
            body: body.filter(|_| detail),
        });
    }

    /// The sample carries the run's log (`load_log`): a header, one line per
    /// send in finish order, and the summary, so history keeps the whole run.
    fn report(mut self, wall_ms: f64, total: usize, parallel: usize) -> LoadReport {
        let mut times_ms = std::mem::take(&mut self.times_ms);
        times_ms.sort_by(f64::total_cmp);
        let mut errors: Vec<_> = std::mem::take(&mut self.errors).into_iter().collect();
        errors.sort_by_key(|e| std::cmp::Reverse(e.1));
        let mut report = LoadReport {
            statuses: std::mem::take(&mut self.statuses).into_iter().collect(),
            errors,
            times_ms,
            wall_ms,
            sample: None,
        };
        let info = |at_ms: f64, text: String| LoadLogEntry {
            at_ms,
            kind: "info".into(),
            text,
            events: vec![],
            body: None,
        };
        let mut log = vec![info(0.0, format!("{total} sends, {parallel} in parallel"))];
        log.append(&mut self.log);
        if self.unlisted > 0 {
            log.push(info(
                wall_ms,
                format!("… {} more sends not listed", self.unlisted),
            ));
        }
        log.push(LoadLogEntry {
            kind: "done".into(),
            ..info(wall_ms, summary(&report))
        });
        if let Some(mut sample) = self.sample {
            sample.load_log = log;
            report.sample = Some(sample);
        }
        report
    }
}

/// `100 sent · 200 ×86 · 500 ×14 · p50 12 ms · p95 15 ms · 806 req/s`
fn summary(r: &LoadReport) -> String {
    let mut parts = vec![format!("{} sent", r.sent())];
    parts.extend(r.statuses.iter().map(|(s, n)| format!("{s} ×{n}")));
    let failed: usize = r.errors.iter().map(|(_, n)| n).sum();
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    for p in [50.0, 95.0] {
        if let Some(ms) = r.percentile_ms(p) {
            parts.push(format!("p{p} {ms:.0} ms"));
        }
    }
    if let Some(rate) = r.per_second() {
        parts.push(format!("{rate:.0} req/s"));
    }
    parts.join(" · ")
}

impl ApiBackend {
    /// Send `req_id` `total` times, at most `parallel` in flight (both
    /// clamped to the limits above). `done` counts finished sends, for
    /// progress. Dropping the future aborts every send in flight.
    pub async fn load_http(
        &self,
        ws_id: String,
        req_id: String,
        env_id: Option<String>,
        total: usize,
        parallel: usize,
        done: Arc<AtomicUsize>,
    ) -> Result<LoadReport, String> {
        let total = total.clamp(1, LOAD_MAX_TOTAL);
        let parallel = parallel.clamp(1, LOAD_MAX_PARALLEL.min(total));
        let chain = [req_id.clone()];
        self.send_sources(&ws_id, &req_id, env_id.as_deref(), None, false, &chain)
            .await?;
        let p = Arc::new(
            self.prepare(&ws_id, &req_id, env_id, Pick::default(), false)
                .await?,
        );
        eprintln!(
            "[load] {total} × {} {} ({parallel} in parallel)",
            p.req.method,
            redact::redact_url(&p.req.url)
        );

        let started = Instant::now();
        let mut tally = Tally::default();
        let mut set = JoinSet::new();
        for i in 0..total {
            if set.len() >= parallel {
                if let Some(r) = set.join_next().await {
                    tally.add(r);
                }
            }
            let (exec, p, done) = (self.executor.clone(), p.clone(), done.clone());
            set.spawn(async move {
                let mut req = p.req.clone();
                // The executor cancels an in-flight send with the same id.
                req.id = format!("{}#{i}", req.id);
                let send = exec.send(&req, p.cookies.clone(), p.dns.clone());
                let r = HttpExecutor::with_cert_check_skipped(p.skip_certs, send).await;
                done.fetch_add(1, Ordering::Relaxed);
                (i, started.elapsed().as_secs_f64() * 1000.0, r)
            });
        }
        while let Some(r) = set.join_next().await {
            tally.add(r);
        }
        let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut report = tally.report(wall_ms, total, parallel);
        // shortcut: cookies the run receives aren't stored in the jar; ingest the sample's if a run must log in.
        if let Some(sample) = report.sample.as_mut() {
            sample.request_id = req_id.clone();
            self.store_sample(&ws_id, &req_id, sample, &p).await;
        }
        Ok(report)
    }

    async fn store_sample(&self, ws_id: &str, req_id: &str, resp: &mut HttpResponse, p: &Prepared) {
        let responses = self.responses.clone();
        let (ws, req) = (ws_id.to_string(), req_id.to_string());
        let (r, resolved, edited) = (resp.clone(), p.resolved.clone(), p.edited.clone());
        let stored = tokio::task::spawn_blocking(move || {
            responses.append(&ws, &req, r, resolved, Some(edited), 20)
        })
        .await;
        match stored {
            Ok(Ok(stored)) => {
                resp.response_id = stored.id;
                (self.notify)(
                    "mcp:response:stored",
                    serde_json::json!({ "workspaceId": ws_id, "requestId": req_id }),
                );
            }
            Ok(Err(e)) => eprintln!("[load] failed to store the sample response: {e}"),
            Err(e) => eprintln!("[load] history task panicked: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LoadReport;

    fn report(times: &[f64]) -> LoadReport {
        LoadReport {
            times_ms: times.to_vec(),
            wall_ms: 500.0,
            errors: vec![("boom".into(), 2)],
            ..Default::default()
        }
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let r = report(&[10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0]);
        assert_eq!(r.percentile_ms(50.0), Some(50.0));
        assert_eq!(r.percentile_ms(95.0), Some(100.0));
        assert_eq!(r.percentile_ms(0.0), Some(10.0));
        assert_eq!(r.percentile_ms(100.0), Some(100.0));
        assert_eq!(r.mean_ms(), Some(55.0));
    }

    #[test]
    fn errors_count_as_sent_but_not_timed() {
        let r = report(&[5.0]);
        assert_eq!(r.sent(), 3);
        assert_eq!(r.per_second(), Some(6.0));
        assert_eq!(report(&[]).percentile_ms(95.0), None);
        assert_eq!(report(&[]).mean_ms(), None);
    }
}
