//! Вид окна настроек (`SettingsModal.vue` 0.4.9, стили компонента): окно как у профиля
//! (min(1040, 100 % − 64) × min(720, 100 % − 64), крестик 40 px, шапка со значком 88 px,
//! заголовком 28/36 и чипом версии, вкладки `l-ink`), разделы со своей прокруткой и сменой
//! `l-slide-*`, подвал. Строки `.settings-row`, группы `h4.settings-group`, выбор
//! `.settings-select`, путь `.settings-path`, кнопки `.settings-btn` (как у «Персонализации»).

use super::{SettingsModal, State, themes};
use crate::configs::footer::{BtnLook, icon_label, settings_btn};
use crate::icons::{chevron, fa_glyph, icon, svg_icon};
use crate::kit::button::{focus_handle, focus_visible, outline, white};
use crate::kit::input::FieldStyle;
use crate::kit::{
    CheckLook, Checked, Item, Tab, TabStyle, TextInput, ToggleState, check, dropdown, tabs, toggle,
};
use crate::layer::{TipExt, listen};
use crate::liquid;
use crate::motion::{SLIDE_NEXT, SLIDE_PREV, Timing, hover, mix, swap, tween};
use crate::paint::{Radial, ring, shadow};
use crate::scale::{u, viewport, zoom};
use crate::scroll::{Bar, ScrollArea};
use crate::text::{MONO, RUBIK, TextExt, spaced};
use crate::theme::Theme;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Div, Entity, FontWeight, Hsla, IntoElement, ObjectFit,
    Role, SharedString, WeakEntity, Window, div, img, prelude::*, px, rgb,
};
use hlsr_ui::controllers::settings as logic;
use std::collections::HashMap;
use std::rc::Rc;

const ERROR: u32 = 0xff6b78;
/// HLSR-CE: пункт «Свой exe…» в списке движков мода.
pub(super) const PICK_EXE: &str = "__pick__";

/// Снимок окна для разделов (раздел, который уходит, рисуется из того же снимка).
pub(super) struct Snap {
    pub(super) state: State,
    pub(super) theme: Theme,
    pub(super) weak: WeakEntity<SettingsModal>,
    pub(super) path: Option<Entity<TextInput>>,
    /// HLSR-CE: поля параметров запуска модов (по id мода).
    pub(super) mod_args: HashMap<String, Entity<TextInput>>,
    /// Ширина окна, CSS px.
    pub(super) width: f32,
    pub(super) zoom: f32,
}

impl Snap {
    pub(super) fn t(&self, key: &str) -> String {
        self.state.t(key)
    }

    pub(super) fn tr(&self, key: &str, args: &[&str]) -> String {
        hlsr_ui::tr(self.state.lang, key, args)
    }

    /// Обработчик события: шаг окна.
    pub(super) fn on<E: ?Sized>(
        &self,
        f: impl Fn(&mut SettingsModal, &mut Window, &mut Context<SettingsModal>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        let weak = self.weak.clone();
        move |_, window, cx| {
            weak.update(cx, |this, cx| f(this, window, cx)).ok();
        }
    }

    fn color(&self, key: &str) -> Hsla {
        self.theme.color(key)
    }
}

/// `.settings-path__input` в рамке `.settings-path`: поля 11/14, 13 px; наведение .07;
/// фокус — рамка `--accent`; `.invalid` — `#ff4d5e`. Справа место под кнопку «Выбрать» (42).
pub(super) fn path_style(theme: &Theme) -> FieldStyle {
    FieldStyle {
        height: 0.,
        pad_x: 14.,
        pad_right: Some(14. + 42.),
        pad_y: 11.,
        // `line-height: normal` Rubik 13 px в Chromium — 16 px (поле 40 с рамкой).
        line: 16.,
        hover_border: theme.color("l-border"),
        glow: None,
        ..FieldStyle::gx(theme)
    }
}

/// HLSR-CE: поле параметров запуска мода — как путь библиотеки, без места под кнопку.
pub(super) fn args_style(theme: &Theme) -> FieldStyle {
    FieldStyle {
        pad_right: None,
        ..path_style(theme)
    }
}

impl SettingsModal {
    pub(super) fn frame(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.global::<Theme>().clone();
        let zoom = zoom(window);
        let (vw, vh) = viewport(window);
        let width = (vw - 64.).min(1040.);
        let height = (vh - 64.).min(720.);
        let snap = Rc::new(Snap {
            state: self.state.clone(),
            theme: theme.clone(),
            weak: cx.weak_entity(),
            path: self.path.clone(),
            mod_args: self
                .mod_args
                .iter()
                .map(|(id, (input, _))| (id.clone(), input.clone()))
                .collect(),
            width,
            zoom,
        });
        let st = &snap.state;
        let title = snap.t("#UI_SETTINGS");

        // ---- Шапка: значок, заголовок, чип версии, вкладки ----
        let badge = div()
            .relative()
            .flex_none()
            .size(u(88.))
            .rounded(u(18.))
            .overflow_hidden()
            .bg(theme.color("l-modal-bg"))
            .child(
                div().absolute().inset_0().child(
                    Radial {
                        center: (0.3, 0.2),
                        radius: (1., 1.),
                        stops: vec![(0., 0.22), (0.7, 0.)],
                    }
                    .paint(88., 88., theme.color("l-accent")),
                ),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(u(18.))
                    .shadow(vec![ring(zoom, 1., theme.color("l-border"))])
                    .text_color(theme.color("l-accent"))
                    .child(fa_glyph("fas fa-cog", 36.)),
            );
        let platform = if st.linux { "Linux" } else { "Windows" };
        let chip = div()
            .py(u(4.))
            .px(u(10.))
            .rounded(theme.rem("l-radius-sm"))
            .bg(white(0.05))
            .text_px(12.5, 20.)
            .tnum()
            .text_color(theme.color("l-text-dim"))
            .child(format!("HLSR v{} · {platform}", st.version));
        let who = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .items_start()
            .gap(u(8.))
            .child(
                div()
                    .text_px(28., 36.)
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.color("l-text"))
                    .child(title.clone()),
            )
            .child(div().flex().child(chip));
        let tab_list: Vec<Tab> = st
            .sections
            .iter()
            .map(|section| Tab::new(snap.t(section.title)).icon(section.icon))
            .collect();
        let weak = cx.weak_entity();
        let bar = div().mt(u(20.)).ml(u(-12.)).child(crate::snap::target(
            "settings-tabs",
            tabs(
                "settings-tabs",
                tab_list,
                st.section,
                TabStyle::modal(&theme),
            )
            .label(title.clone())
            .on_select(move |index, _, cx| {
                weak.update(cx, |this, cx| this.select(*index, cx)).ok();
            }),
        ));
        let hero = div()
            .relative()
            .flex_none()
            .px(u(32.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(24.))
                    .pt(u(24.))
                    .pr(u(56.))
                    .child(badge)
                    .child(who),
            )
            .child(bar)
            .child(line(theme.color("l-modal-border"), true));

