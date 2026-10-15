//! Auth pane: scheme select + its fields (`auth.rs` says which). Hosted by the
//! request editor and the folder/workspace auth dialog (`AuthHost`). Secrets
//! are masked with an eye toggle.

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
    vars,
};

type KindSelect = SelectState<SearchableVec<SharedString>>;

/// Whose auth the form edits.
#[derive(Clone, Copy, PartialEq)]
pub enum Owner {
    Request,
    Folder,
    Workspace,
}

enum Slot {
    Input(Entity<InputState>),
    Pick(SharedString),
}

/// A view that holds an `AuthForm` and reacts to its edits.
pub trait AuthHost: Sized + 'static {
    fn form(&self) -> &AuthForm;
    fn form_mut(&mut self) -> &mut AuthForm;
    /// The auth on disk: switching back to its scheme restores it.
    fn stored_auth(&self) -> &AuthConfig;
    /// After any edit (typing, scheme, pick).
    fn auth_edited(&mut self, cx: &mut Context<Self>);
    /// A text field changed, before `auth_edited` (chips, completion).
    fn auth_typed(&mut self, _: &Entity<InputState>, _: &mut Window, _: &mut Context<Self>) {}
    /// Extra rendering on each text field (`{{ VAR }}` chips).
    fn decorate(&self, input: Input) -> Input {
        input
    }
    /// OAuth 2 "get token" button; `false` hides it.
    fn can_fetch_token(&self) -> bool {
        false
    }
    fn fetch_token(&mut self, _: &mut Window, _: &mut Context<Self>) {}
}

pub struct AuthForm {
    kind: Entity<KindSelect>,
    owner: Owner,
    /// `SCHEMES` indexes the select offers, in its row order.
    offered: Vec<usize>,
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
    pub fn new<H: AuthHost>(
        auth: &AuthConfig,
        owner: Owner,
        window: &mut Window,
        cx: &mut Context<H>,
    ) -> (Self, Subscription) {
        // Folders and the workspace are what `inherit` reads: they can't inherit.
        let offered: Vec<usize> = (0..SCHEMES.len())
            .filter(|&ix| owner == Owner::Request || SCHEMES[ix].kind != "inherit")
            .collect();
        let scheme = Some(auth::scheme_ix(auth))
            .filter(|ix| offered.contains(ix))
            .unwrap_or(0);
        let names: Vec<SharedString> = offered
            .iter()
            .map(|&ix| t!(SCHEMES[ix].label).into_owned().into())
            .collect();
        let row = offered.iter().position(|&ix| ix == scheme).unwrap_or(0);
        let kind = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(names),
                Some(IndexPath::new(row)),
                window,
                cx,
            )
        });
        let sub = cx.subscribe_in(
            &kind,
            window,
            |this: &mut H, _, _: &SelectEvent<SearchableVec<SharedString>>, window, cx| {
                switch(this, window, cx)
            },
        );
        let mut form = Self {
            kind,
            owner,
            offered,
            scheme,
            base: if scheme == 0 {
                Map::new()
            } else {
                auth::object(auth)
            },
            slots: vec![],
            drafts: HashMap::new(),
            status: "".into(),
            _subs: vec![],
        };
        form.fill(window, cx);
        (form, sub)
    }

    /// One slot per field of the current scheme, seeded from `base`.
    fn fill<H: AuthHost>(&mut self, window: &mut Window, cx: &mut Context<H>) {
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
                |this: &mut H, input, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::Change) {
                        this.auth_typed(input, window, cx);
                        this.auth_edited(cx);
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

    pub fn label(&self) -> std::borrow::Cow<'static, str> {
        match self.scheme {
            0 => "—".into(),
            ix => t!(SCHEMES[ix].label),
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

    /// Text field entities, for the `{{` autocomplete.
    pub fn inputs(&self) -> impl Iterator<Item = Entity<InputState>> + '_ {
        self.slots.iter().filter_map(|s| match s {
            Slot::Input(i) => Some(i.clone()),
            Slot::Pick(_) => None,
        })
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

    /// What the scheme does; "no auth" means something else one level up.
    fn hint(&self) -> &'static str {
        match (self.scheme, self.owner) {
            (0, Owner::Folder) => {
                "nothing here: inheriting requests look in the parent folder, then the workspace"
            }
            (0, Owner::Workspace) => {
                "inheriting requests with no folder auth go out unauthenticated"
            }
            (ix, _) => SCHEMES[ix].hint,
        }
    }
}

fn switch<H: AuthHost>(this: &mut H, window: &mut Window, cx: &mut Context<H>) {
    let form = this.form();
    let Some(row) = form.kind.read(cx).selected_index(cx).map(|i| i.row) else {
        return;
    };
    let ix = form.offered[row];
    if ix == form.scheme {
        return;
    }
    let stored = this.stored_auth();
    let stored = (auth::scheme_ix(stored) == ix).then(|| auth::object(stored));
    let form = this.form_mut();
    if let Some(a) = form.draft(cx) {
        form.drafts.insert(form.scheme, auth::object(&a));
    }
    form.base = form.drafts.get(&ix).cloned().or(stored).unwrap_or_default();
    form.scheme = ix;
    form.status = "".into();
    form.fill(window, cx);
    this.auth_edited(cx);
    cx.notify();
}

pub fn render<H: AuthHost>(host: &H, cx: &mut Context<H>) -> AnyElement {
    let form = host.form();
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
                        host.decorate(Input::new(input).small())
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
                                "true" => t!("yes").into_owned(),
                                "false" => t!("no").into_owned(),
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
                                .on_click(cx.listener(move |this: &mut H, _, _, cx| {
                                    let form = this.form_mut();
                                    form.slots[ix] = Slot::Pick(opt.clone().into());
                                    // A token status is about the old grant.
                                    form.status = "".into();
                                    this.auth_edited(cx);
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
                        .child(t!(field.label)),
                )
                .child(control)
        })
        .collect::<Vec<_>>();

    let token = (form.is_oauth2() && host.can_fetch_token()).then(|| {
        h_flex()
            .gap_3()
            .pt_1()
            .child(div().w(px(130.)).flex_none())
            .child(
                Button::new("oauth2-token")
                    .small()
                    .outline()
                    .label(t!("get token").into_owned())
                    .on_click(
                        cx.listener(|this: &mut H, _, window, cx| this.fetch_token(window, cx)),
                    ),
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
                        .flex_none()
                        .child(Select::new(&form.kind).appearance(false).xsmall()),
                )
                .child(t!(form.hint())),
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
