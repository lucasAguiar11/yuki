//! App-wide preferences (font sizes, language, response layout) in
//! `settings.yaml` in the data dir, applied over the theme at startup and from
//! the settings screen.

use std::{path::PathBuf, sync::OnceLock};

use gpui_kit::component::{
    searchable_list::SearchableVec, select::SelectState, ActiveTheme as _, IndexPath, Theme,
};
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use yuki_core::YukiError;

use crate::backend;

/// `None` = theme default / system language.
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mono_font_size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mono_font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Response beside the request editor instead of below it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub response_beside: bool,
    /// The folder every workspace lives in, one subfolder each. `None` =
    /// wherever each one is (the data dir by default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspaces_dir: Option<PathBuf>,
}

impl Global for Prefs {}

/// The theme's own typography, captured before any pref: what `None` and
/// "restore defaults" go back to.
struct Defaults {
    size: Pixels,
    mono_size: Pixels,
    family: SharedString,
    mono_family: SharedString,
}

static DEFAULTS: OnceLock<Defaults> = OnceLock::new();

pub type FamilySelect = SelectState<SearchableVec<SharedString>>;

#[derive(Clone, Copy)]
pub enum Font {
    Ui,
    Code,
}

fn path() -> PathBuf {
    backend::data_dir().join("settings.yaml")
}

impl Prefs {
    /// The file as saved; a broken or missing one means defaults. Read
    /// directly where the global isn't reachable (background imports).
    pub fn load() -> Prefs {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_yaml::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Startup: read the file, apply, keep global.
    pub fn init(cx: &mut App) {
        let prefs = Self::load();
        let theme = cx.theme();
        let _ = DEFAULTS.set(Defaults {
            size: theme.font_size,
            mono_size: theme.mono_font_size,
            family: theme.font_family.clone(),
            mono_family: theme.mono_font_family.clone(),
        });
        prefs.apply(cx);
        cx.set_global(prefs);
    }

    fn apply(&self, cx: &mut App) {
        // YUKI_LANG (dev override) beats the setting, which beats the system.
        let lang = std::env::var("YUKI_LANG")
            .ok()
            .or_else(|| self.language.clone())
            .or_else(sys_locale::get_locale)
            .unwrap_or_default();
        rust_i18n::set_locale(&lang);
        let Some(d) = DEFAULTS.get() else { return };
        let theme = Theme::global_mut(cx);
        theme.font_size = self.font_size.map_or(d.size, px);
        theme.mono_font_size = self.mono_font_size.map_or(d.mono_size, px);
        theme.font_family = self
            .font_family
            .clone()
            .map_or(d.family.clone(), Into::into);
        theme.mono_font_family = self
            .mono_font_family
            .clone()
            .map_or(d.mono_family.clone(), Into::into);
        cx.refresh_windows();
    }

    /// Sizes and families back to the theme's; the language stays.
    pub fn reset_fonts(cx: &mut App) -> Prefs {
        let prefs = cx.global_mut::<Prefs>();
        *prefs = Prefs {
            language: prefs.language.take(),
            response_beside: prefs.response_beside,
            workspaces_dir: prefs.workspaces_dir.take(),
            ..Prefs::default()
        };
        let prefs = prefs.clone();
        prefs.apply(cx);
        prefs
    }

    /// `None` = the theme's family. Applied at once; the caller saves.
    pub fn set_family(font: Font, family: Option<String>, cx: &mut App) -> Prefs {
        let prefs = cx.global_mut::<Prefs>();
        match font {
            Font::Ui => prefs.font_family = family,
            Font::Code => prefs.mono_font_family = family,
        }
        let prefs = prefs.clone();
        prefs.apply(cx);
        prefs
    }

    /// The theme's family for `font`, as it shows in the picker.
    pub fn default_family(font: Font) -> SharedString {
        DEFAULTS.get().map_or_else(SharedString::default, |d| {
            shown(match font {
                Font::Ui => &d.family,
                Font::Code => &d.mono_family,
            })
        })
    }

    /// Installed families plus the theme's own; the one in use selected.
    pub fn family_select(font: Font, window: &mut Window, cx: &mut App) -> Entity<FamilySelect> {
        let theme = cx.theme();
        let current = shown(match font {
            Font::Ui => &theme.font_family,
            Font::Code => &theme.mono_font_family,
        });
        // Dot-prefixed names are private to the system; only its UI font
        // comes back in, under its public name.
        let mut names: Vec<SharedString> = cx
            .text_system()
            .all_font_names()
            .into_iter()
            .filter(|n| !n.starts_with('.'))
            .map(Into::into)
            .collect();
        let default = Self::default_family(font);
        if !names.contains(&default) {
            names.push(default);
            names.sort_unstable();
        }
        let ix = names.iter().position(|n| *n == current).unwrap_or(0);
        cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(names),
                Some(IndexPath::new(ix)),
                window,
                cx,
            )
            .searchable(true)
        })
    }

    /// `None` = follow the system. Applied at once; the caller saves.
    pub fn set_language(lang: Option<&str>, cx: &mut App) -> Prefs {
        let prefs = cx.global_mut::<Prefs>();
        prefs.language = lang.map(str::to_owned);
        let prefs = prefs.clone();
        prefs.apply(cx);
        prefs
    }

    /// Response beside (`true`) or below the request editor. Applied at once
    /// (every window re-renders); the caller saves.
    pub fn set_response_beside(on: bool, cx: &mut App) -> Prefs {
        let prefs = cx.global_mut::<Prefs>();
        prefs.response_beside = on;
        let prefs = prefs.clone();
        cx.refresh_windows();
        prefs
    }

    pub fn save(&self) -> Result<(), YukiError> {
        let text = serde_yaml::to_string(&self).map_err(|e| YukiError::Storage(e.to_string()))?;
        yuki_storage::write_atomic(path(), text)
    }

    pub fn size(font: Font, cx: &App) -> f32 {
        let theme = cx.theme();
        match font {
            Font::Ui => theme.font_size,
            Font::Code => theme.mono_font_size,
        }
        .as_f32()
    }

    /// Step a font size and apply it; the caller saves the returned prefs.
    // ponytail: fixed-px row heights cap the UI size at 16; past that rows clip.
    pub fn bump(font: Font, delta: f32, cx: &mut App) -> Prefs {
        let (min, max) = match font {
            Font::Ui => (11., 16.),
            Font::Code => (10., 20.),
        };
        let size = Some((Self::size(font, cx) + delta).clamp(min, max));
        let prefs = cx.global_mut::<Prefs>();
        match font {
            Font::Ui => prefs.font_size = size,
            Font::Code => prefs.mono_font_size = size,
        }
        let prefs = prefs.clone();
        prefs.apply(cx);
        prefs
    }
}
/// macOS's UI font is `.SystemUIFont` to the text system; people know it as SF Pro.
fn shown(family: &SharedString) -> SharedString {
    if family.starts_with('.') {
        "SF Pro".into()
    } else {
        family.clone()
    }
}
