//! `{{ VAR }}` badges inside the code editors (raw body, GraphQL query and
//! variables). Code editors take no atomic tokens, so each template is painted
//! instead: tinted fill + frame, name in the state color, faint braces.

use gpui_kit::component::input::{
    EditorState, RangeDecoration, RangeDecorationCollection, RangeDecorationStyle, TextDecoration,
    TextDecorationCollection,
};
use gpui_kit::*;

use crate::{
    var_view::kind_color,
    vars::{self, Kind, Vars},
};

pub struct CodeVars {
    pub editor: Entity<EditorState>,
    boxes: RangeDecorationCollection,
    text: TextDecorationCollection,
}

impl CodeVars {
    pub fn new(editor: Entity<EditorState>, cx: &mut App) -> Self {
        let (boxes, text) = editor.update(cx, |s, cx| {
            (
                s.create_range_decorations_collection(vec![], cx),
                s.create_decorations_collection(vec![], cx),
            )
        });
        Self {
            editor,
            boxes,
            text,
        }
    }

    /// Repaint every `{{ … }}`, tinted by its state like the chips.
    pub fn paint(&self, vars: &Vars, cx: &mut App) {
        let value = self.editor.read(cx).value();
        let (mut boxes, mut text) = (vec![], vec![]);
        for (range, expr) in vars::spans(&value) {
            let kind = vars::kind(expr, vars);
            let c = kind_color(kind, cx);
            let border = c.opacity(if kind == Kind::Missing { 0.7 } else { 0.25 });
            boxes.push(
                RangeDecoration::new(range.clone())
                    .with_style(RangeDecorationStyle::Fill)
                    .with_color(c.opacity(0.13)),
            );
            boxes.push(RangeDecoration::new(range.clone()).with_color(border));
            let at = expr.as_ptr() as usize - value.as_ptr() as usize;
            let style = |color: Hsla| HighlightStyle {
                color: Some(color),
                ..Default::default()
            };
            text.push(TextDecoration::new(range.start..at, style(c.opacity(0.4))));
            text.push(TextDecoration::new(at..at + expr.len(), style(c)));
            text.push(TextDecoration::new(
                at + expr.len()..range.end,
                style(c.opacity(0.4)),
            ));
        }
        self.boxes.set(boxes, cx);
        self.text.set(text, cx);
    }
}
