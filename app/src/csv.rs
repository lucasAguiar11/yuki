//! CSV response preview: RFC 4180 rows (quoted fields, `""` escapes,
//! newlines inside quotes) rendered as a monospace table.

use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::{prelude::FluentBuilder as _, *};

/// Rows shown; past it the table says how many were left out.
/// ponytail: a cap, not virtualization — swap for `uniform_list` if huge CSVs matter.
const MAX_ROWS: usize = 1000;
/// Column width cap, in characters; longer cells truncate.
const MAX_CHARS: usize = 40;

pub fn parse(text: &str) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut cell) = (vec![], vec![], String::new());
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                cell.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => cell.push(c),
            (false, '"') if cell.is_empty() => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut cell)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

pub fn is_csv(content_type: &str) -> bool {
    matches!(content_type, "text/csv" | "application/csv")
}

pub fn render(text: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let rows = parse(text);
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..cols)
        .map(|c| {
            rows.iter()
                .take(MAX_ROWS)
                .filter_map(|r| r.get(c))
                .map(|s| s.chars().count())
                .max()
                .unwrap_or(0)
                .clamp(3, MAX_CHARS)
        })
        .collect();
    // Mono advance ≈ 0.6em at the table's 12px, plus the cell padding.
    let width = |chars: usize| px(chars as f32 * 7.2 + 16.);
    let hidden = rows.len().saturating_sub(MAX_ROWS);
    let table = v_flex().children(rows.iter().take(MAX_ROWS).enumerate().map(|(i, row)| {
        h_flex()
            .border_b_1()
            .border_color(theme.border)
            .when(i == 0, |r| {
                r.bg(theme.sidebar)
                    .text_color(theme.foreground)
                    .font_weight(FontWeight::SEMIBOLD)
            })
            .children(widths.iter().enumerate().map(|(c, w)| {
                div()
                    .w(width(*w))
                    .flex_none()
                    .px_2()
                    .py_0p5()
                    .truncate()
                    .child(row.get(c).cloned().unwrap_or_default())
            }))
    }));
    div()
        .id("csv-preview")
        .size_full()
        .overflow_scroll()
        .p_2()
        .text_xs()
        .font_family(theme.mono_font_family.clone())
        .text_color(theme.muted_foreground)
        .child(table)
        .when(hidden > 0, |s| {
            s.child(
                div().p_2().child(
                    t!("%{n} more rows — save the file to see them", n = hidden).into_owned(),
                ),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn quotes_commas_and_newlines() {
        let rows = parse("id,name\r\n1,\"Silva, Ana\"\n2,\"say \"\"hi\"\"\nbye\"\n3,");
        assert_eq!(rows[0], ["id", "name"]);
        assert_eq!(rows[1], ["1", "Silva, Ana"]);
        assert_eq!(rows[2], ["2", "say \"hi\"\nbye"]);
        assert_eq!(rows[3], ["3", ""]);
        assert_eq!(rows.len(), 4);
    }
}
