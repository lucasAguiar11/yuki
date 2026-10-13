//! Self-update from GitHub releases. A build running from `Yuki.app` asks for
//! the latest release on launch; the status line offers it; one click swaps
//! the bundle in place and relaunches.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use gpui_kit::*;

use crate::workspace::Yuki;

const LATEST: &str = "https://api.github.com/repos/lucasAguiar11/yuki/releases/latest";
/// What `.github/workflows/release.yml` uploads: `Yuki.app`, zipped by `ditto`.
const ASSET: &str = "Yuki-macos-universal.zip";

pub struct Update {
    pub version: String,
    url: String,
    pub installing: bool,
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

/// `/releases/latest` skips drafts and pre-releases: publishing the draft
/// `release.yml` makes is what ships an update.
async fn latest() -> Result<Option<Update>, String> {
    let rel: serde_json::Value = get(LATEST)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let tag = rel["tag_name"].as_str().unwrap_or_default();
    if !newer(tag, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let url = rel["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"] == ASSET)
        .and_then(|a| a["browser_download_url"].as_str());
    Ok(url.map(|url| Update {
        version: tag.to_owned(),
        url: url.to_owned(),
        installing: false,
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
        let zip_path = stage.join(ASSET);
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

impl Yuki {
    pub(crate) fn check_update(&self, cx: &mut Context<Self>) {
        if bundle().is_none() {
            return;
        }
        let task = self.backend.rt.spawn(latest());
        cx.spawn(async move |this, cx| match task.await {
            Ok(Ok(Some(update))) => {
                let _ = this.update(cx, |this, cx| {
                    this.release = Some(update);
                    cx.notify();
                });
            }
            Ok(Err(e)) => eprintln!("[update] {e}"),
            _ => {}
        })
        .detach();
    }

    pub(crate) fn install_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(update), Some(app)) = (self.release.as_mut(), bundle()) else {
            return;
        };
        if update.installing {
            return;
        }
        update.installing = true;
        cx.notify();
        let dl = self.backend.rt.spawn(download(update.url.clone()));
        cx.spawn_in(window, async move |this, cx| {
            let result = match dl.await.map_err(|e| e.to_string()).and_then(|r| r) {
                Ok(zip) => {
                    let app = app.clone();
                    cx.background_spawn(async move { install(&zip, &app) })
                        .await
                }
                Err(e) => Err(e),
            }
            .and_then(|()| relaunch(&app));
            this.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.flush_blocking(cx);
                    cx.quit();
                }
                Err(e) => {
                    if let Some(u) = &mut this.release {
                        u.installing = false;
                    }
                    this.status = t!("update failed: %{e}", e = e).into_owned().into();
                    cx.notify();
                }
            })
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
