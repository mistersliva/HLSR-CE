//! Колонка значков (`GameSidebar.vue` 0.4.9): игры (логотип маской SVG цветом состояния, онлайн
//! или «скоро»), линия, инструменты (LiveSplit), свой пункт «Демо» (`fa-film`), внизу —
//! переключатель колонки, Discord, папка игр, список изменений, настройки лаунчера.
//!
//! Выдвигается 80 → 240 (`--l-rail-w` → `--l-rail-open-w`) за `--l-dur-swipe` `--l-ease-swipe`:
//! колонка стоит в строке рамки, страница сдвигается и сужается во всех разделах. Подписи
//! проявляются с задержкой .14 с (`--l-dur` `--l-ease`) и гаснут сразу (`--l-dur-fast`
//! `--l-ease-in`). Под колонкой — матовое стекло, как `backdrop-filter: blur(--l-rail-blur)`
//! ([`crate::glass::RailGlass`]: слой оболочки под колонкой — кадр фона под её коробкой,
//! размытый вместе с затемнением, и тень выдвинутой колонки), сверху заливка
//! `--l-rail-fill` → `--l-rail-open-fill` (решение владельца 6: как в Electron, со стеклом GL).
//!
//! Подсказка `rail-tip` — только у свёрнутой колонки и без окон поверх (`tipsEnabled`): справа от
//! значка, по его центру, ширина кратна 4 (`v-grid-width`), появление и уход `--l-fast`.
//!
//! Логика — [`NavController`] (`hlsr-ui`): данные колонки, выбор игры, инструмента, «Демо»,
//! кнопки внизу, `toggleRail`.
//!
//! HLSR-CE: после «Демо» — линия и «Библиотека модов» (свои Source-моды плитками, вкладка `mods`;
//! в Linux пункта нет).

use super::root::{ShellContext, ShellRoot};
use crate::icons::fa_glyph;
use crate::kit::button::{child_id, focus_handle, focus_visible, outline};
use crate::layer::Layers;
use crate::motion::{AnimGate, Timing, hover, mix, tween};
use crate::paint::{ring, shadow};
use crate::scale::{u, zoom, zpx};
use crate::text::{BODY_SPACING, RUBIK, TextExt, spaced};
use crate::theme::Theme;
use gpui::{
    Bounds, Canvas, ClickEvent, Context, CursorStyle, ElementId, FontWeight, Hsla, LineCap,
    LineJoin, PathBuilder, PathStyle, Pixels, Render, Role, ScrollHandle, SharedString,
    StrokeOptions, TextRun, Window, black, canvas, deferred, div, font, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgb, svg,
};
use hlsr_ui::controllers::nav::NavController;
use hlsr_ui::services::Services;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

/// Цвет онлайна (`.is-online`).
const ONLINE: u32 = 0x5fd17f;
/// Подписи колонки ждут, пока она почти раскроется (`transition-delay: .14s`).
const LABEL_DELAY_MS: f64 = 140.;
/// `deferred` подсказки: над страницей, под окнами (`z-index: 30` у колонки с `z-index: 2`).
const TIP_PRIORITY: usize = 2;

/// CSS `transition` числа с задержкой: откуда, куда, с какого мгновения.
#[derive(Clone, Copy)]
struct Move {
    from: f32,
    to: f32,
    since: Instant,
    delay: f64,
    timing: Timing,
}

impl Move {
    fn rest(value: f32) -> Move {
        Move {
            from: value,
            to: value,
            since: Instant::now(),
            delay: 0.,
            timing: Timing::new(0., [0., 0., 1., 1.]),
        }
    }

    fn value(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.since).as_secs_f64() * 1000. - self.delay;
        if elapsed <= 0. {
            return self.from;
        }
        if self.timing.ms <= 0. || elapsed >= self.timing.ms {
            return self.to;
        }
        let eased = self.timing.curve.solve(elapsed / self.timing.ms) as f32;
        self.from + (self.to - self.from) * eased
    }

    /// Новая цель — от текущего значения. `still` (`.no-anim`) — сразу.
    fn retarget(&mut self, to: f32, delay: f64, timing: Timing, still: bool, now: Instant) {
        if self.to == to {
            return;
        }
        *self = if still {
            Move::rest(to)
        } else {
            Move {
                from: self.value(now),
                to,
                since: now,
                delay,
                timing,
            }
        };
    }

    fn moving(&self, now: Instant) -> bool {
        self.value(now) != self.to
    }
}

/// Подсказка: заголовок, подпись и её тон.
#[derive(Clone, PartialEq)]
struct Tip {
    title: String,
    note: Option<(String, bool)>,
}

/// Показанная подсказка: чья, что и где (центр значка от верха колонки, CSS px).
#[derive(Clone)]
struct ShownTip {
    owner: SharedString,
    tip: Tip,
    center: f32,
}

