//! Settings → general's send fields: one value for every workspace
//! (`resolve::GlobalSettings`, machine-local). They ride the screen's
//! autosave with the workspace fields; each workspace's own still wins or adds.

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::*;
use yuki_mcp::resolve::GlobalSettings;

use crate::{
    draft,
    settings::{parse_names, SettingsScreen},
};

pub struct General {
    /// As last written.
    pub saved: GlobalSettings,
    pub skip_certs: bool,
    pub user_agent: Entity<InputState>,
    pub dns: Entity<TextareaState>,
    pub env: Entity<InputState>,
    _subs: [Subscription; 3],
}

impl General {
    pub fn new(
        saved: GlobalSettings,
        window: &mut Window,
        cx: &mut Context<SettingsScreen>,
    ) -> Self {
        let user_agent = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(yuki_core::DEFAULT_USER_AGENT)
                .default_value(saved.user_agent.clone())
        });
        let dns = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 12)
                .default_value(draft::hosts_text(&saved.dns_overrides))
        });
        let env = cx.new(|cx| {
            InputState::new(window, cx).default_value(saved.system_env_allowlist.join(", "))
        });
        let on_change =
            |this: &mut SettingsScreen, ev: &InputEvent, cx: &mut Context<SettingsScreen>| {
                if matches!(ev, InputEvent::Change) {
                    this.changed(cx);
                }
            };
        let _subs = [
            cx.subscribe(&user_agent, move |this, _, ev, cx| on_change(this, ev, cx)),
            cx.subscribe(&dns, move |this, _, ev, cx| on_change(this, ev, cx)),
            cx.subscribe(&env, move |this, _, ev, cx| on_change(this, ev, cx)),
        ];
        Self {
            skip_certs: saved.skip_certificate_check,
            saved,
            user_agent,
            dns,
            env,
            _subs,
        }
    }

    /// As edited, when it differs from what's stored. Untouched DNS text keeps
    /// the stored rows (and their ids) as they are.
    pub fn draft(&self, cx: &App) -> Option<GlobalSettings> {
        let mut g = self.saved.clone();
        g.user_agent = self.user_agent.read(cx).value().trim().to_string();
        let dns = self.dns.read(cx).value();
        if dns != draft::hosts_text(&g.dns_overrides) {
            g.dns_overrides = draft::hosts_rows(&dns, &g.dns_overrides);
        }
        g.system_env_allowlist = parse_names(&self.env.read(cx).value());
        g.skip_certificate_check = self.skip_certs;
        (g != self.saved).then_some(g)
    }
}
