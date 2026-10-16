//! Response vs the previous one in history, git-style: a line that's new is
//! tinted green, a line that replaced others yellow. Removed lines have nothing
//! left to tint; the strip's `+3 −1` counts them.

use gpui_kit::component::{
    input::{RangeDecoration, RangeDecorationStyle},
    ActiveTheme as _,
};
use gpui_kit::*;

use crate::{editor::RequestEditor, response};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Added,
    Modified,
}

/// Lines of `new` (0-based) that differ from `old`, by LCS over whole lines.
pub fn changed_lines(old: &str, new: &str) -> Vec<(usize, Change)> {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..]
        .iter()
        .rev()
        .zip(b[pre..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a, b) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    // One hunk: its inserted lines, yellow if it also removed some.
    fn flush(out: &mut Vec<(usize, Change)>, ins: &mut Vec<usize>, deleted: bool) {
        let kind = if deleted {
            Change::Modified
        } else {
            Change::Added
        };
        out.extend(ins.drain(..).map(|k| (k, kind)));
    }
    let mut out = vec![];
    // ponytail: O(n·m) table; past ~4M cells the whole middle is one change.
    if a.len() * b.len() > 4_000_000 {
        let mut ins: Vec<usize> = (pre..pre + b.len()).collect();
        flush(&mut out, &mut ins, !a.is_empty());
        return out;
    }
    let w = b.len() + 1;
    let mut t = vec![0u32; (a.len() + 1) * w];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            t[i * w + j] = if a[i] == b[j] {
                t[(i + 1) * w + j + 1] + 1
            } else {
                t[(i + 1) * w + j].max(t[i * w + j + 1])
            };
        }
    }
    let (mut ins, mut deleted) = (vec![], false);
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            flush(&mut out, &mut ins, deleted);
            deleted = false;
            (i, j) = (i + 1, j + 1);
        } else if j < b.len() && (i == a.len() || t[i * w + j + 1] >= t[(i + 1) * w + j]) {
            ins.push(pre + j);
            j += 1;
        } else {
            deleted = true;
            i += 1;
        }
    }
    flush(&mut out, &mut ins, deleted);
    out
}

impl RequestEditor {
    /// Tint the shown response's lines that changed since the response before
    /// it and count them for the strip (`+3 −1`). Off the UI thread; dropped if
    /// another response is shown meanwhile.
    pub(crate) fn mark_changes(&mut self, text: String, resp_id: String, cx: &mut Context<Self>) {
        self.diff_marks.clear(cx);
        self.diff_stat = None;
        if resp_id.is_empty() || cx.global::<crate::prefs::Prefs>().hide_line_changes {
            return;
        }
        let backend = self.backend.clone();
        let (ws, id) = (self.saved.workspace_id.clone(), self.saved.id.clone());
        let shown = resp_id.clone();
        let task = cx.background_spawn(async move {
            let history = backend.history(&ws, &id);
            let at = history.iter().position(|s| s.id == resp_id)?;
            let (prev, _) = backend
                .stored_response(&ws, &id, &history.get(at + 1)?.id)
                .ok()?;
            let (old, _) = response::body_for_editor(&prev);
            let changes = changed_lines(&old, &text);
            // git's count: a changed line is one in and one out.
            let plus = changes.len();
            let minus = (old.lines().count() + plus).saturating_sub(text.lines().count());
            Some((changes, (plus, minus), text))
        });
        cx.spawn(async move |this, cx| {
            let Some((changes, stat, text)) = task.await else {
                return;
            };
            _ = this.update(cx, |this, cx| {
                let current = this.response.as_ref().and_then(|r| r.as_ref().ok());
                if current.is_none_or(|r| r.response_id != shown) {
                    return;
                }
                let marks = tints(&text, changes, cx);
                this.diff_marks.set(marks, cx);
                this.diff_stat = (stat != (0, 0)).then_some(stat);
                cx.notify();
            });
        })
        .detach();
    }
}

/// The fills for `changes` in `text`. The editor has no gutter hook (a
/// one-cell bar reads too thick), so the whole line takes a faint tint.
pub(crate) fn tints(text: &str, changes: Vec<(usize, Change)>, cx: &App) -> Vec<RangeDecoration> {
    let (green, yellow) = (cx.theme().green, cx.theme().yellow);
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(ix, _)| ix + 1))
        .collect();
    changes
        .into_iter()
        .filter_map(|(ix, change)| {
            let start = *starts.get(ix)?;
            let end = starts.get(ix + 1).copied().unwrap_or(text.len());
            let color = match change {
                Change::Added => green,
                Change::Modified => yellow,
            };
            Some(
                RangeDecoration::new(start..end)
                    .with_style(RangeDecorationStyle::Fill)
                    .with_color(color.opacity(0.11)),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{changed_lines, Change::*};

    #[test]
    fn marks_added_and_modified_lines_of_the_new_text() {
        let old = "a\nb\nc\nd\ne";
        let new = "a\nB\nc\nx\nd\ne\nf";
        // b→B replaced; x inserted; f appended.
        assert_eq!(
            changed_lines(old, new),
            vec![(1, Modified), (3, Added), (6, Added)]
        );
        assert!(changed_lines(old, old).is_empty());
        // A deletion alone tints nothing (the strip counts it).
        assert!(changed_lines(old, "a\nc\nd\ne").is_empty());
    }
}