        // ---- Разделы ----
        let scrolls = self.scrolls.clone();
        let pages_snap = snap.clone();
        let pages = swap(
            "settings-page",
            st.section,
            if st.slide_next {
                SLIDE_NEXT
            } else {
                SLIDE_PREV
            },
            move |index: &usize| {
                let snap = &pages_snap;
                let id = snap.state.sections.get(*index).map_or("about", |s| s.id);
                let content = match id {
                    "general" => general(snap),
                    "downloads" => downloads(snap),
                    "mods" => mods(snap),
                    "proton" => proton(snap),
                    "interface" => interface(snap),
                    "diagnostics" => diagnostics(snap),
                    _ => about(snap),
                };
                let gap = match id {
                    "diagnostics" => 14.,
                    "proton" => 4.,
                    _ => 8.,
                };
                let scroll = scrolls.get(*index).cloned().unwrap_or_default();
                crate::snap::target(
                    "settings-body",
                    ScrollArea::new(("settings-scroll", *index), &scroll)
                        .bar(Bar::Thin)
                        .contain()
                        .size_full()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(u(gap))
                                .pt(u(28.))
                                .px(u(32.))
                                .pb(u(32.))
                                .children(content),
                        ),
                )
            },
        )
        .flex_1()
        .min_h_0()
        .overflow_hidden();

        // ---- Подвал ----
        let footer = div()
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .gap(u(12.))
            .py(u(16.))
            .px(u(32.))
            .child(line(theme.color("l-modal-border"), false))
            .child(crate::snap::target(
                "settings-reset",
                settings_btn("settings-reset", BtnLook::Ghost)
                    .on_click(snap.on(|this, _, cx| this.reset(cx)))
                    .child(snap.t("#L_RESET")),
            ))
            .child(div().flex_1())
            .child(crate::snap::target(
                "settings-cancel",
                settings_btn("settings-cancel", BtnLook::Plain)
                    .on_click(snap.on(|this, _, cx| this.close(cx)))
                    .child(snap.t("#L_CANCEL")),
            ))
            .child(crate::snap::target(
                "settings-save",
                settings_btn("settings-save", BtnLook::Primary)
                    .on_click(snap.on(|this, _, cx| this.submit(cx)))
                    .child(snap.t("#L_SAVE")),
            ));

