use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, IconName};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{ui, workspace::Yuki, OpenWorkspaces};

const ROW_H: f32 = 24.;
const INDENT: f32 = 14.;

pub fn render(yuki: &Yuki, cx: &mut Context<Yuki>) -> impl IntoElement {
    let theme = cx.theme();
    let active_id = yuki
        .tabs
        .get(yuki.active)
        .map(|t| t.read(cx).saved.id.clone());
    let count = yuki.rows.iter().filter(|r| r.method.is_some()).count();

    let rows = yuki.rows.iter().enumerate().map(|(ix, row)| {
        let selected = active_id.as_deref() == Some(row.id.as_ref());
        // Indent guides: one hairline per ancestor level.
        let guides = (0..row.depth).map(|_| {
            div()
                .w(px(INDENT))
                .h_full()
                .ml(px(5.))
                .border_l_1()
                .border_color(theme.border)
        });
        let lead: AnyElement = match &row.method {
            Some(m) => div()
                .w(px(36.))
                .flex_none()
                .text_xs()
                .font_family(theme.mono_font_family.clone())
                .text_color(ui::method_color(m, cx))
                .child(ui::method_short(m).to_string())
                .into_any_element(),
            None => div()
                .w(px(16.))
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(if row.open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .into_any_element(),
        };
        let id = row.id.clone();
        let is_folder = row.method.is_none();
        h_flex()
            .id(ix)
            .h(px(ROW_H))
            .pl_2()
            .pr_2()
            .text_sm()
            .cursor_pointer()
            .text_color(if is_folder {
                theme.muted_foreground
            } else {
                theme.foreground
            })
            .hover(|s| s.bg(theme.list_hover))
            // Always reserve the bar so selecting a row never shifts its text.
            .border_l_2()
            .border_color(if selected {
                theme.ring
            } else {
                transparent_black()
            })
            .when(selected, |s| s.bg(theme.list_active))
            .children(guides.collect::<Vec<_>>())
            .child(lead)
            .child(div().flex_1().truncate().child(row.name.clone()))
            .on_click(cx.listener(move |this, _, window, cx| {
                if is_folder {
                    let key = id.to_string();
                    if !this.collapsed.remove(&key) {
                        this.collapsed.insert(key);
                    }
                    this.refresh_rows();
                    cx.notify();
                } else {
                    this.open_request(&id, window, cx);
                }
            }))
    });

    v_flex()
        .size_full()
        .bg(theme.sidebar)
        .child(
            // Workspace switcher: the obvious place to look for "which project am I in".
            h_flex()
                .id("ws-switcher")
                .h(px(32.))
                .mx_1p5()
                .mt_1()
                .px_1p5()
                .gap_1p5()
                .rounded(theme.radius)
                .cursor_pointer()
                .hover(|s| s.bg(theme.list_hover))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(
                            yuki.ws
                                .as_ref()
                                .map_or("workspace".into(), |w| w.name.clone()),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(count.to_string()),
                )
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(IconName::ChevronsUpDown),
                )
                .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenWorkspaces), cx)),
        )
        .child(
            div()
                .id("tree")
                .flex_1()
                .overflow_y_scroll()
                .pb_2()
                .children(rows.collect::<Vec<_>>()),
        )
        .when(yuki.workspaces.is_empty(), |s| {
            s.child(
                div()
                    .p_3()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("nenhum workspace — use :comandos → Importar"),
            )
        })
}
