//! HLSR-CE: свои Source-моды в «Играх» (вкладка `mods`, пункт «Библиотека модов» в колонке).
//!
//! - Библиотека (мод не выбран): плитки модов (щелчок — страница мода), «Добавить папку»,
//!   «Найти в Steam», «Установить из архива» (zip/7z) и настройки модов.
//! - Страница мода — как у инструмента: герой (название, движок, папка, предупреждения о
//!   недостающих эпизодах — жёлтым, движок, галочки частых параметров, строка параметров) и
//!   нижняя панель (все моды, папка, убрать из списка, «Играть»).
//!
//! Данные — каналы `custom-mods-*`, список перечитывается по событию `custom-mods-changed`;
//! выбранный мод — [`NavController::selected_mod`] (пусто — библиотека).
//!
//! [`NavController::selected_mod`]: hlsr_ui::controllers::nav::NavController::selected_mod

use super::hero::{subtitle, title};
use super::tool_panel::{feather, side_button};
use crate::configs::footer::{BtnLook, icon_label, settings_btn};
use crate::icons::{chevron, icon};
use crate::kit::button::white;
use crate::kit::input::FieldStyle;
use crate::kit::{
    CheckLook, Checked, FieldEvent, Item, Look, TextInput, Tone, button, check, dropdown,
};
use crate::motion::{Timing, hover, mix};
use crate::scale::{u, viewport};
use crate::shell::root::ShellContext;
use crate::text::{TextExt, spaced};
use crate::theme::Theme;
use gpui::{
    AnyElement, App, AppContext, Context, Div, Entity, FontWeight, IntoElement, Render,
    SharedString, Subscription, Task, Window, div, prelude::*, rgb,
};
use hlsr_core::language::Language;
use hlsr_ui::controllers::custom_mods::{
    LAUNCH_FLAGS, ModEntry, ModsView, episode_key, has_flag, note_for, problem_key, set_flag,
};
use hlsr_ui::services::Services;
use serde_json::{Value, json};

/// Пункт «Свой exe…» в списке движков.
const PICK_EXE: &str = "__pick__";
const ERROR: u32 = 0xff6b78;
/// Предупреждение (недостающие эпизоды): жёлтый.
const WARNING: u32 = 0xffc857;
const OK: u32 = 0x5fd38d;

pub struct ModPage {
    services: Services,
    view: ModsView,
    busy: bool,
    /// Поле параметров запуска и мод, чьи параметры в нём.
    args: Option<(Entity<TextInput>, Subscription)>,
    args_for: String,
    _events: Task<()>,
}

fn language(cx: &App) -> Language {
    ShellContext::get(cx).map_or(Language::En, |shell| shell.shell.language())
}

/// Поле параметров и поиска — как путь библиотеки в настройках.
fn field_style(theme: &Theme) -> FieldStyle {
    FieldStyle {
        height: 0.,
        pad_x: 14.,
        pad_y: 11.,
        line: 16.,
        hover_border: theme.color("l-border"),
        glow: None,
        ..FieldStyle::gx(theme)
    }
}

/// Тост с итогом действия.
fn toast(channel: &str, reply: &Value, cx: &App) {
    let (Some(note), Some(shell)) = (note_for(channel, reply), ShellContext::get(cx)) else {
        return;
    };
    let args: Vec<&str> = note.arg.as_deref().into_iter().collect();
    let icon = if note.error {
        "fas fa-exclamation-triangle"
    } else {
        "fas fa-check"
    };
    shell.shell.notify(&shell.shell.tr(note.key, &args), icon);
}