        // ---- Крестик: правый край на линии содержимого ----
        let close_label = snap.t("#L_CLOSE");
        let close_id = gpui::ElementId::Name("settings-close".into());
        let close_focus = focus_handle(&close_id, window, cx);
        let close_ring = focus_visible(&close_focus, window);
        let (dim, text, border, accent) = (
            theme.color("l-text-dim"),
            theme.color("l-text"),
            theme.color("l-border"),
            theme.color("l-accent"),
        );
        let radius_sm = theme.len("l-radius-sm");
        let on_close = snap.on::<ClickEvent>(|this, _, cx| this.close(cx));
        let close = hover(close_id.clone(), Timing::fast(), move |t| {
            div()
                .id(close_id.clone())
                .button_text()
                .on_click(on_close)
                .role(Role::Button)
                .button_text()
                .aria_label(close_label.clone())
                .tip(close_label)
                .track_focus(&close_focus)
                .absolute()
                .top(u(24.))
                .right(u(32.))
                .size(u(40.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(u(radius_sm))
                .bg(mix(gpui::black().opacity(0.32), white(0.1), t))
                .shadow(vec![ring(zoom, 1., border)])
                .text_color(mix(dim, text, t))
                .cursor_pointer()
                .child(fa_glyph("fas fa-times", 15.))
                .when(close_ring, |el| {
                    el.child(outline(zoom, radius_sm, 2., 2., accent))
                })
        });

        let body = div()
            .id("settings-window")
            .role(Role::Dialog)
            .aria_label(title)
            .relative()
            .flex()
            .flex_col()
            .w(u(width))
            .h(u(height))
            .overflow_hidden()
            .bg(theme.color("l-modal-bg"))
            .rounded(theme.rem("l-radius"))
            .shadow(vec![
                ring(zoom, 1., theme.color("l-modal-border")),
                shadow(zoom, 0., 24., 72., 0., gpui::black().opacity(0.6)),
            ]);
        let radius = theme.rem("l-radius");
        let body = liquid::glass(body, liquid::Kind::Window, radius, px(0.), &theme, zoom)
            .child(hero)
            .child(pages)
            .child(footer)
            // Крестик — после вкладок: при открытии фокус получает выбранная вкладка.
            .child(crate::snap::target("settings-close", close));
        let on_close = listen(cx, |this: &mut SettingsModal, _, cx| this.close(cx));
        self.modal
            .render(window, cx, on_close, move || body)
            .into_any_element()
    }
}

/// Линия `box-shadow: inset 0 ∓1px 0` у края блока (места не занимает).
fn line(color: Hsla, bottom: bool) -> Div {
    let el = div().absolute().left_0().right_0().h(u(1.)).bg(color);
    if bottom { el.bottom_0() } else { el.top_0() }
}

// ---- Строки --------------------------------------------------------------------------------------

/// `h4.settings-group`: 12/16, 600, разрядка .08em, заглавные, `--l-text-dim`; поля 32/4
/// (первый в разделе — без верхнего).
fn group(snap: &Snap, key: &str, first: bool) -> AnyElement {
    div()
        .mt(u(if first { 0. } else { 32. }))
        .mb(u(4.))
        .child(spaced(
            snap.t(key).to_uppercase(),
            12.,
            16.,
            FontWeight::SEMIBOLD,
            0.08,
            snap.color("l-text-dim"),
        ))
        .into_any_element()
}

/// `.settings-row__label`: 14/24, 500.
fn label(snap: &Snap, text: String) -> Div {
    div()
        .text_px(14., 24.)
        .font_weight(FontWeight::MEDIUM)
        .text_color(snap.color("l-text"))
        .child(text)
}

/// `.settings-row__hint` (12/20, `--l-text-faint`).
fn hint(snap: &Snap, text: String) -> Div {
    div()
        .text_px(12., 20.)
        .text_color(snap.color("l-text-faint"))
        .child(text)
}

/// `.settings-path__error` / `__warning`: 12 px, строка 1,4.
fn note(text: String, color: u32) -> Div {
    div()
        .text_px(12., 12. * 1.4)
        .text_color(rgb(color))
        .child(text)
}

/// Место строки в разделе: первая — без верхнего поля, последняя — без линии.
#[derive(Clone, Copy, PartialEq)]
enum At {
    First,
    Middle,
    Last,
}

/// `.settings-row`: текст слева, контрол справа (`column` — под текстом), поля 16 сверху и
/// снизу, линия снизу .045.
fn row(texts: Vec<Div>, control: Option<AnyElement>, column: bool, at: At) -> AnyElement {
    let first = at == At::First;
    let last = at == At::Last;
    div()
        .relative()
        .flex()
        .pt(u(if first { 0. } else { 16. }))
        .pb(u(16.))
        .map(|el| {
            if column {
                el.flex_col().gap(u(12.))
            } else {
                el.items_center().justify_between().gap(u(20.))
            }
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap(u(4.))
                .min_w_0()
                .when(!column, |el| el.flex_1())
                .children(texts),
        )
        .children(control)
        .when(!last, |el| el.child(line(white(0.045), true)))
        .into_any_element()
}

/// `.settings-select`: от 150 px, поля 9/14, промежуток 12, 13 px / 500, шеврон 10 px; открыт
/// или наведён — фон .08 и `--l-border-strong`. Открытый — шеврон повёрнут на 180° (`--l-fast`).
/// Флаг, подпись и шеврон — отдельные дети `space-between`: подпись встаёт посередине между ними.
fn select_toggle(theme: &Theme, lead: Option<AnyElement>, text: String, state: ToggleState) -> Div {
    let open = state.open;
    let dim = theme.color("l-text-dim");
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(u(12.))
        .min_w(u(150.))
        .px(u(14.))
        .py(u(9.))
        .rounded(theme.rem("l-radius-sm"))
        .border_1()
        .border_color(theme.color(if open { "l-border-strong" } else { "l-border" }))
        .bg(white(if open { 0.08 } else { 0.05 }))
        .hover(|s| s.bg(white(0.08)))
        .text_px(13., 20.)
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.color("l-text"))
        .children(lead)
        .child(text)
        // `.settings-select.open i { transform: rotate(180deg) }` за `--l-fast`.
        .child(tween(
            "settings-chev",
            f32::from(u8::from(open)),
            Timing::fast(),
            move |t| chevron(10., 180. * t, dim),
        ))
}

/// Выпадающий список строки (`UiDropdown align="right"`).
fn select(
    snap: &Snap,
    id: &'static str,
    items: Vec<logic::Item>,
    value: String,
    label: String,
    pick: fn(&mut SettingsModal, &str, &mut Context<SettingsModal>),
) -> AnyElement {
    let items: Vec<Item> = items
        .into_iter()
        .map(|item| Item::new(item.id, item.label))
        .collect();
    let theme = snap.theme.clone();
    let weak = snap.weak.clone();
    crate::snap::target(
        id,
        dropdown(id, items, Some(value.into()))
            .align_right()
            .toggle(move |state| select_toggle(&theme, None, label, state))
            .on_select(move |item, _, cx| {
                weak.update(cx, |this, cx| pick(this, &item.id, cx)).ok();
            }),
    )
}

/// Флаг языка `.settings-lang__flag`: 21 × 14, скругление 3.
fn flag(code: &str) -> AnyElement {
    img(SharedString::from(format!("flags/flag-{code}.svg")))
        .flex_none()
        .w(u(21.))
        .h(u(14.))
        .rounded(u(3.))
        .object_fit(ObjectFit::Cover)
        .into_any_element()
}

/// Переключатель строки (`.settings-switch` = `.gx-switch`).
fn switch(snap: &Snap, key: &'static str, label_key: &str, default: bool) -> AnyElement {
    let on = snap.state.draft.get_bool(key, default);
    crate::snap::target(
        format!("settings-{key}"),
        toggle(format!("settings-{key}"), on)
            .label(snap.t(label_key))
            .on_change(snap.on(move |this, _, cx| {
                let on = !this.state.draft.get_bool(key, default);
                this.set_flag(key, on, cx)
            })),
    )
}

/// `.settings-about__link`: поля 9/16, промежуток 9, фон .05, рамка `--l-border`, 13 px / 500,
/// значок `--l-text-dim`; наведение — .09 и `--l-border-strong`.
fn link_button(
    snap: &Snap,
    id: &'static str,
    classes: &'static str,
    text: String,
    act: impl Fn(&mut SettingsModal, &mut Context<SettingsModal>) + 'static,
) -> AnyElement {
    let theme = &snap.theme;
    let (border, strong) = (theme.color("l-border"), theme.color("l-border-strong"));
    let (color, dim) = (theme.color("l-text"), theme.color("l-text-dim"));
    let radius = theme.rem("l-radius-sm");
    let on_click = snap.on::<ClickEvent>(move |this, _, cx| act(this, cx));
    crate::snap::target(
        id,
        hover(id, Timing::fast(), move |t| {
            div()
                .id(id)
                .role(Role::Button)
                .button_text()
                .flex()
                .items_center()
                .gap(u(9.))
                .py(u(9.))
                .px(u(16.))
                .rounded(radius)
                .border_1()
                .border_color(mix(border, strong, t))
                .bg(white(0.05 + 0.04 * t))
                .active(|s| s.bg(white(0.12)))
                .font_family(RUBIK)
                // `<button>`: `line-height: normal` Rubik (1,185).
                .text_px(13., 13. * 1.185)
                .font_weight(FontWeight::MEDIUM)
                .text_color(color)
                .cursor_pointer()
                .on_click(on_click)
                .child(fa_glyph(classes, 13.).text_color(dim))
                .child(text.clone())
        }),
    )
}

// ---- «Общие» -------------------------------------------------------------------------------------

fn general(snap: &Snap) -> Vec<AnyElement> {
    let st = &snap.state;
    let languages = logic::language_items();
    let language = {
        let theme = snap.theme.clone();
        let current = st.language.clone();
        let text = logic::language_label(&current);
        let items: Vec<Item> = languages
            .iter()
            .map(|item| Item::new(item.id.clone(), item.label.clone()))
            .collect();
        let weak = snap.weak.clone();
        crate::snap::target(
            "settings-language",
            dropdown("settings-language", items, Some(current.clone().into()))
                .align_right()
                .toggle(move |state| select_toggle(&theme, Some(flag(&current)), text, state))
                // Флаг и `.ui-dropdown__label` — дети пункта: зазор 12 px и цвет пункта
                // (невыбранный — `--l-text-dim`, светлеет при наведении; у слота своих правил
                // подсветки нет, поэтому `lit` не нужен).
                .item_view(move |item: &Item, _| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(u(12.))
                        .child(flag(&item.id))
                        .child(div().truncate().child(item.label.clone()))
                })
                .on_select(move |item, _, cx| {
                    weak.update(cx, |this, cx| this.set_language(&item.id, cx))
                        .ok();
                }),
        )
    };
    let discord = {
        let on = st.discord.enabled;
        crate::snap::target(
            "settings-discord",
            toggle("settings-discord", on)
                .label(snap.t("#L_S_DISCORD_RPC"))
                .on_change(snap.on(move |this, _, cx| {
                    let on = !this.state.discord.enabled;
                    this.set_discord_rpc(on, cx)
                })),
        )
    };
    let status = snap.t(logic::discord_status_key(
        st.discord.enabled,
        &st.discord.state,
    ));
    vec![
        group(snap, "#L_S_LAUNCHER_GROUP", true),
        row(
            vec![
                label(snap, snap.t("#L_S_LANGUAGE")),
                hint(snap, snap.t("#L_S_LANGUAGE_HINT")),
            ],
            Some(language),
            false,
            At::Middle,
        ),
        group(snap, "#L_S_WHILE_PLAYING", false),
        row(
            vec![
                label(snap, snap.t("#L_S_HIDE_PLAYING")),
                hint(snap, snap.t("#L_S_HIDE_PLAYING_HINT")),
            ],
            Some(switch(snap, "hideWhilePlaying", "#L_S_HIDE_PLAYING", true)),
            false,
            At::Middle,
        ),
        group(snap, "#L_S_INTEGRATIONS", false),
        row(
            vec![
                label(snap, snap.t("#L_S_DISCORD_RPC")),
                hint(snap, snap.t("#L_S_DISCORD_RPC_HINT")),
                hint(snap, status),
            ],
            Some(discord),
            false,
            At::Last,
        ),
    ]
}

