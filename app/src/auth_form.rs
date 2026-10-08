//! Auth pane: scheme select + its fields (`auth.rs` says which). Text fields
//! take `{{ VAR }}` chips like the URL; secrets are masked with an eye toggle.

use std::collections::HashMap;

use gpui_kit::component::{
    button::Button,
    h_flex,
    input::{Input, InputEvent, InputState},
    searchable_list::SearchableVec,
    select::{Select, SelectEvent, SelectState},
    v_flex, ActiveTheme as _, IndexPath, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use serde_json::{Map, Value};
use yuki_core::AuthConfig;

use crate::{
    auth::{self, Ty, SCHEMES},
    chip_templates,
    editor::RequestEditor,
    vars,
};

type KindSelect = SelectState<SearchableVec<SharedString>>;

enum Slot {
    Input(Entity<InputState>),
    Pick(SharedString),
}

pub struct AuthForm {
    kind: Entity<KindSelect>,
    scheme: usize,
    /// Stored object of the current scheme: keys the form doesn't own ride along.
    base: Map<String, Value>,
    slots: Vec<Slot>,
    /// What was typed in each scheme this session, so switching back restores it.
    drafts: HashMap<usize, Map<String, Value>>,
    /// OAuth 2 token feedback.
    pub status: SharedString,
    _subs: Vec<Subscription>,
}

impl AuthForm {
    pub fn new(
        auth: &AuthConfig,
        window: &mut Window,
        cx: &mut Context<RequestEditor>,
    ) -> (Self, Subscription) {
        let scheme = auth::scheme_ix(auth);
        let names: Vec<SharedString> = SCHEMES.iter().map(|s| s.label.into()).collect();
        let kind = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(names),
                Some(IndexPath::new(scheme)),
                window,
                cx,
            )
        });
        let sub = cx.subscribe_in(
            &kind,
            window,
            |this, _, _: &SelectEvent<SearchableVec<SharedString>>, window, cx| {
                this.switch_auth(window, cx)
            },
        );
        let mut form = Self {
            kind,
            scheme,
            base: auth::object(auth),
            slots: vec![],
            drafts: HashMap::new(),
            status: "".into(),
            _subs: vec![],
        };
        form.fill(window, cx);
        (form, sub)
    }

    /// One slot per field of the current scheme, seeded from `base`.
    fn fill(&mut self, window: &mut Window, cx: &mut Context<RequestEditor>) {
        self._subs.clear();
        let mut slots = Vec::new();
        for field in SCHEMES[self.scheme].fields {
            let value = auth::initial(field, &self.base);
            if !matches!(field.ty, Ty::Text | Ty::Secret) {
                slots.push(Slot::Pick(value.into()));
                continue;
            }
            let input = cx.new(|cx| {
                let mut s = InputState::new(window, cx).masked(field.ty == Ty::Secret);
                s.set_value(vars::content(&value), window, cx);
                s
            });
            self._subs.push(cx.subscribe_in(
                &input,
                window,
                |this, input, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::Change) {
                        chip_templates!(input, window, cx);
                        this.update_completion(input, cx);
                        this.refresh_dirty(cx);
                    }
                },
            ));
            slots.push(Slot::Input(input));
        }
        self.slots = slots;
    }

    fn values(&self, cx: &App) -> Vec<String> {
        self.slots
            .iter()
            .map(|s| match s {
                Slot::Input(i) => i.read(cx).value().to_string(),
                Slot::Pick(v) => v.to_string(),
            })
            .collect()
    }

    /// The auth as edited; `None` only if the schema lost a required field.
    pub fn draft(&self, cx: &App) -> Option<AuthConfig> {
        auth::build(&SCHEMES[self.scheme], &self.base, &self.values(cx))
    }

    pub fn label(&self) -> &'static str {
        match self.scheme {
            0 => "—",
            ix => SCHEMES[ix].label,
        }
    }

    /// Text fields, for `{{ VAR }}` usage.
    pub fn texts(&self, cx: &App) -> Vec<SharedString> {
        self.slots
            .iter()
            .filter_map(|s| match s {
                Slot::Input(i) => Some(i.read(cx).value()),
                Slot::Pick(_) => None,
            })
            .collect()
    }

    pub fn is_oauth2(&self) -> bool {
        SCHEMES[self.scheme].kind == "oauth2"
    }

    /// The OAuth 2 grant picked (first field), for field visibility.
    fn grant(&self) -> SharedString {
        match self.slots.first() {
            Some(Slot::Pick(g)) if self.is_oauth2() => g.clone(),
            _ => "".into(),
        }
    }
}

