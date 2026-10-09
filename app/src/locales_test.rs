//! Locale files vs the `t!` keys in the source. English is the source text;
//! every other supported language is one `locales/<lang>.yml`.

use std::collections::BTreeMap;

const SRC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
const LOCALES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/locales");

fn sources() -> Vec<String> {
    std::fs::read_dir(SRC)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect()
}

/// `(lang, translations)` per locale file.
fn locales() -> Vec<(String, BTreeMap<String, String>)> {
    std::fs::read_dir(LOCALES)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml"))
        .map(|p| {
            let lang = p.file_stem().unwrap().to_string_lossy().into_owned();
            let map = serde_yaml::from_str(&std::fs::read_to_string(&p).unwrap())
                .unwrap_or_else(|e| panic!("{lang}.yml: {e}"));
            (lang, map)
        })
        .collect()
}

/// Keys written as string literals inside `t!(…)`. Keys passed as variables (`t!(label)`) are
/// covered by `locales_share_one_key_set` + `no_stale_keys` instead.
fn literal_keys(src: &str) -> Vec<String> {
    let mut keys = vec![];
    for (at, _) in src.match_indices("t!(\"") {
        // skip `format!(`, `print!(`…
        if src[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let mut key = String::new();
        let mut chars = src[at + 4..].chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => key.push(chars.next().unwrap()),
                c => key.push(c),
            }
        }
        keys.push(key);
    }
    keys
}

/// A missing key silently shows English in that language.
#[test]
fn every_locale_covers_every_literal_key() {
    let keys: Vec<String> = sources().iter().flat_map(|s| literal_keys(s)).collect();
    for (lang, map) in locales() {
        let missing: Vec<&String> = keys.iter().filter(|k| !map.contains_key(*k)).collect();
        assert!(
            missing.is_empty(),
            "missing in locales/{lang}.yml: {missing:#?}"
        );
    }
}

/// Every key must still exist as a string literal somewhere in `src/` —
/// catches English text that changed (or copy that was removed) while its
/// old translation stayed behind.
#[test]
fn no_stale_keys() {
    let src = sources().concat();
    for (lang, map) in locales() {
        let stale: Vec<&String> = map
            .keys()
            .filter(|k| !src.contains(&format!("{k:?}")))
            .collect();
        assert!(
            stale.is_empty(),
            "not in src/, remove or update in locales/{lang}.yml: {stale:#?}"
        );
    }
}

/// A key translated in one language but not another (variable keys included).
#[test]
fn locales_share_one_key_set() {
    let all = locales();
    let Some((first, base)) = all.first() else {
        return;
    };
    for (lang, map) in &all[1..] {
        let diff: Vec<&String> = base
            .keys()
            .filter(|k| !map.contains_key(*k))
            .chain(map.keys().filter(|k| !base.contains_key(*k)))
            .collect();
        assert!(
            diff.is_empty(),
            "{first}.yml and {lang}.yml differ: {diff:#?}"
        );
    }
}