// ---- «Загрузки» ----------------------------------------------------------------------------------

fn downloads(snap: &Snap) -> Vec<AnyElement> {
    let st = &snap.state;
    let theme = &snap.theme;
    let browse_label = snap.t("#L_S_BROWSE");
    let (dim, text, border) = (
        theme.color("l-text-dim"),
        theme.color("l-text"),
        theme.color("l-border"),
    );
    let on_browse = snap.on::<ClickEvent>(|this, _, cx| this.browse(cx));
    let browse = hover("settings-browse", Timing::fast(), move |t| {
        div()
            .id("settings-browse")
            .role(Role::Button)
            .button_text()
            .aria_label(browse_label.clone())
            .tip(browse_label.clone())
            .absolute()
            .top(u(1.))
            .right(u(1.))
            .bottom(u(1.))
            .w(u(42.))
            .flex()
            .items_center()
            .justify_center()
            .bg(white(0.08 * t))
            .active(|s| s.bg(white(0.12)))
            .cursor_pointer()
            .on_click(on_browse)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(u(1.))
                    .bg(border),
            )
            .child(svg_icon("glyphs/folder.svg", 18., mix(dim, text, t)))
    });
    let field = div()
        .relative()
        .w_full()
        .children(snap.path.clone())
        .child(crate::snap::target("settings-browse", browse));
    let path_texts = vec![label(snap, snap.t("#L_S_INSTALL_PATH"))];
    let mut path_parts: Vec<Div> = Vec::new();
    if let Some(error) = &st.path_error {
        path_parts.push(note(error.clone(), ERROR));
    } else if st.path_cyrillic {
        path_parts.push(note(snap.t("#UI_FOUND_CYRILLIC"), 0xffb37a));
    }
    let path_row = div()
        .flex()
        .flex_col()
        .gap(u(12.))
        .child(crate::snap::target("settings-path", field))
        .children(path_parts)
        .into_any_element();
    let speed = st.draft.speed_limit();
    let mut speed_texts = vec![
        label(snap, snap.t("#L_S_SPEED_LIMIT")),
        hint(snap, snap.t("#L_S_SPEED_LIMIT_HINT")),
    ];
    if let Some(error) = &st.download_error {
        speed_texts.push(note(error.clone(), ERROR));
    }
    vec![
        row(path_texts, Some(path_row), true, At::First),
        row(
            speed_texts,
            Some(select(
                snap,
                "settings-speed",
                logic::speed_items(st.lang),
                speed.to_string(),
                logic::speed_label(st.lang, speed),
                SettingsModal::set_speed,
            )),
            false,
            At::Last,
        ),
    ]
}

// ---- HLSR-CE: «Свои моды» -----------------------------------------------------------------------

fn mods(snap: &Snap) -> Vec<AnyElement> {
    let st = &snap.state;
    let view = &st.mods;
    let busy = st.mods_busy;
    let mut out = vec![group(snap, "#L_MODS_GROUP", true)];

    // Вступление, кнопки «Добавить папку» / «Найти в Steam» и заметка последнего действия.
    let mut intro = vec![hint(snap, snap.t("#L_MODS_INTRO"))];
    if let Some(n) = &st.mods_note {
        let args: Vec<&str> = n.arg.as_deref().into_iter().collect();
        let color = if n.error { ERROR } else { 0x5fd38d };
        intro.push(note(snap.tr(n.key, &args), color));
    }
    let actions = div()
        .flex()
        .flex_none()
        .gap(u(8.))
        .child(crate::snap::target(
            "settings-mods-add",
            settings_btn("settings-mods-add", BtnLook::Plain)
                .compact()
                .disabled(busy)
                .on_click(
                    snap.on(|this, _, cx| this.mods_do("custom-mods-add-folder", Vec::new(), cx)),
                )
                .children(icon_label(
                    "fas fa-folder-plus",
                    snap.t("#L_MODS_ADD"),
                    true,
                )),
        ))
        .child(crate::snap::target(
            "settings-mods-scan",
            settings_btn("settings-mods-scan", BtnLook::Plain)
                .compact()
                .disabled(busy)
                .on_click(snap.on(|this, _, cx| this.mods_do("custom-mods-scan", Vec::new(), cx)))
                .children(icon_label("fab fa-steam", snap.t("#L_MODS_SCAN"), true)),
        ))
        .into_any_element();
    out.push(row(intro, Some(actions), false, At::Middle));

    if !view.loaded || view.mods.is_empty() {
        let key = if view.loaded {
            "#L_MODS_EMPTY"
        } else {
            "#L_MODS_LOADING"
        };
        out.push(row(vec![hint(snap, snap.t(key))], None, false, At::Last));
        return out;
    }

    let count = view.mods.len();
    for (index, entry) in view.mods.iter().enumerate() {
        let at = if index + 1 == count {
            At::Last
        } else {
            At::Middle
        };
        let mut texts = vec![
            label(snap, entry.name.clone()),
            hint(snap, entry.mod_dir.clone()),
        ];
        if let Some(problem) = &entry.problem {
            texts.push(note(
                snap.t(hlsr_ui::controllers::custom_mods::problem_key(problem)),
                ERROR,
            ));
        }
        // Движок не годится — предложить найденный (HL2 Ghosting из лаунчера, если скачан).
        if let Some(suggested) = &entry.suggested {
            let (id, exe) = (entry.id.clone(), suggested.exe.clone());
            let target = format!("settings-mods-suggest-{}", entry.id);
            texts.push(
                div().flex().pt(u(4.)).child(crate::snap::target(
                    target.clone(),
                    settings_btn(SharedString::from(target), BtnLook::Primary)
                        .compact()
                        .disabled(busy)
                        .on_click(snap.on(move |this, _, cx| this.mods_set_engine(&id, &exe, cx)))
                        .children(icon_label(
                            "fas fa-magic",
                            snap.tr("#L_MODS_USE_ENGINE", &[&suggested.label]),
                            true,
                        )),
                )),
            );
        }

        // Движок: найденные, свои exe, «Свой exe…».
        let engine_name = |engine: &hlsr_ui::controllers::custom_mods::EngineEntry| {
            if engine.custom {
                snap.tr("#L_MODS_ENGINE_CUSTOM", &[&engine.label])
            } else {
                engine.label.clone()
            }
        };
        let mut items: Vec<Item> = view
            .engines
            .iter()
            .map(|engine| {
                let mut item = Item::new(engine.exe.clone(), engine_name(engine));
                item.hint = Some(engine.exe.clone().into());
                item
            })
            .collect();
        let mut pick = Item::new(PICK_EXE, snap.t("#L_MODS_ENGINE_PICK"));
        pick.separated = !items.is_empty();
        items.push(pick);
        let engine_text = match view.engines.iter().find(|e| e.exe == entry.exe) {
            Some(engine) => engine_name(engine),
            None => view
                .engine_label(&entry.exe)
                .map_or_else(|| snap.t("#L_MODS_ENGINE_NONE"), str::to_owned),
        };
        let theme = snap.theme.clone();
        let weak = snap.weak.clone();
        let mod_id = entry.id.clone();
        let engine = dropdown(
            SharedString::from(format!("settings-mods-engine-{}", entry.id)),
            items,
            Some(entry.exe.clone().into()),
        )
        .align_right()
        .label(snap.t("#L_MODS_ENGINE"))
        .toggle(move |state| select_toggle(&theme, None, engine_text, state))
        .on_select(move |item, _, cx| {
            weak.update(cx, |this, cx| this.mods_set_engine(&mod_id, &item.id, cx))
                .ok();
        });

        let id = entry.id.clone();
        let play = settings_btn(
            SharedString::from(format!("settings-mods-play-{id}")),
            BtnLook::Primary,
        )
        .compact()
        .disabled(busy || entry.problem.is_some())
        .on_click(snap.on(move |this, _, cx| {
            this.mods_do("custom-mods-launch", vec![serde_json::json!(id)], cx)
        }))
        .children(icon_label("fas fa-play", snap.t("#L_MODS_PLAY"), true));
        let id = entry.id.clone();
        let folder = settings_btn(
            SharedString::from(format!("settings-mods-folder-{id}")),
            BtnLook::Ghost,
        )
        .compact()
        .label(snap.t("#L_MODS_FOLDER"))
        .on_click(snap.on(move |this, _, cx| {
            this.mods_do("custom-mods-open-folder", vec![serde_json::json!(id)], cx)
        }))
        .child(fa_glyph("fas fa-folder-open", 13.5));
        let id = entry.id.clone();
        let remove = settings_btn(
            SharedString::from(format!("settings-mods-remove-{id}")),
            BtnLook::Ghost,
        )
        .compact()
        .disabled(busy)
        .label(snap.t("#L_MODS_REMOVE"))
        .on_click(snap.on(move |this, _, cx| {
            this.mods_do("custom-mods-remove", vec![serde_json::json!(id)], cx)
        }))
        .child(fa_glyph("fas fa-trash-alt", 13.5));

        let field = div()
            .flex_1()
            .min_w_0()
            .children(snap.mod_args.get(&entry.id).cloned());
        let controls = div()
            .flex()
            .items_center()
            .gap(u(8.))
            .w_full()
            .child(field)
            .child(div().flex_none().child(engine))
            .child(crate::snap::target(
                format!("settings-mods-play-{}", entry.id),
                play,
            ))
            .child(
                div()
                    .id(SharedString::from(format!(
                        "settings-mods-folder-tip-{}",
                        entry.id
                    )))
                    .tip(snap.t("#L_MODS_FOLDER"))
                    .child(folder),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "settings-mods-remove-tip-{}",
                        entry.id
                    )))
                    .tip(snap.t("#L_MODS_REMOVE"))
                    .child(remove),
            )
            .into_any_element();
        out.push(row(texts, Some(controls), true, at));
    }
    out
}

