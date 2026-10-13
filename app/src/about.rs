//! Settings → about: version, updates, where the data lives, license, bugs.

use gpui_kit::component::{
    button::Button, h_flex, v_flex, ActiveTheme as _, Disableable as _, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    backend,
    settings::SettingsScreen,
    settings_view::field,
    update::{State, Updater},
    CheckUpdates, InstallUpdate, ReportBug,
};

const REPO: &str = "https://github.com/lucasAguiar11/yuki";
const TERMS: &str = "https://github.com/lucasAguiar11/yuki/blob/main/docs/legal/TERMS.md";

/// A new issue on the bug form (`.github/ISSUE_TEMPLATE/bug.yml`), with the
/// version and macOS fields already filled in.
pub fn bug_url() -> String {
    let macos = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    format!(
        "{REPO}/issues/new?template=bug.yml&version={}&macos={macos}",
        env!("CARGO_PKG_VERSION")
    )
}

impl SettingsScreen {
    pub(crate) fn render_about(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, red, blue) = (
            theme.foreground,
            theme.muted_foreground,
            theme.red,
            theme.blue,
        );
        let updater = cx.global::<Updater>();
        let busy = matches!(updater.state, State::Checking | State::Installing);
        let note = match (&updater.state, &updater.release) {
            (State::Checking, _) => Some((t!("checking…").into_owned(), muted)),
            (State::Installing, _) => Some((t!("updating…").into_owned(), muted)),
            (State::Failed(e), _) => Some((e.to_string(), red)),
            (_, Some(r)) => Some((t!("%{v} available", v = r.version).into_owned(), blue)),
            (State::Checked, None) => {
                Some((t!("you're on the latest version").into_owned(), muted))
            }
            (State::Idle, None) => None,
        };
        let can_install = updater.release.is_some() && !busy;
        let link = |id: &'static str, label: String, on_click: fn(&mut Window, &mut App)| {
            div()
                .id(id)
                .cursor_pointer()
                .text_color(blue)
                .hover(move |s| s.text_color(fg))
                .child(label)
                .on_click(move |_, window, cx| on_click(window, cx))
        };
        let updates = h_flex()
            .gap_2()
            .child(
                Button::new("check-updates")
                    .xsmall()
                    .outline()
                    .label(t!("check for updates").into_owned())
                    .disabled(busy)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(CheckUpdates), cx)),
            )
            .when(can_install, |s| {
                s.child(
                    Button::new("install-update")
                        .xsmall()
                        .outline()
                        .label(t!("install and restart").into_owned())
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(InstallUpdate), cx)
                        }),
                )
            })
            .children(note.map(|(text, color)| div().text_xs().text_color(color).child(text)));
        let license = h_flex()
            .gap_3()
            .child("MIT")
            .child(link(
                "about-source",
                t!("source code").into_owned(),
                |_, cx| cx.open_url(REPO),
            ))
            .child(link(
                "about-terms",
                t!("terms of use").into_owned(),
                |_, cx| cx.open_url(TERMS),
            ));
        let bug = link(
            "about-bug",
            t!("report a bug").into_owned(),
            |window, cx| window.dispatch_action(Box::new(ReportBug), cx),
        );
        v_flex()
            .gap_4()
            .child(field(
                t!("version").into_owned(),
                format!(
                    "Yuki {} · macOS {}",
                    env!("CARGO_PKG_VERSION"),
                    std::env::consts::ARCH
                ),
                None,
                cx,
            ))
            .child(field(t!("updates").into_owned(), updates, None, cx))
            .child(field(
                t!("data").into_owned(),
                backend::data_dir().display().to_string(),
                None,
                cx,
            ))
            .child(field(
                t!("license").into_owned(),
                license,
                Some(t!("the code is free; commercial use of the official binaries follows the terms.").into_owned()),
                cx,
            ))
            .child(field(
                t!("problems").into_owned(),
                bug,
                Some(t!("opens a GitHub issue with the version and macOS filled in.").into_owned()),
                cx,
            ))
            .into_any_element()
    }
}
