//! Settings tab: how the request is sent (timeout, redirects; saved with it)
//! and a quick load run (`total` sends, `parallel` at a time; not saved).

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use gpui_kit::component::{
    button::Button,
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputState},
    v_flex, ActiveTheme as _, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::{HttpResponse, RequestSettings};
use yuki_mcp::api::{LoadReport, LOAD_MAX_PARALLEL, LOAD_MAX_TOTAL};

use crate::{editor::RequestEditor, settings_view::field, ui};

pub struct ReqSettings {
    timeout: Entity<InputState>,
    follow: bool,
    max: Entity<InputState>,
    total: Entity<InputState>,
    parallel: Entity<InputState>,
    run: Option<Run>,
    report: Option<Result<LoadReport, String>>,
}

struct Run {
    total: usize,
    done: Arc<AtomicUsize>,
    started: Instant,
    stop: tokio::task::AbortHandle,
}

// Closing the tab (or a finished run) must not leave sends going.
impl Drop for Run {
    fn drop(&mut self) {
        self.stop.abort();
    }
}

fn number(state: &Entity<InputState>, value: Option<u32>, window: &mut Window, cx: &mut App) {
    let text = value.map(|v| v.to_string()).unwrap_or_default();
    state.update(cx, |s, cx| s.set_value(text, window, cx));
}

/// Empty → `None`; `Err` = not a number (shown in red, not saved).
fn parse(state: &Entity<InputState>, cx: &App) -> Result<Option<u32>, ()> {
    let text = state.read(cx).value();
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse().map(Some).map_err(|_| ())
}

impl ReqSettings {
    pub fn new(
        s: &RequestSettings,
        window: &mut Window,
        cx: &mut Context<RequestEditor>,
    ) -> (Self, Vec<Subscription>) {
        let input = |placeholder: &str, window: &mut Window, cx: &mut Context<RequestEditor>| {
            let placeholder = placeholder.to_string();
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let timeout = input("∞", window, cx);
        let max = input(&RequestSettings::MAX_REDIRECTS.to_string(), window, cx);
        let total = input("100", window, cx);
        let parallel = input("10", window, cx);
        number(&timeout, s.timeout_ms, window, cx);
        number(&max, s.max_redirects, window, cx);
        let subs = vec![
            cx.subscribe_in(&timeout, window, RequestEditor::on_input),
            cx.subscribe_in(&max, window, RequestEditor::on_input),
        ];
        let this = Self {
            timeout,
            follow: s.follow_redirects,
            max,
            total,
            parallel,
            run: None,
            report: None,
        };
        (this, subs)
    }

    /// As edited; a field that isn't a number keeps its stored value.
    pub fn draft(&self, saved: &RequestSettings, cx: &App) -> RequestSettings {
        RequestSettings {
            timeout_ms: parse(&self.timeout, cx).unwrap_or(saved.timeout_ms),
            follow_redirects: self.follow,
            max_redirects: parse(&self.max, cx).unwrap_or(saved.max_redirects),
        }
    }

    pub fn running(&self) -> bool {
        self.run.is_some()
    }
}

/// `settings —`, or how many differ from the defaults.
pub fn label(s: &RequestSettings) -> String {
    let n = usize::from(s.timeout_ms.is_some())
        + usize::from(!s.follow_redirects)
        + usize::from(s.max_redirects.is_some());
    match n {
        0 => t!("settings —").into_owned(),
        n => t!("settings %{n}", n = n).into_owned(),
    }
}

fn ms(v: f64) -> String {
    if v < 1000.0 {
        format!("{v:.0} ms")
    } else {
        format!("{:.2} s", v / 1000.0)
    }
}

impl RequestEditor {
    pub(crate) fn set_follow(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.follow = on;
        self.refresh_dirty(cx);
    }