/// Что делает щелчок.
#[derive(Clone)]
enum Action {
    Game(String),
    Tool(String),
    Demo,
    Toggle,
    Footer(&'static str),
    /// HLSR-CE: библиотека своих модов.
    Mods,
}

/// Значок пункта списка.
enum Icon {
    /// Логотип маской (`--l-rail-logo`): `logos/…svg`.
    Logo(SharedString),
    /// Значок Font Awesome: классы, кегль, ширина и высота строки.
    Glyph(String, f32, f32),
}

/// Что гасит подсказку (`watch`: selectedId, activeTab, selectedToolId, tipsEnabled, expanded).
#[derive(Clone, PartialEq, Default)]
struct Watch {
    game: String,
    tab: String,
    tool: String,
    tips: bool,
    open: bool,
}

pub struct GameSidebar {
    scroll: ScrollHandle,
    /// Выдвижение: ширина, заливка, тень, линии, стрелка (0 — свёрнута, 1 — выдвинута).
    swipe: Option<Move>,
    /// Мгновение кадра, с которым оболочка взяла выдвижение ([`GameSidebar::swipe_now`]): колонка
    /// рисуется с ним же.
    shell_now: Option<Instant>,
    /// Прозрачность и сдвиг подписей.
    labels: Option<Move>,
    tip: Option<ShownTip>,
    /// Последняя показанная подсказка: живёт, пока гаснет.
    tip_last: Option<ShownTip>,
    /// Появление и уход подсказки (`--l-fast`).
    tip_fade: Move,
    /// Чей контур `:focus-visible` показан (подсказка по фокусу).
    focused: Option<SharedString>,
    watch: Option<Watch>,
    /// Центры значков (px окна по вертикали) и верх колонки — с прошлой раскладки.
    centers: Rc<RefCell<HashMap<SharedString, f32>>>,
    top: Rc<Cell<f32>>,
}

impl GameSidebar {
    pub fn new(_services: &Services, _cx: &mut Context<GameSidebar>) -> GameSidebar {
        GameSidebar {
            scroll: ScrollHandle::new(),
            swipe: None,
            shell_now: None,
            labels: None,
            tip: None,
            tip_last: None,
            tip_fade: Move::rest(0.),
            focused: None,
            watch: None,
            centers: Rc::default(),
            top: Rc::default(),
        }
    }

    /// `selectItem` / `toggle`: подсказка гаснет, дальше — навигация.
    fn act(&mut self, action: &Action, cx: &mut Context<Self>) {
        self.hide_all(cx);
        let Some((nav, flows)) =
            ShellContext::get(cx).map(|context| (context.nav.clone(), context.flows.clone()))
        else {
            return;
        };
        match action {
            Action::Game(id) => nav.select_game(id),
            Action::Tool(id) => nav.select_tool(id),
            // `onSelectDemo`: без FFmpeg — сначала окно FFmpeg (G3).
            Action::Demo => {
                let in_demo = nav.section() == "games" && nav.active_tab() == "demo";
                flows.on_select_demo(in_demo, move || nav.select_demo());
            }
            Action::Toggle => nav.toggle_rail(),
            Action::Footer(id) => nav.footer_tool(id),
            Action::Mods => nav.select_mod(""),
        }
    }

    /// `showTip`: в выдвинутой колонке и под окнами подсказок нет.
    fn show_tip(&mut self, owner: SharedString, tip: Tip, zoom: f32, cx: &mut Context<Self>) {
        let allowed = ShellContext::get(cx).is_some_and(|context| !context.nav.rail_open())
            && !ShellRoot::overlay_open(cx);
        let Some(&center) = self.centers.borrow().get(&owner) else {
            return;
        };
        if !allowed {
            return;
        }
        let shown = ShownTip {
            owner,
            tip,
            center: (center - self.top.get()) / zoom,
        };
        self.tip_last = Some(shown.clone());
        self.tip = Some(shown);
        cx.notify();
    }

    /// Уход с значка: гасит только свою подсказку (наведение на соседа могло прийти раньше).
    fn hide_tip(&mut self, owner: &str, cx: &mut Context<Self>) {
        if self.tip.as_ref().is_some_and(|tip| tip.owner == owner) {
            self.tip = None;
            cx.notify();
        }
    }

    fn hide_all(&mut self, cx: &mut Context<Self>) {
        if self.tip.take().is_some() {
            cx.notify();
        }
    }

    /// Выдвижение сейчас (0 — свёрнута, 1 — выдвинута); цель `open` — у навигации. Оболочка
    /// берёт его до отрисовки колонки: место закэшированной колонки в строке ([`crate::cache`])
    /// и слой стекла под ней — те же, что у самой колонки в этом кадре: она рисуется с тем же `now`.
    pub fn swipe_now(&mut self, open: bool, theme: &Theme, still: bool, now: Instant) -> f32 {
        self.shell_now = Some(now);
        self.swipe_at(open, theme, still, now)
    }