// ---- «Proton» (Linux) ----------------------------------------------------------------------------

fn proton(snap: &Snap) -> Vec<AnyElement> {
    let st = &snap.state;
    let theme = &snap.theme;
    let accent = theme.color("accent");
    let mut choices: Vec<AnyElement> = st
        .proton
        .installed
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let selected = st.proton_choice.as_deref() == Some(item.path.as_str());
            let path = item.path.clone();
            let fresh = st.proton.is_fresh(&item.name);
            let kind = snap.t(if item.recommended {
                "#L_S_PROTON_RECOMMENDED"
            } else {
                "#L_S_PROTON_SYSTEM"
            });
            div()
                .id(("proton-choice", index))
                .role(Role::RadioButton)
                .button_text()
                .relative()
                .flex()
                .items_center()
                .gap(u(12.))
                .min_w_0()
                .py(u(11.))
                .px(u(13.))
                .rounded(theme.rem("l-radius-sm"))
                .border_1()
                .border_color(if selected {
                    accent
                } else {
                    theme.color("l-border-strong")
                })
                .bg(if selected {
                    theme.color("accent-soft")
                } else {
                    white(0.025)
                })
                .hover(|s| s.bg(white(0.055)))
                .cursor_pointer()
                .on_click(snap.on(move |this, _, cx| this.proton_choose(&path, cx)))
                .child(
                    div()
                        .flex_none()
                        .size(u(36.))
                        .rounded(u(9.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(accent.opacity(0.1))
                        .text_color(accent)
                        .child(fa_glyph("fas fa-flask", 14.)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(u(2.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(u(8.))
                                .min_w_0()
                                .text_px(13.5, 18.)
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.color("l-text"))
                                .child(div().min_w_0().truncate().child(item.name.clone()))
                                .when(fresh, |el| {
                                    el.child(
                                        div()
                                            .flex_none()
                                            .h(u(20.))
                                            .px(u(8.))
                                            .flex()
                                            .items_center()
                                            .rounded(u(4.))
                                            .bg(accent)
                                            .text_color(theme.color("accent-contrast-text"))
                                            .child(spaced(
                                                snap.t("#L_S_PROTON_UPDATED_BADGE").to_uppercase(),
                                                10.,
                                                16.,
                                                FontWeight::BOLD,
                                                0.04,
                                                theme.color("accent-contrast-text"),
                                            )),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_px(11., 14.)
                                .text_color(theme.color("l-text-dim"))
                                .child(kind),
                        )
                        .child(
                            div()
                                .truncate()
                                .font_family(MONO)
                                .text_px(10., 14.)
                                .text_color(theme.color("l-text-faint"))
                                .child(item.path.clone()),
                        ),
                )
                .child(fa_glyph("fas fa-check", 12.).text_color(if selected {
                    accent
                } else {
                    white(0.)
                }))
                .into_any_element()
        })
        .collect();
    if st.proton.installed.is_empty() {
        choices.push(
            div()
                .p(u(18.))
                .rounded(theme.rem("l-radius-sm"))
                .border_1()
                .border_dashed()
                .border_color(theme.color("l-border-strong"))
                .text_px(13., 18.)
                .text_color(theme.color("l-text-dim"))
                .child(snap.t("#L_S_PROTON_EMPTY"))
                .into_any_element(),
        );
    }
    let choice_list = div()
        .flex()
        .flex_col()
        .gap(u(8.))
        .children(choices)
        .into_any_element();
    let mut out = vec![row(
        vec![
            label(snap, snap.t("#L_S_PROTON_TITLE")),
            hint(snap, snap.t("#L_S_PROTON_HINT")),
        ],
        Some(choice_list),
        true,
        At::First,
    )];
    let update = st.proton.update_text(st.lang);
    if !update.is_empty() {
        let tone = st.proton.update_tone();
        let (glyph, color) = match tone {
            "warn" => ("fas fa-exclamation-triangle", rgb(0xffc46b).into()),
            "fresh" => ("fas fa-arrow-circle-up", accent),
            _ => ("fas fa-check-circle", rgb(0x6fd38a).into()),
        };
        out.push(
            div()
                .mt(u(4.))
                .flex()
                .items_center()
                .gap(u(8.))
                .text_px(12., 16.)
                .text_color(theme.color(if tone == "fresh" {
                    "l-text"
                } else {
                    "l-text-dim"
                }))
                .child(fa_glyph(glyph, 12.).text_color(color))
                .child(update)
                .into_any_element(),
        );
    }
    let busy = st.proton.busy();
    if busy {
        let percent = st.proton.percent.clamp(0., 100.) as f32;
        out.push(
            div()
                .flex()
                .flex_wrap()
                .justify_between()
                .gap(u(7.))
                .text_px(12., 16.)
                .text_color(theme.color("l-text"))
                .child(snap.t(st.proton.phase_key()))
                .child(div().font_weight(FontWeight::BOLD).child(format!(
                    "{}%",
                    hlsr_core::js::number::number_to_string(st.proton.percent)
                )))
                .child(
                    div()
                        .w_full()
                        .h(u(5.))
                        .rounded(u(4.))
                        .overflow_hidden()
                        .bg(white(0.1))
                        .child(div().h_full().w(gpui::relative(percent / 100.)).bg(accent)),
                )
                .into_any_element(),
        );
    }
    if let Some(error) = st.proton_error.as_ref().or(st.proton.error.as_ref()) {
        out.push(
            note(
                format!("{}: {error}", snap.t("#L_S_PROTON_ERROR")),
                0xff8787,
            )
            .into_any_element(),
        );
    }
    let action = |id: &'static str,
                  classes: &'static str,
                  key: &str,
                  disabled: bool,
                  act: fn(&mut SettingsModal, &mut Context<SettingsModal>)| {
        crate::snap::target(
            id,
            settings_btn(id, BtnLook::Ghost)
                .disabled(disabled)
                .on_click(snap.on(move |this, _, cx| act(this, cx)))
                .child(fa_glyph(classes, 13.).opacity(0.9))
                .child(snap.t(key)),
        )
    };
    let mut actions = div().flex().flex_wrap().gap(u(8.)).pt(u(10.));
    if busy {
        actions = actions.child(action(
            "proton-cancel",
            "fas fa-times",
            "#L_CANCEL",
            false,
            |this, cx| this.proton_cancel(cx),
        ));
    }
    if st.proton.update.available {
        actions = actions.child(action(
            "proton-update",
            "fas fa-arrow-circle-down",
            "#L_S_PROTON_UPDATE",
            busy,
            |this, cx| this.proton_action("proton-update", cx),
        ));
    }
    actions = actions
        .child(action(
            "proton-download",
            "fas fa-download",
            "#L_S_PROTON_DOWNLOAD",
            busy,
            |this, cx| this.proton_action("proton-download", cx),
        ))
        .child(action(
            "proton-add",
            "fas fa-folder-plus",
            "#L_S_PROTON_ADD",
            busy,
            |this, cx| this.proton_add(cx),
        ))
        .child(crate::snap::target(
            "proton-refresh",
            settings_btn("proton-refresh", BtnLook::Ghost)
                .label(snap.t("#L_S_PROTON_REFRESH"))
                .icon_only(true)
                .on_click(snap.on(|this, _, cx| this.refresh_protons(cx)))
                .child(fa_glyph("fas fa-sync-alt", 13.)),
        ));
    out.push(actions.into_any_element());
    out
}

// ---- «Интерфейс» ---------------------------------------------------------------------------------

fn interface(snap: &Snap) -> Vec<AnyElement> {
    let st = &snap.state;
    let current_theme = st
        .draft
        .prefs
        .get("launcherTheme")
        .and_then(|v| v.as_str())
        .unwrap_or("hlsr")
        .to_owned();
    let current_accent = st
        .draft
        .prefs
        .get("paperAccent")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned();
    // Ширина содержимого: окно без полей 32 и полосы прокрутки 4 (раздел длиннее окна).
    let content = snap.width - 64. - 4.;
    let picker = themes::picker(snap, &current_theme, &current_accent, content);
    let scale = logic::scale_id(st.ui_scale);
    vec![
        row(
            vec![label(snap, snap.t("#L_PZ_THEMES"))],
            Some(picker),
            true,
            At::First,
        ),
        row(
            vec![
                label(snap, snap.t("#L_S_UI_SCALE")),
                hint(snap, snap.t("#L_S_UI_SCALE_HINT")),
            ],
            Some(select(
                snap,
                "settings-scale",
                logic::ui_scale_items(st.lang),
                scale,
                logic::ui_scale_label(st.ui_scale),
                SettingsModal::set_scale,
            )),
            false,
            At::Middle,
        ),
        row(
            vec![
                label(snap, snap.t("#L_S_FULLSCREEN_BUTTON")),
                hint(snap, snap.t("#L_S_FULLSCREEN_BUTTON_HINT")),
            ],
            Some(switch(
                snap,
                "fullscreenButton",
                "#L_S_FULLSCREEN_BUTTON",
                true,
            )),
            false,
            At::Middle,
        ),
        row(
            vec![
                label(snap, snap.t("#L_S_ANIMATIONS")),
                hint(snap, snap.t("#L_S_ANIMATIONS_HINT")),
            ],
            Some(switch(snap, "animations", "#L_S_ANIMATIONS", true)),
            false,
            At::Middle,
        ),
        // HLSR-CE: предел 60 кадров/с снят — интерфейс идёт с частотой монитора окна.
        row(
            vec![
                label(snap, snap.t("#L_S_UNCAPPED_FPS")),
                hint(snap, snap.t("#L_S_UNCAPPED_FPS_HINT")),
            ],
            Some(switch(snap, "uncappedFps", "#L_S_UNCAPPED_FPS", false)),
            false,
            At::Last,
        ),
    ]
}

// ---- «Диагностика» -------------------------------------------------------------------------------

fn diagnostics(snap: &Snap) -> Vec<AnyElement> {
    let st = &snap.state;
    let theme = &snap.theme;
    let contrast = theme.color("accent-contrast-text");
    let run_text = snap.t(if st.diag_running {
        "#L_DG_RUNNING"
    } else if st.diag_finished {
        "#L_DG_AGAIN"
    } else {
        "#L_DG_RUN"
    });
    let run = crate::snap::target(
        "diag-run",
        settings_btn("diag-run", BtnLook::Primary)
            .disabled(st.diag_running)
            .on_click(snap.on(|this, _, cx| this.run_diagnostics(cx)))
            .child(icon(
                if st.diag_running {
                    "fas fa-circle-notch fa-spin"
                } else {
                    "fas fa-play"
                },
                13.,
                contrast.opacity(0.9),
            ))
            .child(run_text),
    );
    let side = if st.diag_running {
        crate::snap::target(
            "diag-cancel",
            settings_btn("diag-cancel", BtnLook::Plain)
                .on_click(snap.on(|this, _, cx| this.cancel_diagnostics(cx)))
                .child(fa_glyph("fas fa-times", 13.).opacity(0.9))
                .child(snap.t("#L_DG_CANCEL")),
        )
    } else {
        crate::snap::target(
            "diag-export",
            settings_btn("diag-export", BtnLook::Ghost)
                .on_click(snap.on(|this, window, cx| this.open_export(window, cx)))
                .child(fa_glyph("fas fa-file-export", 13.).opacity(0.9))
                .child(snap.t("#L_S_EXPORT_DIAGNOSTICS")),
        )
    };
    let mut out = vec![
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(u(10.))
            .child(run)
            .child(div().flex_1())
            .child(side)
            .into_any_element(),
    ];
    if st.export_open {
        let dump = st.crash_dump.then(|| {
            check(
                "diag-dump",
                if st.export_include_dump {
                    Checked::On
                } else {
                    Checked::Off
                },
                CheckLook::Native,
            )
            .gap(u(8.))
            .text_px(12.5, 18.)
            .text_color(theme.color("l-text"))
            .on_change(snap.on(|this, _, cx| {
                let on = !this.state.export_include_dump;
                this.set_include_dump(on, cx)
            }))
            .child(snap.t("#L_DG_EXPORT_DUMP"))
        });
        let text = |key: &str, color: &str| {
            div()
                .text_px(12.5, 12.5 * 1.5)
                .text_color(theme.color(color))
                .child(snap.t(key))
        };
        out.push(
            div()
                .flex()
                .flex_col()
                .gap(u(8.))
                .p(u(14.))
                .rounded(theme.rem("l-radius-sm"))
                .border_1()
                .border_color(theme.color("l-border-strong"))
                .bg(white(0.03))
                .child(
                    div()
                        .text_px(13.5, 20.)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.color("l-text"))
                        .child(snap.t("#L_DG_EXPORT_TITLE")),
                )
                .child(text("#L_DG_EXPORT_INCLUDES", "l-text-dim"))
                .child(text("#L_DG_EXPORT_EXCLUDES", "l-text-faint"))
                .children(dump.map(|dump| crate::snap::target("diag-dump", dump)))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(u(10.))
                        .mt(u(4.))
                        .child(crate::snap::target(
                            "diag-export-cancel",
                            settings_btn("diag-export-cancel", BtnLook::Plain)
                                .on_click(snap.on(|this, _, cx| this.close_export(cx)))
                                .child(snap.t("#L_CANCEL")),
                        ))
                        .child(crate::snap::target(
                            "diag-export-go",
                            settings_btn("diag-export-go", BtnLook::Primary)
                                .disabled(st.export_busy)
                                .on_click(snap.on(|this, _, cx| this.export_zip(cx)))
                                .child(snap.t("#L_DG_EXPORT_GO")),
                        )),
                )
                .into_any_element(),
        );
    }
    if let Some(error) = &st.diag_error {
        out.push(
            div()
                .text_px(12.5, 18.)
                .text_color(theme.color("btn-danger-bg"))
                .child(error.clone())
                .into_any_element(),
        );
    }
    if st.diag_checks.is_empty() && !st.diag_running && st.diag_error.is_none() {
        out.push(
            // `.diag__empty`: строка 20 px от `body` (base.css).
            div()
                .py(u(18.))
                .px(u(2.))
                .text_px(13., 20.)
                .text_color(theme.color("l-text-faint"))
                .child(snap.t("#L_DG_EMPTY"))
                .into_any_element(),
        );
    }
    // `.diag__groups` в разделе всегда, и пустой: зазор 14 у него свой, как в 0.4.9.
    let groups = logic::diag_groups(&st.diag_checks, st.linux);
    out.push(
        div()
            .flex()
            .flex_col()
            .children(groups.into_iter().map(|(id, title, items)| {
                div()
                    .id(SharedString::from(format!("diag-group-{id}")))
                    .flex()
                    .flex_col()
                    .gap(u(2.))
                    .child(div().py(u(6.)).child(spaced(
                        snap.t(title).to_uppercase(),
                        12.5,
                        16.,
                        FontWeight::SEMIBOLD,
                        0.08,
                        theme.color("l-text"),
                    )))
                    .children(items.into_iter().map(|item| diag_row(snap, item)))
            }))
            .into_any_element(),
    );
    out.push(
        div()
            .flex()
            .gap(u(10.))
            .child(link_button(
                snap,
                "diag-logs",
                "fas fa-folder",
                snap.t("#L_S_OPEN_LOGS"),
                |this, cx| this.open_logs(cx),
            ))
            .child(link_button(
                snap,
                "diag-discord",
                "fab fa-discord",
                "Discord".into(),
                |this, cx| this.open_link("discord", cx),
            ))
            .into_any_element(),
    );
    out
}

/// `.diag__row`: значок и цвет полосы слева по состоянию, имя, значение; код — `<details>`
/// «Подробности» с кодом и подсказкой.
fn diag_row(snap: &Snap, item: &logic::DiagCheck) -> AnyElement {
    let theme = &snap.theme;
    let st = &snap.state;
    let tone: Option<Hsla> = match item.status.as_str() {
        "ok" => Some(rgb(0x2fa84f).into()),
        "warn" => Some(rgb(0xe8a33d).into()),
        "fail" => Some(rgb(0xe0455a).into()),
        _ => None,
    };
    let details = item.code.clone().map(|code| {
        let open = st.details.contains(&item.id);
        let id = item.id.clone();
        let summary = div()
            .id(SharedString::from(format!("diag-details-{}", item.id)))
            .flex()
            .items_center()
            .gap(u(4.))
            .text_px(11.5, 16.)
            .text_color(theme.color("l-text-faint"))
            .cursor_pointer()
            .on_click(snap.on(move |this, _, cx| this.toggle_details(&id, cx)))
            .child(svg_icon(
                if open {
                    "glyphs/disclosure-open.svg"
                } else {
                    "glyphs/disclosure-closed.svg"
                },
                8.,
                theme.color("l-text-faint"),
            ))
            .child(snap.t("#L_UA_DETAILS"));
        let hint_text = logic::diag_hint(st.lang, &code);
        div()
            .w_full()
            .child(crate::snap::target(
                format!("diag-details-{}", item.id),
                summary,
            ))
            .when(open, |el| {
                el.child(
                    div().flex().child(
                        div()
                            .my(u(4.))
                            .py(u(2.))
                            .px(u(8.))
                            .rounded(u(5.))
                            .bg(white(0.05))
                            .font_family(MONO)
                            .text_px(11.5, 16.)
                            .text_color(theme.color("l-text-dim"))
                            .child(code),
                    ),
                )
                .child(
                    div()
                        .mt(u(2.))
                        .text_px(12., 12. * 1.45)
                        .text_color(theme.color("l-text-dim"))
                        .child(hint_text),
                )
            })
    });
    div()
        .relative()
        .flex()
        .flex_wrap()
        .items_baseline()
        .gap(u(8.))
        .pt(u(5.))
        .pb(u(5.))
        .pr(u(2.))
        .pl(u(8.))
        .text_px(13., 18.)
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(u(2.))
                .bg(tone.unwrap_or_else(|| theme.color("l-border"))),
        )
        .child(
            div()
                .w(u(14.))
                .flex_none()
                .text_color(tone.unwrap_or_else(|| theme.color("l-text")))
                .child(fa_glyph(logic::status_icon(&item.status), 12.)),
        )
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.color("l-text"))
                .child(logic::diag_name(st.lang, &item.id)),
        )
        .child(
            div()
                .min_w_0()
                .text_px(12.5, 18.)
                .text_color(theme.color("l-text-dim"))
                .child(logic::diag_detail(st.lang, item)),
        )
        .children(details)
        .into_any_element()
}