impl RequestEditor {
    fn switch_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.auth.kind.read(cx).selected_index(cx).map(|i| i.row) else {
            return;
        };
        if ix == self.auth.scheme {
            return;
        }
        if let Some(a) = self.auth.draft(cx) {
            self.auth.drafts.insert(self.auth.scheme, auth::object(&a));
        }
        let stored =
            (auth::scheme_ix(&self.saved.auth) == ix).then(|| auth::object(&self.saved.auth));
        self.auth.base = self
            .auth
            .drafts
            .get(&ix)
            .cloned()
            .or(stored)
            .unwrap_or_default();
        self.auth.scheme = ix;
        self.auth.status = "".into();
        self.auth.fill(window, cx);
        self.refresh_dirty(cx);
        cx.notify();
    }

    pub(crate) fn render_auth(&self, cx: &mut Context<Self>) -> AnyElement {
        let form = &self.auth;
        let scheme = &SCHEMES[form.scheme];
        let grant = form.grant();
        let theme = cx.theme();
        let (fg, muted) = (theme.foreground, theme.muted_foreground);
        let active_bg = theme.secondary;
        let radius = theme.radius;

        let rows = scheme
            .fields
            .iter()
            .zip(&form.slots)
            .enumerate()
            .filter(|(_, (field, _))| auth::visible(field, &grant))
            .map(|(ix, (field, slot))| {
                let control: AnyElement = match slot {
                    Slot::Input(input) => div()
                        .flex_1()
                        .max_w(px(560.))
                        .child(
                            Input::new(input)
                                .small()
                                .token(self.chip_renderer())
                                .when(field.ty == Ty::Secret, |i| i.mask_toggle()),
                        )
                        .into_any_element(),
                    Slot::Pick(current) => {
                        let options: Vec<String> = match field.ty {
                            Ty::Choice(opts) => opts.iter().map(|o| o.to_string()).collect(),
                            _ => vec!["true".into(), "false".into()],
                        };
                        h_flex()
                            .gap_1()
                            .children(options.into_iter().map(|opt| {
                                let active = current.as_ref() == opt;
                                let label = match opt.as_str() {
                                    "true" => "sim".to_string(),
                                    "false" => "não".to_string(),
                                    o => o.replace('_', " "),
                                };
                                div()
                                    .id(SharedString::from(format!("auth-{ix}-{opt}")))
                                    .px_1p5()
                                    .rounded(radius)
                                    .cursor_pointer()
                                    .text_color(if active { fg } else { muted })
                                    .when(active, |s| s.bg(active_bg))
                                    .hover(|s| s.text_color(fg))
                                    .child(label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.auth.slots[ix] = Slot::Pick(opt.clone().into());
                                        // A token status is about the old grant.
                                        this.auth.status = "".into();
                                        this.refresh_dirty(cx);
                                        cx.notify();
                                    }))
                            }))
                            .into_any_element()
                    }
                };
                h_flex()
                    .gap_3()
                    .min_h(px(28.))
                    .child(
                        div()
                            .w(px(130.))
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(field.label),
                    )
                    .child(control)
            })
            .collect::<Vec<_>>();

        let token = form.is_oauth2().then(|| {
            h_flex()
                .gap_3()
                .pt_1()
                .child(div().w(px(130.)).flex_none())
                .child(
                    Button::new("oauth2-token")
                        .small()
                        .outline()
                        .label("obter token")
                        .on_click(cx.listener(|this, _, window, cx| this.fetch_token(window, cx))),
                )
                .child(div().text_xs().text_color(muted).child(form.status.clone()))
        });

        v_flex()
            .size_full()
            .gap_2()
            .child(
                h_flex()
                    .gap_4()
                    .text_xs()
                    .text_color(muted)
                    .child(
                        div()
                            .w(px(150.))
                            .child(Select::new(&form.kind).appearance(false).xsmall()),
                    )
                    .child(scheme.hint),
            )
            .child(
                v_flex()
                    .id("auth-fields")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_1()
                    .children(rows)
                    .children(token),
            )
            .into_any_element()
    }
}