    fn swipe_at(&mut self, open: bool, theme: &Theme, still: bool, now: Instant) -> f32 {
        let target = if open { 1. } else { 0. };
        let timing = Timing::new(
            theme.duration("l-dur-swipe").as_secs_f64() * 1000.,
            [0.32, 0.72, 0., 1.],
        );
        let swipe = self.swipe.get_or_insert_with(|| Move::rest(target));
        swipe.retarget(target, 0., timing, still, now);
        swipe.value(now)
    }
}

/// Ширина строки Rubik в CSS px (`size` и разрядка `spacing` — CSS px).
fn text_width(window: &Window, text: &str, size: f32, weight: FontWeight, spacing: f32) -> f32 {
    let zoom = zoom(window);
    let run = TextRun {
        len: text.len(),
        font: gpui::Font {
            weight,
            ..font(RUBIK)
        },
        color: black(),
        letter_spacing: zpx(zoom, spacing),
        ..Default::default()
    };
    let line = window
        .text_system()
        .layout_line(text, zpx(zoom, size), &[run], None);
    f32::from(line.width) / zoom
}

/// `v-grid-width`: вверх до кратной 4 (допуск — сотая шага).
fn grid_width(width: f32) -> f32 {
    (width / 4. - 0.01).ceil() * 4.
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Круглая точка (`round-dot`), свечение `drop-shadow(0 0 glow)` — тенью круга.
fn dot(size: f32, color: Hsla, glow: f32, zoom: f32) -> gpui::Div {
    div()
        .flex_none()
        .size(u(size))
        .rounded_full()
        .bg(color)
        .when(glow > 0., |dot| {
            dot.shadow(vec![shadow(zoom, 0., 0., glow, 0., color)])
        })
}

/// Значок переключателя колонки (свой SVG 0.4.9, viewBox 24): рамка панели `rx 3`, линия
/// `x 9`, стрелка `M13.5 9.5 16 12l-2.5 2.5` поворачивается на 180° вокруг своей середины по
/// выдвижению.
fn collapse_icon(color: Hsla, turn: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let unit = bounds.size.width / 24.;
            let at = |x: f32, y: f32| point(bounds.left() + unit * x, bounds.top() + unit * y);
            let stroke = |cap, join| {
                PathBuilder::default().with_style(PathStyle::Stroke(StrokeOptions {
                    line_width: f32::from(unit * 2.),
                    line_cap: cap,
                    line_join: join,
                    ..StrokeOptions::default()
                }))
            };
            let radius = point(unit * 3., unit * 3.);
            let mut frame = stroke(LineCap::Butt, LineJoin::Miter);
            frame.move_to(at(6., 4.));
            frame.line_to(at(18., 4.));
            frame.arc_to(radius, Pixels::ZERO, false, true, at(21., 7.));
            frame.line_to(at(21., 17.));
            frame.arc_to(radius, Pixels::ZERO, false, true, at(18., 20.));
            frame.line_to(at(6., 20.));
            frame.arc_to(radius, Pixels::ZERO, false, true, at(3., 17.));
            frame.line_to(at(3., 7.));
            frame.arc_to(radius, Pixels::ZERO, false, true, at(6., 4.));
            frame.close();
            frame.move_to(at(9., 4.));
            frame.line_to(at(9., 20.));
            if let Ok(path) = frame.build() {
                window.paint_path(path, color);
            }
            // transform-box: fill-box — середина рамки стрелки (14.75, 12).
            let (sin, cos) = (std::f32::consts::PI * turn).sin_cos();
            let turned = |x: f32, y: f32| {
                let (dx, dy) = (x - 14.75, y - 12.);
                at(14.75 + dx * cos - dy * sin, 12. + dx * sin + dy * cos)
            };
            let mut chevron = stroke(LineCap::Round, LineJoin::Round);
            chevron.move_to(turned(13.5, 9.5));
            chevron.line_to(turned(16., 12.));
            chevron.line_to(turned(13.5, 14.5));
            if let Ok(path) = chevron.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size(u(24.))
    .flex_none()
}

/// Хвостик подсказки (`::before`): квадрат 10 px с рамкой слева и снизу (11 × 11 с рамкой),
/// повёрнутый на 45°, середина на 0.5 px левее края подсказки.
fn tip_tail(fill: Hsla, border: Hsla) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let unit = bounds.size.width / 18.;
            let cy = bounds.size.height / 2.;
            let half = 5.5 * std::f32::consts::SQRT_2;
            let at = |x: f32, y: Pixels| point(bounds.left() + unit * (x + 9.), bounds.top() + y);
            let (top, left, right, bottom) = (
                at(-0.5, cy - unit * half),
                at(-0.5 - half, cy),
                at(-0.5 + half, cy),
                at(-0.5, cy + unit * half),
            );
            let mut body = PathBuilder::fill();
            body.add_polygon(&[top, right, bottom, left], true);
            if let Ok(path) = body.build() {
                window.paint_path(path, fill);
            }
            // Рамка 1 px внутри квадрата: линия сдвинута внутрь на полпикселя.
            let inset = point(unit * (0.5 * std::f32::consts::SQRT_2), Pixels::ZERO);
            let mut edge = PathBuilder::stroke(unit);
            edge.move_to(top + inset);
            edge.line_to(left + inset);
            edge.line_to(bottom + inset);
            if let Ok(path) = edge.build() {
                window.paint_path(path, border);
            }
        },
    )
}