// ---- «О программе» -------------------------------------------------------------------------------

/// Участники (`launcher-credits.js` 0.4.9): логин, имя, ключ роли, аватар, ключ ссылки.
const TEAM: [(&str, Option<&str>, &str, &str, &str); 4] = [
    (
        "InfiniteC0re",
        None,
        "#L_S_AUTHOR_FOUNDER",
        "launcher/team/InfiniteC0re.jpg",
        "authorInfiniteCore",
    ),
    (
        "prolert",
        None,
        "#L_S_AUTHOR_LEAD",
        "launcher/team/prolert.jpg",
        "authorProlert",
    ),
    (
        "kanoyo-git",
        Some("Kanoyo"),
        "#L_S_AUTHOR_NEWCOMER",
        "launcher/team/kanoyo-git.jpg",
        "authorKanoyo",
    ),
    (
        "ScriptedSnark",
        None,
        "#L_S_AUTHOR_MEMBER",
        "launcher/team/ScriptedSnark.png",
        "authorScriptedSnark",
    ),
];

fn about(snap: &Snap) -> Vec<AnyElement> {
    let st = &snap.state;
    let theme = &snap.theme;
    let titles = div()
        .flex()
        .flex_col()
        .gap(u(3.))
        .child(
            div()
                .flex()
                .items_start()
                .child(spaced(
                    "HLSR",
                    20.,
                    24.,
                    FontWeight::BOLD,
                    0.2,
                    theme.color("l-text"),
                ))
                .child(div().mt(u(2.)).ml(u(3.5)).child(spaced(
                    "BETA",
                    20. * 10. / 22.,
                    20. * 10. / 22.,
                    FontWeight::BOLD,
                    0.08,
                    theme.color("l-text-dim"),
                ))),
        )
        .child(
            div()
                .text_px(12.5, 15.)
                .text_color(theme.color("l-text-dim"))
                .child(format!("v{}", st.version)),
        );
    let links = div()
        .flex()
        .flex_wrap()
        .gap(u(10.))
        .ml_auto()
        .child(link_button(
            snap,
            "about-site",
            "fas fa-globe",
            snap.t("#L_S_WEBSITE"),
            |this, cx| this.open_link("site", cx),
        ))
        .child(link_button(
            snap,
            "about-github",
            "fab fa-github",
            snap.t("#L_S_GITHUB"),
            |this, cx| this.open_link("github", cx),
        ))
        .child(link_button(
            snap,
            "about-discord",
            "fab fa-discord",
            "Discord".into(),
            |this, cx| this.open_link("discord", cx),
        ));
    let head = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(u(16.))
        .gap_y(u(12.))
        .mb(u(4.))
        .child(
            img("logo.png")
                .flex_none()
                .size(u(56.))
                .rounded(u(12.))
                .object_fit(ObjectFit::Contain),
        )
        .child(titles)
        .child(links);
    let text = div()
        .mt(u(12.))
        .text_px(13.5, 13.5 * 1.6)
        .text_color(theme.color("l-text-dim"))
        .child(snap.t("#L_S_ABOUT_TEXT"));
    // Сетка `repeat(auto-fit, minmax(208px, 1fr))`, промежуток 12.
    let width = snap.width - 64.;
    let cols = (((width + 12.) / (208. + 12.)).floor() as usize).clamp(1, TEAM.len());
    let people = TEAM
        .iter()
        .map(|&(login, name, role, avatar, key)| person(snap, login, name, role, avatar, key));
    let team = div()
        .mt(u(20.))
        .child(group(snap, "#L_S_AUTHORS", true))
        .child(
            div()
                .mt(u(12.))
                .grid()
                .grid_cols(cols as u16)
                .gap(u(12.))
                .children(people),
        );
    vec![
        head.into_any_element(),
        text.into_any_element(),
        team.into_any_element(),
    ]
}