impl ModPage {
    pub fn new(services: &Services, cx: &mut Context<ModPage>) -> ModPage {
        let events = crate::bridge::listen(cx, services.subscribe(), |this, event, cx| {
            if event.channel == "custom-mods-changed" {
                this.load(cx);
            }
        });
        let mut page = ModPage {
            services: services.clone(),
            view: ModsView::default(),
            busy: false,
            args: None,
            args_for: String::new(),
            _events: events,
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.act("custom-mods-list", Vec::new(), cx);
    }

    /// Действие с модами: ответ — новый снимок; итог — тостом.
    fn act(&mut self, channel: &'static str, args: Vec<Value>, cx: &mut Context<Self>) {
        let quiet = channel == "custom-mods-list";
        if !quiet {
            self.busy = true;
            cx.notify();
        }
        let reply = self.services.invoke(channel, args);
        cx.spawn(async move |this, cx| {
            let reply = reply.await.unwrap_or_else(|_| json!({"ok": false}));
            this.update(cx, |this, cx| {
                if !quiet {
                    this.busy = false;
                    toast(channel, &reply, cx);
                }
                if let Some(view) = ModsView::from_value(&reply) {
                    this.view = view;
                }
                // Мод убран — назад в библиотеку.
                if channel == "custom-mods-remove"
                    && let Some(shell) = ShellContext::get(cx).cloned()
                {
                    let selected = shell.nav.selected_mod();
                    if !this.view.mods.iter().any(|m| m.id == selected) {
                        shell.nav.select_mod("");
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_engine(&mut self, id: &str, exe: &str, cx: &mut Context<Self>) {
        if exe == PICK_EXE {
            self.act("custom-mods-pick-exe", vec![json!(id)], cx);
        } else {
            self.act("custom-mods-set-engine", vec![json!(id), json!(exe)], cx);
        }
    }

    /// Галочка параметра: флаг в строке параметров (и в поле), сохранение.
    fn toggle_flag(&mut self, flag: &str, cx: &mut Context<Self>) {
        let Some((input, _)) = &self.args else {
            return;
        };
        let current = input.read(cx).value();
        let next = set_flag(&current, flag, !has_flag(&current, flag));
        let text = next.clone();
        input.update(cx, |input, cx| input.set_text(text, cx));
        let id = self.args_for.clone();
        self.act("custom-mods-set-args", vec![json!(id), json!(next)], cx);
    }

    /// Поле параметров показывает параметры выбранного мода.
    fn sync_args(&mut self, entry: &ModEntry, window: &mut Window, cx: &mut Context<Self>) {
        if self.args.is_none() {
            let placeholder = hlsr_ui::tr(language(cx), "#L_MODS_ARGS_PLACEHOLDER", &[]);
            let input = cx.new(|cx| {
                TextInput::new(window, cx)
                    .style(field_style)
                    .placeholder(placeholder)
                    .max_length(1024)
            });
            let events = cx.subscribe(&input, |this, input, event, cx| {
                if *event == FieldEvent::Change && !this.args_for.is_empty() {
                    let text = input.read(cx).value();
                    let id = this.args_for.clone();
                    this.act("custom-mods-set-args", vec![json!(id), json!(text)], cx);
                }
            });
            self.args = Some((input, events));
            self.args_for.clear();
        }
        if self.args_for != entry.id
            && let Some((input, _)) = &self.args
        {
            self.args_for = entry.id.clone();
            let text = entry.args.clone();
            input.update(cx, |input, cx| input.set_text(text, cx));
        }
    }

    fn engine_text(&self, entry: &ModEntry, t: &dyn Fn(&str, &[&str]) -> String) -> String {
        match self.view.engines.iter().find(|e| e.exe == entry.exe) {
            Some(e) if e.custom => t("#L_MODS_ENGINE_CUSTOM", &[&e.label]),
            Some(e) => e.label.clone(),
            None => self
                .view
                .engine_label(&entry.exe)
                .map_or_else(|| t("#L_MODS_ENGINE_NONE", &[]), str::to_owned),
        }
    }
}

/// Подпись блока героя: 12/16, заглавные, `--l-text-dim`.
fn caption(text: String, theme: &Theme) -> impl IntoElement {
    spaced(
        text.to_uppercase(),
        12.,
        16.,
        FontWeight::SEMIBOLD,
        0.08,
        theme.color("l-text-dim"),
    )
}

/// Кнопка списка движков: как `.settings-select`.
fn engine_toggle(theme: &Theme, text: String, open: bool) -> Div {
    let dim = theme.color("l-text-dim");
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(u(12.))
        .min_w(u(260.))
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
        .child(div().min_w_0().truncate().child(text))
        .child(chevron(10., if open { 180. } else { 0. }, dim))
}

/// Строка состояния мода на плитке: проблема (красным), недостающий эпизод (жёлтым), готов.
fn tile_status(entry: &ModEntry, t: &dyn Fn(&str, &[&str]) -> String) -> (String, u32) {
    if let Some(code) = &entry.problem {
        (t(problem_key(code), &[]), ERROR)
    } else if let Some(episode) = entry.missing_episodes.first() {
        let name = if episode == "ep1" {
            "Episode One"
        } else {
            "Episode Two"
        };
        (format!("⚠ {name}"), WARNING)
    } else {
        (t("#L_MODS_READY", &[]), OK)
    }
}

impl ModPage {
    fn library(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.global::<Theme>().clone();
        let lang = language(cx);
        let t = move |key: &str, args: &[&str]| hlsr_ui::tr(lang, key, args);
        let weak = cx.weak_entity();
        let busy = self.busy;
        let action = |id: &'static str, classes: &'static str, text: String, primary: bool| {
            let weak = weak.clone();
            let look = if primary {
                BtnLook::Primary
            } else {
                BtnLook::Plain
            };
            settings_btn(id, look)
                .compact()
                .disabled(busy)
                .children(icon_label(classes, text, true))
                .on_click(move |_, _, cx| {
                    weak.update(cx, |this, cx| match id {
                        "mods-add" => this.act("custom-mods-add-folder", Vec::new(), cx),
                        "mods-scan" => this.act("custom-mods-scan", Vec::new(), cx),
                        "mods-archive" => this.act("custom-mods-install-archive", Vec::new(), cx),
                        _ => {
                            if let Some(shell) = ShellContext::get(cx) {
                                shell.flows.open_mods();
                            }
                        }
                    })
                    .ok();
                })
        };
        let toolbar = div()
            .flex()
            .flex_wrap()
            .gap(u(8.))
            .child(crate::snap::target(
                "mods-archive",
                action(
                    "mods-archive",
                    "fas fa-file-archive",
                    t("#L_MODS_INSTALL_ARCHIVE", &[]),
                    true,
                ),
            ))
            .child(action(
                "mods-add",
                "fas fa-folder-plus",
                t("#L_MODS_ADD", &[]),
                false,
            ))
            .child(action(
                "mods-scan",
                "fab fa-steam",
                t("#L_MODS_SCAN", &[]),
                false,
            ))
            .child(action(
                "mods-settings",
                "fas fa-sliders-h",
                t("#L_S_MODS", &[]),
                false,
            ));
        let tiles: Vec<AnyElement> = if !self.view.loaded {
            vec![
                div()
                    .text_px(14., 22.)
                    .text_color(theme.color("l-text-dim"))
                    .child(t("#L_MODS_LOADING", &[]))
                    .into_any_element(),
            ]
        } else if self.view.mods.is_empty() {
            vec![
                div()
                    .max_w(u(560.))
                    .text_px(14., 22.)
                    .text_color(theme.color("l-text-dim"))
                    .child(t("#L_MODS_EMPTY", &[]))
                    .into_any_element(),
            ]
        } else {
            self.view
                .mods
                .iter()
                .map(|entry| {
                    let (status, color) = tile_status(entry, &t);
                    let engine = self.engine_text(entry, &t);
                    let id = entry.id.clone();
                    let target = format!("mods-tile-{}", entry.id);
                    let (name, theme) = (entry.name.clone(), theme.clone());
                    let tile = hover(
                        SharedString::from(target.clone()),
                        Timing::fast(),
                        move |h| {
                            let accent = theme.color("l-accent");
                            div()
                                .id(SharedString::from(format!("{target}-body")))
                                .w(u(232.))
                                .h(u(132.))
                                .p(u(16.))
                                .flex()
                                .flex_col()
                                .justify_between()
                                .rounded(theme.rem("l-radius"))
                                .bg(theme.color("l-surface-solid").opacity(0.85))
                                .border_1()
                                .border_color(mix(theme.color("l-border"), accent, h * 0.7))
                                .cursor_pointer()
                                .on_click(move |_, _, cx| {
                                    if let Some(shell) = ShellContext::get(cx) {
                                        shell.nav.select_mod(&id);
                                    }
                                })
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(u(10.))
                                        .child(icon(
                                            "fas fa-puzzle-piece",
                                            18.,
                                            mix(theme.color("l-text-dim"), accent, h),
                                        ))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_px(15., 22.)
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(theme.color("l-text"))
                                                .child(name),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(u(2.))
                                        .child(
                                            div()
                                                .truncate()
                                                .text_px(12., 18.)
                                                .text_color(theme.color("l-text-faint"))
                                                .child(engine),
                                        )
                                        .child(
                                            div()
                                                .truncate()
                                                .text_px(12., 18.)
                                                .text_color(rgb(color))
                                                .child(status),
                                        ),
                                )
                        },
                    );
                    crate::snap::target(format!("mods-tile-{}", entry.id), tile)
                })
                .collect()
        };

        div()
            .id("mods-library")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(theme.rem("l-hero-gap"))
                    .pt(theme.rem("l-hero-top"))
                    .px(theme.rem("l-hero-pad"))
                    .pb(u(48.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(title(
                                "mods-title".into(),
                                &t("#L_MODS_LIBRARY", &[]),
                                &theme,
                            ))
                            .child(subtitle(
                                "mods-sub".into(),
                                &t("#L_MODS_LIBRARY_SUB", &[]),
                                &theme,
                            )),
                    )
                    .child(toolbar)
                    .child(div().flex().flex_wrap().gap(u(16.)).children(tiles)),
            )
            .into_any_element()
    }

    fn mod_view(
        &mut self,
        entry: ModEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_args(&entry, window, cx);
        let theme = cx.global::<Theme>().clone();
        let lang = language(cx);
        let t = move |key: &str, args: &[&str]| hlsr_ui::tr(lang, key, args);
        let weak = cx.weak_entity();
        let busy = self.busy;

        // ---- Герой ----
        let engine_text = self.engine_text(&entry, &t);
        let mut notes: Vec<AnyElement> = Vec::new();
        if let Some(code) = &entry.problem {
            notes.push(
                div()
                    .text_px(13., 20.)
                    .text_color(rgb(ERROR))
                    .child(t(problem_key(code), &[]))
                    .into_any_element(),
            );
        }
        for episode in &entry.missing_episodes {
            notes.push(
                div()
                    .flex()
                    .gap(u(8.))
                    .text_px(13., 20.)
                    .text_color(rgb(WARNING))
                    .child(icon(
                        "fas fa-exclamation-triangle",
                        13.,
                        rgb(WARNING).into(),
                    ))
                    .child(div().flex_1().child(t(episode_key(episode), &[])))
                    .into_any_element(),
            );
        }
        if let Some(suggested) = &entry.suggested {
            let (id, exe) = (entry.id.clone(), suggested.exe.clone());
            let weak = weak.clone();
            notes.push(
                div()
                    .flex()
                    .child(
                        settings_btn("mod-suggest", BtnLook::Primary)
                            .compact()
                            .disabled(busy)
                            .on_click(move |_, _, cx| {
                                weak.update(cx, |this, cx| this.set_engine(&id, &exe, cx))
                                    .ok();
                            })
                            .children(icon_label(
                                "fas fa-magic",
                                t("#L_MODS_USE_ENGINE", &[&suggested.label]),
                                true,
                            )),
                    )
                    .into_any_element(),
            );
        }

        let mut items: Vec<Item> = self
            .view
            .engines
            .iter()
            .map(|engine| {
                let label = if engine.custom {
                    t("#L_MODS_ENGINE_CUSTOM", &[&engine.label])
                } else {
                    engine.label.clone()
                };
                let mut item = Item::new(engine.exe.clone(), label);
                item.hint = Some(engine.exe.clone().into());
                item
            })
            .collect();
        let mut pick = Item::new(PICK_EXE, t("#L_MODS_ENGINE_PICK", &[]));
        pick.separated = !items.is_empty();
        items.push(pick);
        let toggle_theme = theme.clone();
        let toggle_text = engine_text.clone();
        let engine_weak = weak.clone();
        let engine_id = entry.id.clone();
        let engine = dropdown(
            SharedString::from(format!("mod-engine-{}", entry.id)),
            items,
            Some(entry.exe.clone().into()),
        )
        .label(t("#L_MODS_ENGINE", &[]))
        .toggle(move |state| engine_toggle(&toggle_theme, toggle_text, state.open))
        .on_select(move |item, _, cx| {
            engine_weak
                .update(cx, |this, cx| this.set_engine(&engine_id, &item.id, cx))
                .ok();
        });

        // Галочки частых параметров — по строке в поле (её правит и игрок).
        let current = self
            .args
            .as_ref()
            .map(|(input, _)| input.read(cx).value())
            .unwrap_or_else(|| entry.args.clone());
        let flags = div()
            .flex()
            .flex_wrap()
            .gap_x(u(20.))
            .gap_y(u(10.))
            .children(LAUNCH_FLAGS.iter().map(|(flag, key)| {
                let weak = weak.clone();
                let on = has_flag(&current, flag);
                crate::snap::target(
                    format!("mod-flag{flag}"),
                    check(
                        SharedString::from(format!("mod-flag{flag}")),
                        if on { Checked::On } else { Checked::Off },
                        CheckLook::Box,
                    )
                    .disabled(busy)
                    .gap(u(8.))
                    .text_px(13., 20.)
                    .text_color(theme.color("l-text"))
                    .on_change(move |_, _, cx| {
                        weak.update(cx, |this, cx| this.toggle_flag(flag, cx)).ok();
                    })
                    .child(t(key, &[])),
                )
            }));

        let block = |label: String, body: AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap(u(8.))
                .child(caption(label, &theme))
                .child(body)
        };
        let folder_text = div()
            .text_px(13., 20.)
            .text_color(theme.color("l-text-faint"))
            .child(entry.mod_dir.clone())
            .into_any_element();
        let args = div()
            .flex()
            .flex_col()
            .gap(u(12.))
            .child(flags)
            .child(
                div()
                    .w_full()
                    .children(self.args.as_ref().map(|(input, _)| input.clone())),
            )
            .into_any_element();
        let back = {
            let theme = theme.clone();
            let text = t("#L_MODS_BACK", &[]);
            crate::snap::target(
                "mod-back",
                hover("mod-back", Timing::fast(), move |h| {
                    div()
                        .id("mod-back-body")
                        .flex()
                        .items_center()
                        .gap(u(8.))
                        .cursor_pointer()
                        .text_px(13., 20.)
                        .text_color(mix(theme.color("l-text-dim"), theme.color("l-text"), h))
                        .on_click(|_, _, cx| {
                            if let Some(shell) = ShellContext::get(cx) {
                                shell.nav.select_mod("");
                            }
                        })
                        .child(icon("fas fa-arrow-left", 12., theme.color("l-text-dim")))
                        .child(text.clone())
                }),
            )
        };

        let hero = div()
            .id("mod-hero")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(theme.rem("l-hero-gap"))
                    .pt(theme.rem("l-hero-top"))
                    .px(theme.rem("l-hero-pad"))
                    .pb(u(24.))
                    .max_w(u(760.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(u(10.))
                            .child(div().flex().child(back))
                            .child(title(
                                format!("mod-title:{}", entry.id).into(),
                                &entry.name,
                                &theme,
                            ))
                            .child(subtitle(
                                format!("mod-sub:{}", entry.id).into(),
                                &engine_text,
                                &theme,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(u(18.))
                            .children(notes)
                            .child(block(t("#L_MODS_FOLDER_LABEL", &[]), folder_text))
                            .child(block(
                                t("#L_MODS_ENGINE", &[]),
                                div().flex().child(engine).into_any_element(),
                            ))
                            .child(block(t("#L_MODS_ARGS", &[]), args)),
                    ),
            );

        // ---- Нижняя панель ----
        let on = |act: fn(&mut ModPage, &str, &mut Context<ModPage>)| {
            let weak = weak.clone();
            let id = entry.id.clone();
            move |cx: &mut App| {
                weak.update(cx, |this, cx| act(this, &id, cx)).ok();
            }
        };
        let play_text = theme.color("btn-ready-text");
        let play_font = theme.len("l-play-font");
        let launchable = entry.problem.is_none() && !busy;
        let play_weak = weak.clone();
        let play_id = entry.id.clone();
        let play = button("mod-play", Look::Tone(Tone::Ready))
            .disabled(!launchable)
            .min_w(theme.rem("l-play-w"))
            .h(theme.rem("l-play-h"))
            .px(u(28.))
            .on_click(move |_, _, cx| {
                let id = play_id.clone();
                play_weak
                    .update(cx, |this, cx| {
                        this.act("custom-mods-launch", vec![json!(id)], cx)
                    })
                    .ok();
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(12.))
                    .child(icon("fas fa-play", play_font, play_text))
                    .child(spaced(
                        t("#L_MODS_PLAY", &[]).to_uppercase(),
                        play_font,
                        24.,
                        FontWeight::BOLD,
                        0.12,
                        play_text,
                    )),
            );
        let play = div().when(!launchable, |el| el.opacity(0.8)).child(play);
        let bar = div()
            .relative()
            .flex()
            .items_center()
            .gap(theme.rem("l-control-gap"))
            .h(theme.rem("l-bottombar-h"))
            .pl(theme.rem("l-hero-pad"))
            .pr(theme.rem("l-edge-pad"))
            .child(div().flex_1())
            .child(side_button(
                "mod-all",
                "fas fa-th-large",
                t("#L_MODS_BACK", &[]),
                false,
                &theme,
                |cx| {
                    if let Some(shell) = ShellContext::get(cx) {
                        shell.nav.select_mod("");
                    }
                },
            ))
            .child(side_button(
                "mod-folder",
                "fas fa-folder-open",
                t("#L_MODS_FOLDER", &[]),
                busy,
                &theme,
                on(|this, id, cx| this.act("custom-mods-open-folder", vec![json!(id)], cx)),
            ))
            .child(side_button(
                "mod-remove",
                "fas fa-trash",
                t("#L_MODS_REMOVE", &[]),
                busy,
                &theme,
                on(|this, id, cx| this.act("custom-mods-remove", vec![json!(id)], cx)),
            ))
            .child(crate::snap::target("mod-play", play));
        let dock = div()
            .w_full()
            .relative()
            .flex_none()
            .child(feather(&theme, viewport(window).0))
            .child(bar);

        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(hero)
            .child(dock)
            .into_any_element()
    }
}

impl Render for ModPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::live::rendered(cx.entity_id());
        let selected = ShellContext::get(cx)
            .map(|shell| shell.nav.selected_mod())
            .unwrap_or_default();
        // Мод не выбран или его больше нет — библиотека.
        match self.view.mods.iter().find(|m| m.id == selected).cloned() {
            Some(entry) => self.mod_view(entry, window, cx),
            None => self.library(cx),
        }
    }
}
