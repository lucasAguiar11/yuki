//! Self-update from GitHub releases. A build running from `Yuki.app` asks for
//! the latest release on launch and every few hours; settings → about checks by hand. A newer
//! release shows as a status-line chip; installing swaps the bundle in place
//! and relaunches.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use gpui_kit::*;

use crate::{settings::Section, workspace::Yuki, CheckUpdates, InstallUpdate, OpenAbout};

const LATEST: &str = "https://api.github.com/repos/lucasAguiar11/yuki/releases/latest";
/// Re-check while the app stays open; GitHub allows 60 unauthenticated calls/h.
const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);
/// What `.github/workflows/release.yml` uploads for this arch: `Yuki.app`,
/// zipped by `ditto`.
fn asset() -> String {
    format!("Yuki-macos-{}.zip", std::env::consts::ARCH)
}

/// What the status chip and settings → about show.
#[derive(Default)]
pub struct Updater {
    /// Newer than this build, from the last check.
    pub release: Option<Release>,
    pub state: State,
}

impl Global for Updater {}

pub struct Release {
    pub version: String,
    url: String,
}

#[derive(Default, PartialEq)]
pub enum State {
    #[default]
    Idle,
    Checking,
    Checked,
    Installing,
    Failed(SharedString),
}

/// Both views read the global; refresh so they repaint.
fn set(cx: &mut App, f: impl FnOnce(&mut Updater)) {
    f(cx.default_global::<Updater>());
    cx.refresh_windows();
}

/// `…/Yuki.app` when running from a bundle; `None` under `cargo run`.
fn bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension()? == "app").then(|| app.to_path_buf())
}

fn newer(tag: &str, current: &str) -> bool {
    let v = |s: &str| semver::Version::parse(s.trim_start_matches('v')).ok();
    matches!((v(tag), v(current)), (Some(a), Some(b)) if a > b)
}

fn get(url: &str) -> reqwest::RequestBuilder {
    // GitHub's API rejects requests without a User-Agent.
    reqwest::Client::new()
        .get(url)
        .header("User-Agent", concat!("Yuki/", env!("CARGO_PKG_VERSION")))
}

/// `/releases/latest` skips drafts and pre-releases: `release.yml` publishes
/// only once both arches are uploaded.
async fn latest() -> Result<Option<Release>, String> {
    let res = get(LATEST).send().await.map_err(|e| e.to_string())?;
    // 404 = nothing published yet.
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let rel: serde_json::Value = res
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let tag = rel["tag_name"].as_str().unwrap_or_default();
    if !newer(tag, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let asset = asset();
    let url = rel["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"] == asset)
        .and_then(|a| a["browser_download_url"].as_str());
    Ok(url.map(|url| Release {
        version: tag.to_owned(),
        url: url.to_owned(),
    }))
}

async fn download(url: String) -> Result<Vec<u8>, String> {
    let res = get(&url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    Ok(res.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
}

/// Unzip next to the bundle (same volume, so the swap is two renames), check
/// the new app is whole, then swap. The running binary stays mapped after
/// its file is gone.
fn install(zip: &[u8], app: &Path) -> Result<(), String> {
    let stage = app
        .parent()
        .ok_or("bundle has no parent")?
        .join(".yuki-update");
    let _ = fs::remove_dir_all(&stage);
    fs::create_dir(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
    let swap = || -> Result<(), String> {
        let zip_path = stage.join("Yuki.zip");
        fs::write(&zip_path, zip).map_err(|e| e.to_string())?;
        run(Command::new("ditto").arg("-xk").arg(&zip_path).arg(&stage))?;
        let new = stage.join("Yuki.app");
        run(Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&new))?;
        let old = stage.join("old.app");
        fs::rename(app, &old).map_err(|e| e.to_string())?;
        fs::rename(&new, app).map_err(|e| {
            let _ = fs::rename(&old, app);
            e.to_string()
        })
    };
    let result = swap();
    let _ = fs::remove_dir_all(&stage);
    result
}

/// Open the new bundle once this process is gone (two instances would race
/// for `mcp.sock`).
fn relaunch(app: &Path) -> Result<(), String> {
    Command::new("sh")
        .arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; open "$2""#)
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(app)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

/// The root view's handlers for the update actions.
pub(crate) fn on_actions(el: Div, cx: &mut Context<Yuki>) -> Div {
    el.on_action(
        cx.listener(|this, _: &OpenAbout, window, cx| {
            this.open_settings(Section::About, window, cx)
        }),
    )
    .on_action(cx.listener(|this, _: &CheckUpdates, window, cx| {
        this.open_settings(Section::About, window, cx);
        this.check_update(cx);
    }))
    .on_action(cx.listener(|this, _: &InstallUpdate, _, cx| this.install_update(cx)))
}

impl Yuki {
    /// Launch and every `RECHECK`: only from a bundle, where an update can land.
    pub(crate) fn watch_updates(&self, cx: &mut Context<Self>) {
        if bundle().is_none() {
            return;
        }
        cx.spawn(async move |this, cx| {
            while this.update(cx, |this, cx| this.check_update(cx)).is_ok() {
                cx.background_executor().timer(RECHECK).await;
            }
        })
        .detach();
    }

    pub(crate) fn check_update(&self, cx: &mut Context<Self>) {
        if matches!(
            cx.global::<Updater>().state,
            State::Checking | State::Installing
        ) {
            return;
        }
        set(cx, |u| u.state = State::Checking);
        let task = self.backend.rt.spawn(latest());
        cx.spawn(async move |_, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            cx.update(|cx| {
                set(cx, |u| match result {
                    Ok(release) => {
                        u.release = release;
                        u.state = State::Checked;
                    }
                    Err(e) => u.state = State::Failed(e.into()),
                })
            });
        })
        .detach();
    }

    pub(crate) fn install_update(&self, cx: &mut Context<Self>) {
        let u = cx.global::<Updater>();
        if u.state == State::Installing {
            return;
        }
        let Some(url) = u.release.as_ref().map(|r| r.url.clone()) else {
            return;
        };
        let Some(app) = bundle() else {
            let e = t!("updates install only into Yuki.app");
            set(cx, |u| u.state = State::Failed(e.into_owned().into()));
            return;
        };
        set(cx, |u| u.state = State::Installing);
        let dl = self.backend.rt.spawn(download(url));
        cx.spawn(async move |this, cx| {
            let result = match dl.await.map_err(|e| e.to_string()).and_then(|r| r) {
                Ok(zip) => {
                    let app = app.clone();
                    cx.background_spawn(async move { install(&zip, &app) })
                        .await
                }
                Err(e) => Err(e),
            }
            .and_then(|()| relaunch(&app));
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.flush_blocking(cx);
                    cx.quit();
                }
                Err(e) => {
                    let e = t!("update failed: %{e}", e = e).into_owned();
                    set(cx, |u| u.state = State::Failed(e.into()));
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn only_strictly_newer_versions_update() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("v0.10.0", "0.9.1"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0", "0.2.0"));
        assert!(!newer("v0.2.0-beta.1", "0.2.0"));
        assert!(!newer("nightly", "0.1.0"));
    }
}