/// `.settings-team__person`: аватар 44, имя 14/20 600, роль 12/18, значок ссылки в углу.
fn person(
    snap: &Snap,
    login: &'static str,
    name: Option<&'static str>,
    role: &str,
    avatar: &'static str,
    key: &'static str,
) -> AnyElement {
    let theme = &snap.theme;
    let zoom = snap.zoom;
    let (border, strong) = (theme.color("l-border"), theme.color("l-border-strong"));
    let (text, dim, faint) = (
        theme.color("l-text"),
        theme.color("l-text-dim"),
        theme.color("l-text-faint"),
    );
    let radius = theme.rem("l-radius-sm");
    let title = snap.tr("#L_S_AUTHOR_GITHUB", &[login]);
    let role = snap.t(role);
    let id = SharedString::from(format!("about-{login}"));
    let on_click = snap.on::<ClickEvent>(move |this, _, cx| this.open_link(key, cx));
    crate::snap::target(
        id.clone(),
        hover(id.clone(), Timing::fast(), move |t| {
            div()
                .id(id.clone())
                .role(Role::Button)
                .button_text()
                .aria_label(title.clone())
                .tip(title.clone())
                .relative()
                .size_full()
                .flex()
                .items_start()
                .gap(u(12.))
                .py(u(12.))
                .px(u(14.))
                .rounded(radius)
                .bg(white(0.04 + 0.03 * t))
                .shadow(vec![ring(zoom, 1., mix(border, strong, t))])
                .cursor_pointer()
                .on_click(on_click)
                .child(
                    img(avatar)
                        .flex_none()
                        .size(u(44.))
                        .rounded(radius)
                        .object_fit(ObjectFit::Cover),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(u(2.))
                        .min_w_0()
                        .pr(u(8.))
                        .child(
                            div()
                                .text_px(14., 20.)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(text)
                                .child(name.unwrap_or(login)),
                        )
                        .child(div().text_px(12., 18.).text_color(dim).child(role.clone())),
                )
                .child(
                    div()
                        .absolute()
                        .top(u(12.))
                        .right(u(12.))
                        .text_color(faint)
                        .child(fa_glyph("fas fa-external-link-alt", 9.)),
                )
        }),
    )
}