    /// Start a load run (saving first, like a send), or stop the one running.
    pub(crate) fn toggle_load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(run) = self.settings.run.take() {
            run.stop.abort();
            self.settings.report = Some(Err(t!("stopped").into_owned()));
            cx.notify();
            return;
        }
        let count = |s: &Entity<InputState>, default: u32, cx: &App| {
            parse(s, cx).ok().flatten().unwrap_or(default) as usize
        };
        let total = count(&self.settings.total, 100, cx).clamp(1, LOAD_MAX_TOTAL);
        let parallel = count(&self.settings.parallel, 10, cx).clamp(1, LOAD_MAX_PARALLEL);
        let req = self.draft(cx);
        let done = Arc::new(AtomicUsize::new(0));
        let job = self.backend.load_run(
            req.clone(),
            self.env_id.clone(),
            total,
            parallel,
            done.clone(),
        );
        self.settings.run = Some(Run {
            total,
            done,
            started: Instant::now(),
            stop: job.abort_handle(),
        });
        self.settings.report = None;
        cx.notify();
        // Progress: re-render until the run settles.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let running = this.update(cx, |e, cx| {
                cx.notify();
                e.settings.running()
            });
            if !matches!(running, Ok(true)) {
                break;
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = job.await else {
                return; // aborted: `toggle_load` already reported it
            };
            _ = this.update_in(cx, |this, window, cx| {
                this.settings.run = None;
                let sample: Option<Result<HttpResponse, String>> = match &result {
                    Ok(r) => r.sample.clone().map(Ok),
                    Err(e) => Some(Err(e.clone())),
                };
                this.settings.report = Some(result);
                match sample {
                    Some(sample) => {
                        this.land(req, sample, window, cx);
                        // Every send of the run is in the answer's `log` tab.
                        this.resp_tab = crate::editor::RespTab::Log;
                    }
                    None => cx.notify(),
                }
            });
        })
        .detach();
    }

    pub(crate) fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let follow = Checkbox::new("follow-redirects")
            .checked(self.settings.follow)
            .label(t!("follow").into_owned())
            .on_click(cx.listener(|this, on: &bool, _, cx| this.set_follow(*on, cx)));
        let run_label = if self.settings.running() {
            t!("■ stop")
        } else {
            t!("▸ run")
        };
        let run = Button::new("load-run")
            .xsmall()
            .outline()
            .label(run_label.into_owned())
            .on_click(cx.listener(|this, _, window, cx| this.toggle_load(window, cx)));
        let s = &self.settings;
        let red = cx.theme().red;
        let num = |state: &Entity<InputState>, w: f32, cx: &App| {
            div()
                .w(px(w))
                .when(parse(state, cx).is_err(), |d| d.text_color(red))
                .child(Input::new(state).xsmall())
        };
        let faint = ui::faint(cx);
        let unit = |t: String| div().text_xs().text_color(faint).child(t);
        let timeout = h_flex()
            .gap_2()
            .child(num(&s.timeout, 110., cx))
            .child(unit("ms".into()));
        let redirects = h_flex()
            .gap_3()
            .child(follow)
            .when(self.settings.follow, |d| {
                d.child(unit(t!("max").into_owned()))
                    .child(num(&s.max, 64., cx))
            });
        let load = h_flex()
            .gap_2()
            .child(num(&s.total, 80., cx))
            .child(unit(t!("sends ×").into_owned()))
            .child(num(&s.parallel, 64., cx))
            .child(unit(t!("in parallel").into_owned()))
            .child(run);
        v_flex()
            .id("req-settings")
            // The pane is as tall as the editor split: past it, scroll — never spill onto the response.
            .size_full()
            .overflow_y_scroll()
            .gap_3()
            .pt_1()
            .text_sm()
            .child(field(
                t!("timeout").into_owned(),
                timeout,
                Some(t!("whole send, connect to last byte · empty = no limit").into_owned()),
                cx,
            ))
            .child(field(
                t!("redirects").into_owned(),
                redirects,
                Some(
                    t!(
                        "off: the 3xx is the response · max %{n} by default",
                        n = RequestSettings::MAX_REDIRECTS
                    )
                    .into_owned(),
                ),
                cx,
            ))
            .child(field(
                t!("load").into_owned(),
                v_flex().gap_2().child(load).child(self.render_load(cx)),
                Some(
                    t!("resolved once, same values every send · every send in the log tab")
                        .into_owned(),
                ),
                cx,
            ))
            .into_any_element()
    }

    fn render_load(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, red) = (theme.foreground, theme.muted_foreground, theme.red);
        let s = &self.settings;
        if let Some(run) = &s.run {
            let done = run.done.load(Ordering::Relaxed);
            let secs = run.started.elapsed().as_secs_f64();
            return div()
                .text_color(muted)
                .child(t!(
                    "running %{done}/%{total} · %{secs} s",
                    done = done,
                    total = run.total,
                    secs = format!("{secs:.1}")
                ))
                .into_any_element();
        }
        let Some(report) = &s.report else {
            return div().into_any_element();
        };
        let r = match report {
            Ok(r) => r,
            Err(e) => return div().text_color(red).child(e.clone()).into_any_element(),
        };
        let statuses = r.statuses.iter().map(|(code, n)| {
            div()
                .text_color(ui::status_color(*code, cx))
                .child(format!("{code} ×{n}"))
        });
        let errors = r
            .errors
            .iter()
            .map(|(e, n)| div().text_color(red).child(format!("✕ {n}× {e}")));
        let pct = |p: f64| r.percentile_ms(p).map_or("—".into(), ms);
        let times = (!r.times_ms.is_empty()).then(|| {
            format!(
                "min {} · avg {} · p50 {} · p95 {} · max {}",
                pct(0.0),
                r.mean_ms().map_or("—".into(), ms),
                pct(50.0),
                pct(95.0),
                pct(100.0)
            )
        });
        let rate = r
            .per_second()
            .map_or(String::new(), |v| format!(" · {v:.1} req/s"));
        v_flex()
            .gap_1()
            .child(h_flex().gap_3().flex_wrap().children(statuses))
            .children(errors)
            .children(times.map(|t| div().text_color(fg).child(t)))
            .child(div().text_color(muted).child(format!(
                "{} · {}{rate}",
                t!("%{n} sent", n = r.sent()),
                ms(r.wall_ms)
            )))
            .into_any_element()
    }
}
