//! Sidebar focus: one folder as the tree's root, so only what's inside it
//! shows. Kept per workspace in the session; ⌘P still finds every request.

use gpui_kit::component::{h_flex, tooltip::Tooltip, ActiveTheme as _};
use gpui_kit::*;

use crate::{tree, workspace::Yuki};

impl Yuki {
    pub(crate) fn set_focus_folder(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.focus_folder = id;
        self.refresh_rows();
        cx.notify();
    }

    /// Where something asked for `parent` lands: there if it's inside the
    /// focused folder (or nothing is focused), else in the focused folder —
    /// never somewhere the sidebar doesn't show.
    pub(crate) fn within_focus(&self, parent: Option<String>) -> Option<String> {
        let (Some(focus), Some(d)) = (&self.focus_folder, &self.data) else {
            return parent;
        };
        if tree::within(&d.folders, parent.as_deref(), focus) {
            parent
        } else {
            Some(focus.clone())
        }
    }

    /// `SEFAZ-PR / DC-e  ✕` above the tree while a folder is focused.
    pub(crate) fn render_focus_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (focus, d) = (self.focus_folder.as_deref()?, self.data.as_ref()?);
        let path = tree::folder_path(&d.folders, Some(focus)).join(" / ");
        let theme = cx.theme();
        let (fg, muted, hover, border) = (
            theme.foreground,
            theme.muted_foreground,
            theme.list_hover,
            theme.border,
        );
        Some(
            h_flex()
                .h(px(26.))
                .mx_1p5()
                .px_1p5()
                .gap_1p5()
                .border_b_1()
                .border_color(border)
                .text_xs()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(fg)
                        .child(path),
                )
                .child(
                    div()
                        .id("leave-focus")
                        .px_1()
                        .rounded(theme.radius)
                        .cursor_pointer()
                        .text_color(muted)
                        .hover(move |s| s.text_color(fg).bg(hover))
                        .child("✕")
                        .tooltip(|window, cx| {
                            Tooltip::new(t!("leave focus").into_owned()).build(window, cx)
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.set_focus_folder(None, cx))),
                ),
        )
    }
}
