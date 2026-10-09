//! App-wide preferences (font sizes) in `settings.yaml` in the data dir,
//! applied over the theme at startup and from the settings screen.

use std::path::PathBuf;

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use yuki_core::YukiError;

use crate::backend;

/// `None` = theme default.
#[derive(Default, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mono_font_size: Option<f32>,
}

impl Global for Prefs {}

#[derive(Clone, Copy)]
pub enum Font {
    Ui,
    Code,
}

fn path() -> PathBuf {
    backend::data_dir().join("settings.yaml")
}

impl Prefs {
    /// Startup: read the file (a broken one means defaults), apply, keep global.
    pub fn init(cx: &mut App) {
        let prefs: Prefs = std::fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_yaml::from_str(&t).ok())
            .unwrap_or_default();
        prefs.apply(cx);
        cx.set_global(prefs);
    }

    fn apply(self, cx: &mut App) {
        let theme = Theme::global_mut(cx);
        if let Some(s) = self.font_size {
            theme.font_size = px(s);
        }
        if let Some(s) = self.mono_font_size {
            theme.mono_font_size = px(s);
        }
        cx.refresh_windows();
    }

    pub fn save(self) -> Result<(), YukiError> {
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
        let prefs = *prefs;
        prefs.apply(cx);
        prefs
    }
}