/// Записать центр значка (px окна) после раскладки.
fn center_probe(
    owner: SharedString,
    centers: Rc<RefCell<HashMap<SharedString, f32>>>,
) -> impl IntoElement {
    canvas(
        move |bounds: Bounds<Pixels>, _, _| {
            centers
                .borrow_mut()
                .insert(owner, f32::from(bounds.center().y));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

/// Линия `.game-rail__rule`: 40 px по центру, у выдвинутой — во всю ширину; места не занимает.
fn rule(width: f32, full: f32, t: f32, color: Hsla) -> gpui::Div {
    let w = lerp(width, full, t);
    div().flex_none().h(px(0.)).relative().child(
        div()
            .absolute()
            .top_0()
            .left(u((full - w) / 2.))
            .w(u(w))
            .h(px(1.))
            .bg(color),
    )
}

impl Render for GameSidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::live::rendered(cx.entity_id());
        let theme = cx.global::<Theme>().clone();
        let Some(context) = ShellContext::get(cx).cloned() else {
            return div()
                .id("game-rail")
                .flex_none()
                .h_full()
                .w(u(theme.len("l-rail-w")));
        };
        let nav: NavController = context.nav.clone();
        let tr = |key: &str| context.shell.tr(key, &[]);
        // Мгновение оболочки: со своим конец выдвижения у колонки и у её места мог прийтись на
        // разные кадры, и место осталось бы недовыдвинутым (следующего кадра колонка не просит).
        // Часы исполнителя, как у оболочки (`swipe_now`) и остального движения.
        let now = self
            .shell_now
            .take()
            .unwrap_or_else(|| cx.background_executor().now());
        let still = AnimGate::still(cx).any();
        let zoom = zoom(window);
        let radius = Layers::radius(cx);

        // ---- Выдвижение и подписи ----------------------------------------------------
        let open = nav.rail_open();
        let target = if open { 1. } else { 0. };
        let t = self.swipe_at(open, &theme, still, now);
        let labels = self.labels.get_or_insert_with(|| Move::rest(target));
        if open {
            labels.retarget(1., LABEL_DELAY_MS, Timing::med(), still, now);
        } else {
            let fast = theme.duration("l-dur-fast").as_secs_f64() * 1000.;
            labels.retarget(0., 0., Timing::new(fast, [0.55, 0., 0.75, 0.2]), still, now);
        }
        let label_t = labels.value(now);
        let labels_moving = labels.moving(now);
        let swiping = self.swipe.is_some_and(|swipe| swipe.moving(now));
        if swiping || labels_moving {
            crate::motion::monitor_frame(window);
        }

        // ---- Подсказка: что её гасит ------------------------------------------------------
        let section = nav.section();
        let tab = if section == "games" {
            nav.active_tab()
        } else {
            String::new()
        };
        let watch = Watch {
            game: nav.selected_game(),
            tab: tab.clone(),
            tool: nav.selected_tool(),
            tips: !ShellRoot::overlay_open(cx),
            open,
        };
        let games = nav.games();
        if self.watch.as_ref() != Some(&watch) {
            let revealed = self.watch.as_ref().map(|old| old.game.clone());
            if self.watch.is_some() {
                self.tip = None;
            }
            // revealActiveCard: при показе и при смене игры.
            if revealed.as_ref() != Some(&watch.game)
                && let Some(index) = games.iter().position(|game| game.id == watch.game)
            {
                self.scroll.scroll_to_item(index);
            }
            self.watch = Some(watch.clone());
        }

        // ---- Размеры и цвета -------------------------------------------------------------
        let closed_w = theme.len("l-rail-w");
        let width = lerp(closed_w, theme.len("l-rail-open-w"), t);
        let item_h = theme.len("l-rail-item");
        let logo = theme.len("l-rail-logo");
        let gap = theme.len("l-rail-gap");
        let tool_h = theme.len("l-rail-tool");
        let side = (closed_w - item_h) / 2.;
        let foot = (closed_w - tool_h) / 2.;
        let radius_l = theme.len("l-radius");
        let radius_sm = theme.len("l-radius-sm");
        let (text, text_dim, text_faint) = (
            theme.color("l-text"),
            theme.color("l-text-dim"),
            theme.color("l-text-faint"),
        );
        let accent = theme.color("l-accent");
        let border = theme.color("l-border");
        let rail_game = theme.color("l-rail-game");
        let rail_icon = theme.color("l-rail-icon");
        let online: Hsla = rgb(ONLINE).into();
        let rule_color = theme.color("l-border-strong").opacity(0.6);
        let white = |a: f32| gpui::white().opacity(a);

        // ---- Пункты списка --------------------------------------------------------------
        struct Entry {
            owner: SharedString,
            target: String,
            label: String,
            tip: Tip,
            icon: Icon,
            available: bool,
            active: bool,
            action: Action,
        }
        let mut entries: Vec<Entry> = games
            .iter()
            .map(|game| {
                let note = if !game.available {
                    Some((tr("#L_GAME_COMING_SOON"), false))
                } else {
                    match game.online_state.as_str() {
                        "online" => Some((
                            context
                                .shell
                                .tr("#L_RAIL_ONLINE", &[&game.online.to_string()]),
                            true,
                        )),
                        "unknown" => Some((tr("#L_ONLINE_UNKNOWN"), false)),
                        _ => None,
                    }
                };
                let label = match &note {
                    Some((note, _)) => format!("{} — {note}", game.title),
                    None => game.title.clone(),
                };
                Entry {
                    owner: format!("game-{}", game.id).into(),
                    target: format!("rail-game-{}", game.id),
                    label,
                    tip: Tip {
                        title: game.title.clone(),
                        note,
                    },
                    icon: Icon::Logo(if game.logo.is_empty() {
                        "glyphs/hl.svg".into()
                    } else {
                        format!("logos/{}.svg", game.logo).into()
                    }),
                    available: game.available,
                    active: tab == "games" && watch.game == game.id,
                    action: Action::Game(game.id.clone()),
                }
            })
            .collect();
        let game_count = entries.len();
        entries.extend(nav.tools().into_iter().map(|tool| Entry {
            owner: format!("tool-{}", tool.id).into(),
            target: format!("rail-tool-{}", tool.id),
            label: tool.title.clone(),
            tip: Tip {
                title: tool.title.clone(),
                note: None,
            },
            icon: if tool.logo.is_empty() {
                Icon::Glyph(tool.icon.clone(), 20., 24.)
            } else {
                Icon::Logo(format!("logos/{}.svg", tool.logo).into())
            },
            available: true,
            active: tab == "tools" && watch.tool == tool.id,
            action: Action::Tool(tool.id.clone()),
        }));
        let demo_title = tr("#L_DEMO_TITLE");
        entries.push(Entry {
            owner: "demo".into(),
            target: "rail-demo".into(),
            label: demo_title.clone(),
            tip: Tip {
                title: demo_title,
                note: None,
            },
            icon: Icon::Glyph("fas fa-film".into(), 26., logo),
            available: true,
            active: tab == "demo",
            action: Action::Demo,
        });
        // HLSR-CE: «Библиотека модов» — после линии.
        let mods_at = entries.len();
        if !cfg!(target_os = "linux") {
            let title = tr("#L_MODS_LIBRARY");
            entries.push(Entry {
                owner: "mods".into(),
                target: "rail-mods".into(),
                label: title.clone(),
                tip: Tip { title, note: None },
                icon: Icon::Glyph("fas fa-puzzle-piece".into(), 22., logo),
                available: true,
                active: tab == "mods",
                action: Action::Mods,
            });
        }

        let mut focused = None;
        let mut list = div()
            .id("game-rail-list")
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .gap(u(gap))
            .py(u(gap))
            .px(u(side))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll(cx.listener(|this, _, _, cx| this.hide_all(cx)));
        for (index, entry) in entries.into_iter().enumerate() {
            if index == game_count || (index == mods_at && index > game_count) {
                // Линия перед инструментами: всегда, и без инструментов («Демо» после неё).
                list = list.child(rule(40., width - 2. * side, t, rule_color));
            }
            let id = ElementId::Name(entry.target.clone().into());
            let focus = focus_handle(&id, window, cx);
            let ring_on = focus_visible(&focus, window);
            if ring_on {
                focused = Some((entry.owner.clone(), entry.tip.clone()));
            }
            let action = entry.action.clone();
            let on_click = cx.listener(move |this, _: &ClickEvent, _, cx| this.act(&action, cx));
            let (owner, tip) = (entry.owner.clone(), entry.tip.clone());
            let on_hover = cx.listener(move |this, hovered: &bool, window, cx| {
                if *hovered {
                    this.show_tip(owner.clone(), tip.clone(), crate::scale::zoom(window), cx);
                } else {
                    this.hide_tip(&owner, cx);
                }
            });
            let probe = center_probe(entry.owner.clone(), self.centers.clone());
            let Entry {
                target,
                label,
                tip,
                icon,
                available,
                active,
                ..
            } = entry;
            let inner = id.clone();
            let picked = child_id(&id, "active");
            // `.rail-item { transition: color, background-color, box-shadow var(--l-fast) }`:
            // выбранная красится в акцент переходом, как и наведение.
            let item = hover(id, Timing::fast(), move |h| {
                tween(
                    picked,
                    f32::from(u8::from(active)),
                    Timing::fast(),
                    move |a| {
                        let lit = if available { h } else { 0. };
                        let ink = mix(mix(rail_game, text, lit), accent, a);
                        // Выдвинутая: строка — карточкой (наведение — фон и контур, выбранная — акцент).
                        let bg = if open {
                            white(if available { 0.05 } else { 0.02 } * h * (1. - a))
                        } else {
                            white(0.)
                        };
                        let ring_color = if open {
                            mix(border.opacity(h), accent.opacity(0.4), a)
                        } else {
                            border.opacity(0.)
                        };
                        let icon_el = match icon {
                            Icon::Logo(path) => svg()
                                .path(path)
                                .flex_none()
                                .size(u(logo))
                                .text_color(ink)
                                .into_any_element(),
                            Icon::Glyph(classes, size, side) => div()
                                .flex_none()
                                .w(u(side))
                                .h(u(side))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(ink)
                                .child(fa_glyph(&classes, size).line_height(u(side)))
                                .into_any_element(),
                        };
                        let note_el = tip.note.map(|(note, is_online)| {
                            let color = if is_online { online } else { text_faint };
                            div()
                                .flex()
                                .items_center()
                                .gap(u(6.))
                                .text_color(color)
                                .text_px(11.5, 16.)
                                .when(is_online, |row| row.child(dot(6., color, 3., zoom)))
                                .child(div().min_w_0().truncate().child(note))
                        });
                        div()
                            .id(inner)
                            .role(Role::Button)
                            .button_text()
                            .aria_label(SharedString::from(label))
                            .relative()
                            .flex_none()
                            .w_full()
                            .h(u(item_h))
                            .rounded(u(radius_l))
                            .flex()
                            .items_center()
                            .bg(bg)
                            .when(open && active, |el| {
                                el.bg(linear_gradient(
                                    90.,
                                    linear_color_stop(accent.alpha(0.18), 0.),
                                    linear_color_stop(accent.alpha(0.06), 1.),
                                ))
                            })
                            .shadow(vec![ring(zoom, 1., ring_color)])
                            .cursor(if !available {
                                CursorStyle::OperationNotAllowed
                            } else if active {
                                CursorStyle::Arrow
                            } else {
                                CursorStyle::PointingHand
                            })
                            .track_focus(&focus)
                            .on_click(on_click)
                            .on_hover(on_hover)
                            .child(probe)
                            .child(
                                div()
                                    .flex_none()
                                    .size(u(item_h))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(!available, |el| el.opacity(0.5))
                                    .child(icon_el),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_hidden()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .relative()
                                            .left(u((label_t - 1.) * 8.))
                                            .w_full()
                                            .pr(u(12.))
                                            .opacity(label_t)
                                            .flex()
                                            .flex_col()
                                            .gap(u(2.))
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_px(13., 18.)
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(text)
                                                    .when(!available, |el| el.opacity(0.5))
                                                    .child(tip.title),
                                            )
                                            .children(note_el),
                                    ),
                            )
                            .when(ring_on, |el| {
                                el.child(outline(zoom, radius_l, 0., 2., accent))
                            })
                    },
                )
            });
            list = list.child(crate::snap::target(target, item));
        }

        // ---- Низ: переключатель и кнопки ----------------------------------------------
        let toggle_title = tr(if open {
            "#L_RAIL_COLLAPSE"
        } else {
            "#L_RAIL_EXPAND"
        });
        enum FootIcon {
            Toggle,
            Svg(&'static str),
            Fa(&'static str),
        }
        let footer_items = [
            ("toggle", toggle_title, FootIcon::Toggle, Action::Toggle),
            (
                "discord",
                tr("#L_DISCORD"),
                FootIcon::Svg("glyphs/discord.svg"),
                Action::Footer("discord"),
            ),
            (
                "folder",
                tr("#L_GAME_FOLDER"),
                FootIcon::Svg("glyphs/folder.svg"),
                Action::Footer("folder"),
            ),
            (
                "changelog",
                tr("#L_CHANGELOG"),
                FootIcon::Svg("glyphs/changelog.svg"),
                Action::Footer("changelog"),
            ),
            (
                "settings",
                tr("#L_LAUNCHER_SETTINGS"),
                FootIcon::Fa("fas fa-cog"),
                Action::Footer("settings"),
            ),
        ];
        let icon_pad = (tool_h - 24.) / 2.;
        let mut footer = div()
            .flex_none()
            .w_full()
            .flex()
            .flex_col()
            .gap(u(theme.len("l-rail-tool-gap")))
            .pt(u(gap))
            .px(u(foot))
            .pb(u(theme.len("l-rail-bottom")));
        for (name, title, icon, action) in footer_items {
            let target = format!("rail-{name}");
            let id = ElementId::Name(target.clone().into());
            let focus = focus_handle(&id, window, cx);
            let ring_on = focus_visible(&focus, window);
            let owner: SharedString = name.into();
            let tip = Tip {
                title: title.clone(),
                note: None,
            };
            if ring_on {
                focused = Some((owner.clone(), tip.clone()));
            }
            let on_click = cx.listener(move |this, _: &ClickEvent, _, cx| this.act(&action, cx));
            let on_hover = {
                let owner = owner.clone();
                cx.listener(move |this, hovered: &bool, window, cx| {
                    if *hovered {
                        this.show_tip(owner.clone(), tip.clone(), crate::scale::zoom(window), cx);
                    } else {
                        this.hide_tip(&owner, cx);
                    }
                })
            };
            let probe = center_probe(owner, self.centers.clone());
            let inner = id.clone();
            let button = hover(id, Timing::fast(), move |h| {
                let ink = mix(rail_icon, text, h);
                let icon_el = match icon {
                    FootIcon::Toggle => collapse_icon(ink, t).into_any_element(),
                    FootIcon::Svg(path) => svg()
                        .path(path)
                        .flex_none()
                        .size(u(24.))
                        .text_color(ink)
                        .into_any_element(),
                    FootIcon::Fa(classes) => div()
                        .flex_none()
                        .size(u(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(fa_glyph(classes, 22.).line_height(u(24.)))
                        .into_any_element(),
                };
                div()
                    .id(inner)
                    .role(Role::Button)
                    .button_text()
                    .aria_label(SharedString::from(title.clone()))
                    .relative()
                    .flex_none()
                    .w_full()
                    .h(u(tool_h))
                    .pl(u(icon_pad))
                    .rounded(u(radius_sm))
                    .flex()
                    .items_center()
                    .gap(u(icon_pad + 4.))
                    .bg(white(0.06 * h))
                    .active(|el| el.bg(white(0.1)))
                    .text_color(ink)
                    .cursor_pointer()
                    .track_focus(&focus)
                    .on_click(on_click)
                    .on_hover(on_hover)
                    .child(probe)
                    .child(icon_el)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .relative()
                                    .left(u((label_t - 1.) * 8.))
                                    .w_full()
                                    .opacity(label_t)
                                    .truncate()
                                    .text_px(13., 20.)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(title),
                            ),
                    )
                    .when(ring_on, |el| {
                        el.child(outline(zoom, radius_sm, 0., 2., accent))
                    })
            });
            footer = footer.child(crate::snap::target(target, button));
            if name == "toggle" {
                footer = footer.child(rule(40., width - 2. * foot, t, rule_color));
            }
        }

        // Подсказка по фокусу с клавиатуры (`showFocusTip`) и её уход по `blur`.
        let focused_owner = focused.as_ref().map(|(owner, _)| owner.clone());
        if focused_owner != self.focused {
            if let Some(old) = self.focused.take() {
                self.hide_tip(&old, cx);
            }
            if let Some((owner, tip)) = focused {
                self.show_tip(owner, tip, zoom, cx);
            }
            self.focused = focused_owner;
        }

        // ---- Заливка над стеклом (слой стекла под колонкой — [`crate::glass::RailGlass`]) ----
        let fill = mix(
            theme.color("l-rail-fill"),
            theme.color("l-rail-open-fill"),
            t,
        );
        let glass = div()
            .absolute()
            .inset_0()
            .rounded_tl(radius)
            .rounded_bl(radius)
            .bg(fill);

        // ---- Подсказка ------------------------------------------------------------------
        let shown_now = self.tip.is_some();
        self.tip_fade.retarget(
            if shown_now { 1. } else { 0. },
            0.,
            Timing::fast(),
            still,
            now,
        );
        let v = self.tip_fade.value(now);
        if self.tip_fade.moving(now) {
            crate::motion::monitor_frame(window);
        } else if !shown_now {
            // Погасла — дерево подсказки и замер текста больше не строятся.
            self.tip_last = None;
        }
        let tip_el = self.tip_last.clone().map(|shown| {
            let has_note = shown.tip.note.is_some();
            let height = if has_note { 64. } else { 40. };
            let title = shown.tip.title.to_uppercase();
            let mut natural = text_width(window, &title, 13., FontWeight::BOLD, 0.04 * 13.);
            if let Some((note, is_online)) = &shown.tip.note {
                let dot_w = if *is_online { 16. } else { 0. };
                // Подпись `.rail-tip__note` — с разрядкой `body`.
                let note_w = text_width(window, note, 12., FontWeight::NORMAL, BODY_SPACING);
                natural = natural.max(dot_w + note_w);
            }
            let tip_w = grid_width(natural + 32.);
            let surface = theme.color("l-surface-solid");
            let left = width + 12.;
            let top = shown.center - height / 2.;
            let note_el = shown.tip.note.map(|(note, is_online)| {
                let color = if is_online { online } else { text_dim };
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .text_color(color)
                    .text_px(12., 20.)
                    .when(is_online, |row| row.child(dot(8., color, 0., zoom)))
                    .child(note)
            });
            let body = div()
                .w(u(tip_w))
                .h(u(height))
                .flex()
                .flex_col()
                .gap(u(4.))
                .px(u(16.))
                .py(u(10.))
                .rounded(u(radius_sm))
                .bg(surface)
                .shadow(vec![
                    ring(zoom, 1., border),
                    shadow(zoom, 0., 10., 28., 0., black().opacity(0.45)),
                ]);
            // Liquid Glass: `.rail-tip` — стекло всплывающей панели.
            let body = crate::liquid::glass(
                body,
                crate::liquid::Kind::Panel,
                u(radius_sm),
                gpui::Pixels::ZERO,
                &theme,
                zoom,
            )
            .font_family(RUBIK)
            .whitespace_nowrap()
            .child(
                tip_tail(surface, border)
                    .absolute()
                    .left(u(-9.))
                    .top_0()
                    .w(u(18.))
                    .h_full(),
            )
            .child(spaced(title, 13., 20., FontWeight::BOLD, 0.04, text))
            .children(note_el);
            deferred(
                div()
                    .absolute()
                    .left(u(left - 4. * (1. - v)))
                    .top(u(top))
                    .opacity(v)
                    .child(body),
            )
            .with_priority(TIP_PRIORITY)
        });

        let top = self.top.clone();
        // Тень выдвинутой колонки рисует слой стекла под ней ([`crate::glass::RailGlass`]):
        // под стеклом, как у Chromium (внешняя тень внутри коробки не видна).
        div()
            .id("game-rail")
            .relative()
            .flex_none()
            .h_full()
            .w(u(width))
            .rounded_tl(radius)
            .rounded_bl(radius)
            .font_family(RUBIK)
            .child(
                canvas(
                    move |bounds, _, _| top.set(f32::from(bounds.top())),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(glass)
            .child(
                div()
                    .relative()
                    .size_full()
                    .pt(u(theme.len("l-topbar-h")))
                    .flex()
                    .flex_col()
                    .child(list)
                    .child(footer),
            )
            .children(tip_el)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_wait_then_fade_in_and_reverse_from_where_they_are() {
        let start = Instant::now();
        let ms = |n: u64| start + std::time::Duration::from_millis(n);
        let mut labels = Move::rest(0.);
        labels.retarget(
            1.,
            LABEL_DELAY_MS,
            Timing::new(260., [0., 0., 1., 1.]),
            false,
            start,
        );
        assert_eq!(labels.value(ms(100)), 0.);
        assert!((labels.value(ms(270)) - 0.5).abs() < 1e-3);
        assert_eq!(labels.value(ms(400)), 1.);
        assert!(!labels.moving(ms(400)));
        // Свернули на полпути: гаснут от текущего значения, без задержки.
        let mut half = Move::rest(0.);
        half.retarget(1., 0., Timing::new(100., [0., 0., 1., 1.]), false, start);
        half.retarget(0., 0., Timing::new(100., [0., 0., 1., 1.]), false, ms(50));
        assert!((half.value(ms(50)) - 0.5).abs() < 1e-3);
        assert_eq!(half.value(ms(150)), 0.);
        // Без анимаций — сразу.
        let mut still = Move::rest(0.);
        still.retarget(1., LABEL_DELAY_MS, Timing::med(), true, start);
        assert_eq!(still.value(start), 1.);
    }

    /// Корень на фикстурах: наведение, щелчок, переключатель — как `GameSidebar` 0.4.9.
    #[gpui::test]
    fn rail_tips_selects_and_toggles_like_0_4_9(cx: &mut gpui::TestAppContext) {
        use crate::app::LauncherRoot;
        use crate::snap::{bounds_of, pointer_move, press};
        use gpui::{MouseButton, VisualTestContext};
        use hlsr_ui::fixtures::{FixtureServices, Scene, Variant};
        use serde_json::json;
        use std::sync::Arc;

        cx.update(crate::init);
        let scene: Scene = serde_json::from_value(json!({
            "id": "t", "route": "/launcher", "platform": "win32",
            "window": {"width": 1600, "height": 900},
            "themes": ["hlsr"], "languages": ["ru"], "scales": [1],
            "now": "2026-11-17T12:00:00.000Z", "timezone": "UTC",
            "store": {"settings": {"config": {"language": 1}}},
            "invoke": {"public-online-get": {"status": "ok", "games": {"hl1-won": {"online": 7}}}}
        }))
        .unwrap();
        let variant = Variant {
            theme: "hlsr".into(),
            lang: "ru".into(),
            scale: 1.0,
            zoom: None,
            rail: None,
        };
        let fx = Arc::new(FixtureServices::new(&scene, &variant).unwrap());
        let services: Services = fx.clone();
        let (_, cx) = cx.add_window_view(|_, cx| LauncherRoot::launcher(services, cx));
        cx.run_until_parked();

        let at = |cx: &mut VisualTestContext, target: &str| {
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            bounds_of(target)
                .unwrap_or_else(|| panic!("нет цели {target}"))
                .center()
        };
        let hover = |cx: &mut VisualTestContext, target: &str| {
            let point = at(cx, target);
            cx.update(|window, cx| pointer_move(window, cx, point));
            cx.run_until_parked();
        };
        let click = |cx: &mut VisualTestContext, target: &str| {
            let point = at(cx, target);
            cx.update(|window, cx| {
                pointer_move(window, cx, point);
                press(window, cx, point, MouseButton::Left, 1, true);
                press(window, cx, point, MouseButton::Left, 1, false);
            });
            cx.run_until_parked();
        };
        let tip = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                let root = ShellRoot::get(cx).unwrap();
                let rail = root.read(cx).frame.rail.clone();
                rail.read(cx)
                    .tip
                    .as_ref()
                    .map(|tip| (tip.owner.to_string(), tip.tip.clone()))
            })
        };
        let selected = |fx: &FixtureServices| -> Vec<serde_json::Value> {
            fx.log()
                .into_iter()
                .filter(|entry| {
                    entry["kind"] == "invoke" && entry["channel"] == "game-delivery-selected"
                })
                .map(|entry| entry["args"].clone())
                .collect()
        };

        // Свёрнутая: подсказка у значка, с онлайном; уход с значка — гаснет.
        hover(cx, "rail-game-70");
        let (owner, shown) = tip(cx).expect("подсказка Half-Life");
        assert_eq!(owner, "game-70");
        assert_eq!(shown.title, "Half-Life");
        assert_eq!(shown.note, Some(("Онлайн: 7".into(), true)));
        cx.update(|window, cx| pointer_move(window, cx, gpui::point(zpx(1., 900.), zpx(1., 450.))));
        cx.run_until_parked();
        assert!(tip(cx).is_none());

        // Щелчок выбирает игру (`game-delivery-selected`) и гасит подсказку.
        click(cx, "rail-game-70");
        assert_eq!(selected(&fx).last(), Some(&json!(["70"])));
        assert!(tip(cx).is_none());

        // Переключатель: колонка выдвинута, подсказок у выдвинутой нет.
        click(cx, "rail-toggle");
        let open = cx.update(|_, cx| ShellContext::get(cx).unwrap().nav.rail_open());
        assert!(open);
        hover(cx, "rail-discord");
        assert!(tip(cx).is_none());
    }

    #[test]
    fn tip_width_is_on_the_four_pixel_grid() {
        assert_eq!(grid_width(126.88), 128.);
        assert_eq!(grid_width(128.), 128.);
        assert_eq!(grid_width(128.1), 132.);
    }
}
