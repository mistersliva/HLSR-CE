//! Фон лаунчера (`LauncherBackground.vue` 0.4.9): кадр игры во всё окно с перетеканием
//! `.5 с --l-ease-fade`, параллакс по курсору, затемнение `--l-bg-scrim`, передний план поверх
//! затемнения, фон LiveSplit с тремя шариками и тайной игрой «повтори цвета» (`good-memory`).
//!
//! Какие слои показаны, когда прежние гаснут и где стоит параллакс — логика
//! [`BackgroundController`] (`hlsr-ui`), правила и тайминги игры — [`MemoryGame`]. Здесь — только
//! как это нарисовано:
//! - кадр: `cover` по верху в рамке окна, увеличенной на `--l-bg-par-zoom` (1,05), со сдвигом
//!   параллакса; рисуется в рамке окна с её скруглением (`overflow: hidden` и `clip-path`);
//! - передний план — по [`FrontLayout::place`] со сдвигом ×2; наклон 1,3° не переносится (у блоков
//!   GPUI нет `rotate`, как в slint — решение 10);
//! - фон LiveSplit (`radial` + полосы 45° + `linear-gradient(160deg)`) — одна картинка на ЦП в
//!   физических px окна; шарики — блоки с тенями, блик — маска SVG, `filter` — пересчитанные цвета.
//!
//! Вид не решает, что показывать: оболочка зовёт [`Background::show`] (`backgroundSrc`,
//! `foregroundSrc`) и [`Background::set_tool`] (`toolBackground`). Фон лежит во всё окно с его
//! угла. Пауза под окнами — по [`AnimGate`] (окна-слои F7), видимость окна — тоже, фокус — окна.

use crate::env::GpuiEnv;
use crate::glass::Backdrop;
use crate::image::{Pictures, Raster, device_size};
use crate::motion::{AnimGate, Timing, VALUE, keyframes, presence, presence_with, tween};
use crate::paint::{SCRIM, ring, shadow, strips};
use crate::scale::{WINDOW_RADIUS, u, viewport, zoom, zpx};
use crate::theme::{Gradient, Stop, Theme};
use gpui::{
    AnyElement, AppContext, Bounds, BoxShadow, Context, Corners, DispatchPhase, Div, EntityId,
    Hsla, InteractiveElement, IntoElement, MouseExitEvent, MouseMoveEvent, ParentElement, Pixels,
    Render, RenderImage, Rgba, StatefulInteractiveElement, Styled, Subscription, Task, WeakEntity,
    Window, canvas, div, point, px, size, svg,
};
use hlsr_ui::Rgba8Image;
use hlsr_ui::controllers::background::{
    BackgroundController, BackgroundInputs, Selection, register_art,
};
use hlsr_ui::controllers::memory_game::{MemoryGame, ORB_COUNT, os_random};
use hlsr_ui::env::Env;
use hlsr_ui::game_art::FrontLayout;
use hlsr_ui::images::{ImageLoader, ImageRequest, NoRemote};
use hlsr_ui::render::rail_glass::Placement;
use hlsr_ui::services::Services;
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// `--l-dur-fade`: перетекание слоёв (`.launcher-bg-fade-*`), мс.
const FADE_MS: u64 = 500;
/// Шарики: цвет (`--orb`), центр в долях окна, анимация `tool-orbit` со своей задержкой.
const ORBS: [(u32, (f32, f32), &str); ORB_COUNT] = [
    (0xcf3247, (0.72, 0.26), "tool-orb-red"),
    (0x2f5fd8, (0.64, 0.5), "tool-orb-blue"),
    (0x2f9a4a, (0.81, 0.5), "tool-orb-green"),
];
/// Диаметр шарика, CSS px.
const ORB: f32 = 176.;
/// `transition: filter .12s ease-out, box-shadow .12s ease-out`.
const ORB_TIMING: (f64, [f64; 4]) = (120., [0., 0., 0.58, 1.]);
/// Блик шарика: `radial-gradient(circle at 32% 25%, color-mix(orb 72%, white), orb 65%)` —
/// это белый .28 → 0 поверх цвета шарика, круг до дальнего угла (178,18 при 176).
const ORB_GLOSS: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='176' height='176' \
    viewBox='0 0 176 176'><radialGradient id='g' gradientUnits='userSpaceOnUse' cx='56.32' \
    cy='44' r='178.18'><stop offset='0' stop-color='#fff' stop-opacity='0.28'/><stop \
    offset='0.65' stop-color='#fff' stop-opacity='0'/></radialGradient><circle cx='88' cy='88' \
    r='88' fill='url(#g)'/></svg>";

/// Место кадра в окне `view` (CSS px): `cover` по верху в рамке окна × `zoom` (`--l-bg-par-zoom`,
/// увеличение вокруг центра) и сдвиг параллакса `par`.
pub fn frame_place(view: (f32, f32), natural: (f32, f32), zoom: f32, par: (f32, f32)) -> Placement {
    let (box_w, box_h) = (view.0 * zoom, view.1 * zoom);
    let k = (box_w / natural.0.max(1.)).max(box_h / natural.1.max(1.));
    let (width, height) = (natural.0 * k, natural.1 * k);
    Placement {
        x: (view.0 - width) / 2. + par.0,
        y: (view.1 - box_h) / 2. + par.1,
        width,
        height,
    }
}

/// Место переднего плана без параллакса: [`FrontLayout::place`].
fn front_place(layout: FrontLayout, view: (f32, f32), aspect: f64) -> Placement {
    let (x, y, width, height) = layout.place((f64::from(view.0), f64::from(view.1)), aspect);
    Placement {
        x: x as f32,
        y: y as f32,
        width: width as f32,
        height: height as f32,
    }
}

/// `filter: brightness(b) saturate(s)` у цвета (sRGB, как фильтры Chromium; ступени по очереди).
fn filtered(color: Rgba, brightness: f32, saturate: f32) -> Rgba {
    let [r, g, b] = [color.r, color.g, color.b].map(|v| (v * brightness).clamp(0., 1.));
    let s = saturate;
    let mix = |kr: f32, kg: f32, kb: f32| (kr * r + kg * g + kb * b).clamp(0., 1.);
    Rgba {
        r: mix(0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s),
        g: mix(0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s),
        b: mix(0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s),
        a: color.a,
    }
}

fn alpha(color: impl Into<Hsla>, a: f32) -> Hsla {
    Hsla { a, ..color.into() }
}

/// `.launcher-bg__tool` одной картинкой (BGRA, физические px `device`) для окна `view` CSS px:
/// снизу `linear-gradient(160deg, …)`, над ним полосы 45° (2 px из 6 белым .022), сверху блик
/// `radial-gradient(ellipse 46% 52% at 70% 40%, rgba(255,255,255,.05), transparent 70%)`.
/// У Paper свой рисунок в тёмных тонах палитры (launcher.css 0.4.9).
pub fn tool_backdrop(device: (u32, u32), view: (f32, f32), paper: bool) -> image::RgbaImage {
    let stops: [[f32; 3]; 3] = if paper {
        [[23., 14., 16.], [27., 17., 19.], [30., 20., 23.]]
    } else {
        [[10., 13., 18.], [11., 16., 22.], [13., 19., 26.]]
    };
    let (vw, vh) = view;
    let (width, height) = (device.0.max(1), device.1.max(1));
    let (sx, sy) = (vw / width as f32, vh / height as f32);
    let (sin, cos) = 160f32.to_radians().sin_cos();
    let line = (vw * sin).abs() + (vh * cos).abs();
    let diag = std::f32::consts::FRAC_1_SQRT_2;
    let line45 = (vw + vh) * diag;
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t);
    image::RgbaImage::from_fn(width, height, |col, row| {
        let (x, y) = ((col as f32 + 0.5) * sx, (row as f32 + 0.5) * sy);
        let (cx, cy) = (x - vw / 2., y - vh / 2.);
        // Ось градиента CSS: 0deg — вверх, по часовой; y экрана — вниз.
        let t = ((cx * sin - cy * cos) / line + 0.5).clamp(0., 1.);
        let mut c = if t < 0.55 {
            lerp(stops[0], stops[1], t / 0.55)
        } else {
            lerp(stops[1], stops[2], (t - 0.55) / 0.45)
        };
        let along = (cx - cy) * diag + line45 / 2.;
        let stripe = if along.rem_euclid(6.) < 2. { 0.022 } else { 0. };
        let (ex, ey) = ((x - 0.7 * vw) / (0.46 * vw), (y - 0.4 * vh) / (0.52 * vh));
        let gloss = 0.05 * (1. - (ex * ex + ey * ey).sqrt() / 0.7).max(0.);
        for a in [stripe, gloss] {
            c = c.map(|v| v + (255. - v) * a);
        }
        let [r, g, b] = c.map(|v| v.round().clamp(0., 255.) as u8);
        image::Rgba([b, g, r, 255])
    })
}

/// Картинка в рамке элемента (во всё окно) со скруглением окна: видна только часть `place`
/// (CSS px окна) со сдвигом параллакса `shift` внутри рамки, как у слоя под `overflow: hidden`.
fn picture(
    image: Arc<RenderImage>,
    place: Placement,
    shift: (f32, f32),
    radius: Pixels,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window, _| {
            let zoom = zoom(window);
            // Chromium ставит фоновую картинку по целым пикселям устройства, а сдвиг слоя
            // (`transform` параллакса) — дробный.
            let scale = window.scale_factor();
            let snap = |css: f32| zpx(1., (css * zoom * scale).round() / scale);
            let at = Bounds::new(
                bounds.origin
                    + point(
                        snap(place.x) + zpx(zoom, shift.0),
                        snap(place.y) + zpx(zoom, shift.1),
                    ),
                size(snap(place.width), snap(place.height)),
            );
            window
                .paint_image(bounds, at, Corners::all(radius), image, 0, false)
                .ok();
        },
    )
    .size_full()
}

/// Затемнение `--l-bg-scrim` цветом `--l-shade-rgb` (полосы по 2 стопа).
pub fn scrims(theme: &Theme, radius: Pixels) -> Vec<Div> {
    let shade = theme.color("l-shade-rgb");
    SCRIM
        .iter()
        .flat_map(|&(angle, stops)| {
            let gradient = Gradient {
                angle,
                stops: stops
                    .iter()
                    .map(|&(pos, a)| Stop {
                        color: alpha(shade, a).into(),
                        pos,
                    })
                    .collect(),
            };
            strips(&gradient, radius)
        })
        .collect()
}

pub struct Background {
    controller: BackgroundController,
    /// Вид со стеклом над фоном (колонка, [`Background::glass_over`]) и кадр под стеклом, о
    /// котором он уже знает.
    glass: Option<EntityId>,
    told: Option<Backdrop>,
    memory: MemoryGame,
    pictures: Pictures,
    tool: bool,
    /// Выбор, ещё не показанный: до первого кадра (картинки грузятся под размер окна, а его знает
    /// только кадр) и пока его картинки не готовы ([`Background::ready`]).
    pending: Option<Option<Selection>>,
    /// Выбран кадр игры (`backgroundSrc`): параллаксу есть что сдвигать.
    art: bool,
    inputs: Option<BackgroundInputs>,
    /// Фон LiveSplit: под какое окно (физические px, Paper) посчитан или считается.
    tool_key: Option<((u32, u32), bool)>,
    tool_raster: Raster,
    tool_task: Option<Task<()>>,
    /// Фон LiveSplit под стеклом Liquid Glass: окно (биты CSS px), Paper и маленькая картинка.
    tool_glass: Option<((u32, u32, bool), Arc<Rgba8Image>)>,
    /// Последняя показанная картинка слоя (передний план?, ключ).
    held: HashMap<(bool, i32), Arc<RenderImage>>,
    /// Прозрачность слоёв кадра в прошлом кадре окна (перетекание `l-value`): стекло Liquid Glass
    /// берёт под себя и уходящий кадр, и проявляющийся.
    fades: Rc<RefCell<HashMap<i32, f32>>>,
    radius: Pixels,
    activation: Option<Subscription>,
    /// HLSR-CE: под какое окно (физические px) картинки игр уже прогреты и задача прогрева.
    warmed: Option<(u32, u32)>,
    warm_task: Option<Task<()>>,
    _changes: Task<()>,
}

/// HLSR-CE: игры с артом фона ([`hlsr_ui::game_art::GameArt`]; у 380/420 арт Half-Life 2).
const WARM_GAMES: [&str; 3] = ["70", "218", "220"];
/// Прогрев — после первого показанного фона и паузы: выбранная игра грузится первой.
const WARM_DELAY: std::time::Duration = std::time::Duration::from_millis(800);
/// Предел кеша картинок: арт всех игр под окно 2560 × 1440 (кадры и передний план) помещается
/// целиком, переключение игр не разбирает картинки заново.
const PICTURE_BUDGET: usize = 192 * 1024 * 1024;

impl Background {
    pub fn new(services: &Services, cx: &mut Context<Background>) -> Background {
        register_art();
        let env = GpuiEnv::new(cx);
        let pictures = cx.try_global::<Pictures>().cloned().unwrap_or_else(|| {
            // Тесты: ответ потока декодирования будил бы планировщик теста из чужого потока (gpui
            // это ловит) — без глобала `Pictures` картинки фона в тестах не грузятся.
            #[cfg(test)]
            let env = hlsr_ui::env::ManualEnv::new();
            Pictures::new(ImageLoader::new(Rc::new(NoRemote), &*env))
        });
        // HLSR-CE: кеш с запасом под арт всех игр (прогрев — `Background::warm_up`).
        let budget = hlsr_ui::images::DEFAULT_BUDGET_BYTES.max(PICTURE_BUDGET);
        pictures.loader().set_budget(budget);
        let controller = BackgroundController::new(env.clone(), pictures.loader());
        let memory = MemoryGame::new(env.clone(), os_random());
        // Перемены логики приходят и изнутри её же вызовов из вида: перерисовка — через очередь.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let notify = move || {
            let _ = tx.send(());
        };
        controller.on_change(notify.clone());
        memory.on_change(notify);
        let services = services.clone();
        memory.on_won(move || {
            let reply = services.invoke("runner-secret-unlock", vec![json!("good-memory")]);
            env.spawn_local(Box::pin(async move {
                let _ = reply.await;
            }));
        });
        let changes = cx.spawn(async move |this, cx| {
            while rx.recv().await.is_some() {
                while rx.try_recv().is_ok() {}
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        Background {
            controller,
            glass: None,
            told: None,
            memory,
            pictures,
            tool: false,
            pending: None,
            art: false,
            inputs: None,
            tool_key: None,
            tool_raster: Raster::default(),
            tool_task: None,
            tool_glass: None,
            held: HashMap::new(),
            fades: Rc::default(),
            radius: px(WINDOW_RADIUS),
            activation: None,
            warmed: None,
            warm_task: None,
            _changes: changes,
        }
    }

    /// Кадр и передний план игры (`backgroundSrc`, `foregroundSrc`, `frontLayout`); `None` —
    /// фона игры нет (другой раздел, инструмент).
    pub fn show(&mut self, selection: Option<Selection>, cx: &mut Context<Background>) {
        self.pending = Some(selection);
        cx.notify();
    }

    /// `toolBackground`: фон LiveSplit; уход с него сбрасывает «повтори цвета».
    pub fn set_tool(&mut self, on: bool, cx: &mut Context<Background>) {
        if self.tool == on {
            return;
        }
        self.tool = on;
        self.controller.set_tool(on);
        if !on {
            self.memory.reset();
        }
        cx.notify();
    }

    /// Скругление рамки окна (в полном экране — 0).
    pub fn set_radius(&mut self, radius: Pixels, cx: &mut Context<Background>) {
        self.radius = radius;
        cx.notify();
    }

    /// Над фоном стекло вида `view` (колонка): он читает [`Background::backdrop`] в своём
    /// `render`. Он закэширован ([`crate::cache`]) — кадр под стеклом сдвинулся, и вид рисуется
    /// заново.
    pub fn glass_over(&mut self, view: EntityId) {
        self.glass = Some(view);
    }

    /// Кадр под стеклом сейчас — вид со стеклом рисуется заново, если он сдвинулся или сменился.
    /// С кадром движения окна ([`crate::motion::monitor_notify`]), как и сам фон: отдельный кадр
    /// на каждый шаг параллакса удвоил бы кадры.
    pub(crate) fn tell_glass(&mut self, backdrop: Option<Backdrop>, window: &Window) {
        let moved = match (&self.told, &backdrop) {
            (Some(told), Some(now)) => !told.same(now),
            (told, now) => told.is_some() != now.is_some(),
        };
        if !moved {
            return;
        }
        self.told = backdrop;
        if let Some(view) = self.glass {
            crate::motion::monitor_notify(window, view);
        }
    }

    /// Кадр под стеклом колонки: живой слой кадра, его место с параллаксом и увеличением.
    pub fn backdrop(&self, view: (f32, f32), shade: Rgba) -> Option<Backdrop> {
        let layer = self
            .controller
            .frames()
            .into_iter()
            .rev()
            .find(|l| l.alive)?;
        let pixels = self
            .pictures
            .loader()
            .cached(&self.controller.frame_request(&layer))?;
        let natural = (pixels.width() as f32, pixels.height() as f32);
        let (zoom, par, _) = self.parallax();
        Some(Backdrop {
            frame: frame_place(view, natural, zoom, par),
            pixels,
            shade,
            view,
        })
    }

    /// Подложка стекла Liquid Glass: живой кадр и передний план там, где они в окне сейчас (без
    /// кадра — `--l-bg-fallback`), на LiveSplit — его фон и шарики. Шарики — круги цвета без
    /// блика и тени и без качания `tool-orbit` (под окнами оно стоит, под стеклом σ ≥ 6 px).
    fn liquid_under(&mut self, view: (f32, f32), cx: &mut Context<Background>) {
        let loader = self.pictures.loader();
        let (zoom, par, shift) = self.parallax();
        let tool = self.tool.then(|| {
            let paper = cx.global::<Theme>().id() == "paper";
            let key = (view.0.to_bits(), view.1.to_bits(), paper);
            if self.tool_glass.as_ref().is_none_or(|(k, _)| *k != key) {
                // Рисунок гладкий: стеклу хватает 160 × 90 (BGRA → RGBA).
                let mut small = tool_backdrop((160, 90), view, paper);
                for pixel in small.pixels_mut() {
                    pixel.0.swap(0, 2);
                }
                let image = Rgba8Image::from_bytes(160, 90, small.as_raw());
                self.tool_glass = Some((key, Arc::new(image)));
            }
            let image = self.tool_glass.as_ref().expect("только что").1.clone();
            let whole = Placement {
                x: 0.,
                y: 0.,
                width: view.0,
                height: view.1,
            };
            (image, whole)
        });
        let orbs = if self.tool {
            ORBS.iter()
                .enumerate()
                .map(|(index, &(tone, (x, y), _))| {
                    let (lit, miss) = (self.memory.lit(index), self.memory.miss());
                    let brightness =
                        1. + 0.5 * f32::from(u8::from(lit)) - 0.65 * f32::from(u8::from(miss));
                    let saturate =
                        1. + 0.15 * f32::from(u8::from(lit)) - 0.6 * f32::from(u8::from(miss));
                    let color = filtered(gpui::rgb(tone), brightness, saturate);
                    let center = (view.0 * x, view.1 * y);
                    crate::liquid::Orb {
                        center,
                        radius: ORB / 2.,
                        color: crate::liquid::bytes(color),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let frames = self.controller.frames();
        let fades = self.fades.borrow();
        let cached = |layer: &hlsr_ui::controllers::background::BackgroundLayer| {
            let pixels = loader.cached(&self.controller.frame_request(layer))?;
            let natural = (pixels.width() as f32, pixels.height() as f32);
            let opacity = fades.get(&layer.key).copied().unwrap_or(0.);
            Some((pixels, frame_place(view, natural, zoom, par), opacity))
        };
        let live = frames
            .iter()
            .rev()
            .find(|layer| layer.alive)
            .and_then(cached);
        // Пока живой кадр проявляется, под стеклом — уходящий, а живой ложится над ним.
        let leaving = frames
            .iter()
            .rev()
            .filter(|layer| !layer.alive)
            .find_map(|layer| cached(layer).filter(|(_, _, opacity)| *opacity > 0.));
        let (frame, incoming) = match (live, leaving) {
            (Some((new, at, alpha)), leaving) if alpha < 1. => {
                // Уходящего нет (первый кадр, игра без арта): под новым — запасной фон.
                let (frame, gone) = match leaving {
                    Some((old, place, gone)) => (Some((old, place)), gone),
                    None => (None, 0.),
                };
                (frame, Some((new, at, alpha, gone)))
            }
            (live, _) => (live.map(|(pixels, place, _)| (pixels, place)), None),
        };
        drop(fades);
        let front = self
            .controller
            .fronts()
            .into_iter()
            .rev()
            .find(|layer| layer.alive)
            .and_then(|layer| {
                let pixels = loader.cached(&self.controller.front_request(&layer))?;
                let aspect = f64::from(pixels.width()) / f64::from(pixels.height().max(1));
                let place = front_place(layer.layout, view, aspect);
                Some((
                    pixels,
                    Placement {
                        x: place.x + shift.0,
                        y: place.y + shift.1,
                        ..place
                    },
                ))
            });
        let (frame, incoming) = match tool {
            Some(tool) => (Some(tool), None),
            None => (frame, incoming),
        };
        crate::liquid::set_under(cx, frame, incoming, front, orbs, view);
    }

    /// `--l-bg-par-zoom`, `--l-bg-par-x/y` и `--l-bg-front-x/y` (CSS px).
    fn parallax(&self) -> (f32, (f32, f32), (f32, f32)) {
        let vars = self.controller.vars();
        let pair = |v: Option<(f64, f64)>| v.map_or((0., 0.), |(x, y)| (x as f32, y as f32));
        (
            vars.and_then(|v| v.zoom).unwrap_or(1.) as f32,
            pair(vars.and_then(|v| v.par)),
            pair(vars.and_then(|v| v.front)),
        )
    }

    /// Окно, фокус, пауза, видимость и движение — модели параллакса; шаг на кадре окна.
    fn sync(&mut self, view: (f32, f32), window: &mut Window, cx: &mut Context<Background>) {
        if self.activation.is_none() {
            self.activation = Some(cx.observe_window_activation(window, |_, _, cx| cx.notify()));
        }
        let gate = AnimGate::get(cx);
        let (width, height) = device_size(window, view.0, view.1);
        self.controller
            .set_window_px(f64::from(width), f64::from(height));
        self.controller
            .set_fade_ms(if gate.still.any() { 0 } else { FADE_MS });
        if let Some(selection) = self.pending.take() {
            self.art = selection.is_some();
            if self.ready(selection.as_ref(), window, cx) {
                self.controller.show(selection);
            } else {
                self.pending = Some(selection);
            }
        }
        if self.pending.is_none() {
            self.warm_up((width, height), cx);
        }
        let inputs = BackgroundInputs {
            paused: gate.top_layer > 0,
            focused: window.is_window_active(),
            visible: !gate.hidden,
            moving: !gate.still.any(),
            width: f64::from(view.0),
            height: f64::from(view.1),
        };
        if self.inputs != Some(inputs) {
            self.inputs = Some(inputs);
            self.controller.inputs_changed(&inputs);
        }
        if self.controller.frame_pending() {
            self.controller.frame();
            // Пружина считает по реальному времени кадра: кадры реже — путь тот же. Сдвиг виден
            // только у кадра игры и переднего плана: без них («Персонализация», «Главная»; уходящие
            // слои доигрывают) кадры окна на параллакс не тратятся — пружина пойдёт дальше с кадром.
            if self.art || !self.controller.frames().is_empty() {
                crate::motion::monitor_frame(window);
            }
        }
    }

    /// HLSR-CE: прогрев арта всех игр под окно `size` (после паузы, в фоновых потоках загрузчика):
    /// без него картинка игры разбирается при первом выборе (WebP Ghosting — ~0,3 с), и фон
    /// меняется с задержкой. Новый размер окна — прогрев заново (прежний отменяется).
    fn warm_up(&mut self, size: (u32, u32), cx: &mut Context<Background>) {
        if cfg!(test) || self.warmed == Some(size) {
            return;
        }
        self.warmed = Some(size);
        self.warm_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WARM_DELAY).await;
            this.update(cx, |this, _| {
                let loader = this.pictures.loader();
                let selections = WARM_GAMES.iter().flat_map(|game| {
                    [Selection::of_game(game), Selection::of_episodes(game)]
                        .into_iter()
                        .flatten()
                });
                for selection in selections {
                    for request in this.controller.requests(&selection) {
                        if loader.cached(&request).is_none() {
                            loader.request(request, |_| {});
                        }
                    }
                }
            })
            .ok();
        }));
    }

    /// Картинки выбора готовы к отрисовке (или не загрузились): кадр и передний план входят одним
    /// шагом, а не каждый по своей загрузке — в 0.4.9 `preload` и `preloadFront` успевали вместе.
    /// Не готовы — грузятся все разом, вид перерисуется, когда они придут.
    fn ready(
        &mut self,
        selection: Option<&Selection>,
        window: &mut Window,
        cx: &mut Context<Background>,
    ) -> bool {
        let Some(selection) = selection else {
            return true;
        };
        // `fold` с `&`, а не `all`: `all` остановился бы на первой неготовой, и прочие не грузились бы.
        (self.controller.requests(selection).into_iter())
            .map(|request| self.pictures.get(request, window, cx).is_some())
            .fold(true, |ready, got| ready & got)
    }

    /// Курсор окна — параллаксу (`mousemove` и `mouseleave` документа).
    fn pointer(cx: &mut Context<Background>) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let moved = this.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    #[cfg(feature = "dev-hooks")]
                    if cx.has_global::<StillParallax>() {
                        return;
                    }
                    let z = zoom(window);
                    let x = f64::from(f32::from(event.position.x) / z);
                    let y = f64::from(f32::from(event.position.y) / z);
                    moved
                        .update(cx, |bg, _| bg.controller.pointer_moved(x, y))
                        .ok();
                });
                window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        this.update(cx, |bg, _| bg.controller.pointer_left()).ok();
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }

    /// Картинка слоя `slot` (передний план?, ключ) под нынешнее окно. Пока она грузится (окно
    /// сменило размер) — прежняя картинка слоя, а не пустое место; новая вытесняет её из атласа.
    fn layer_image(
        &mut self,
        slot: (bool, i32),
        request: ImageRequest,
        window: &mut Window,
        cx: &mut Context<Background>,
    ) -> Option<Arc<RenderImage>> {
        match self.pictures.get(request, window, cx) {
            Some(Ok(image)) => {
                if let Some(old) = self.held.insert(slot, image.clone())
                    && !Arc::ptr_eq(&old, &image)
                {
                    cx.drop_image(old, Some(window));
                }
                Some(image)
            }
            Some(Err(_)) => None,
            None => self.held.get(&slot).cloned(),
        }
    }

    /// Ушедшие слои отпускают свои картинки.
    fn release_gone(&mut self, window: &mut Window, cx: &mut Context<Background>) {
        let alive: Vec<(bool, i32)> = (self.controller.frames().iter())
            .map(|layer| (false, layer.key))
            .chain(
                self.controller
                    .fronts()
                    .iter()
                    .map(|layer| (true, layer.key)),
            )
            .collect();
        let gone: Vec<(bool, i32)> = self
            .held
            .keys()
            .filter(|slot| !alive.contains(slot))
            .copied()
            .collect();
        for slot in gone {
            if let Some(image) = self.held.remove(&slot) {
                cx.drop_image(image, Some(window));
            }
        }
    }

    /// Слои кадра (`.launcher-bg__image`), живой последним; слой без готовой картинки ждёт её.
    fn frames(
        &mut self,
        view: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Background>,
    ) -> Vec<AnyElement> {
        let (zoom, par, _) = self.parallax();
        let mut out = Vec::new();
        let layers = self.controller.frames();
        self.fades
            .borrow_mut()
            .retain(|key, _| layers.iter().any(|layer| layer.key == *key));
        for layer in layers {
            let request = self.controller.frame_request(&layer);
            let Some(image) = self.layer_image((false, layer.key), request, window, cx) else {
                continue;
            };
            let natural = image.size(0);
            let natural = (natural.width.0 as f32, natural.height.0 as f32);
            let place = frame_place(view, natural, zoom, (0., 0.));
            let radius = self.radius;
            let fades = self.fades.clone();
            let key = layer.key;
            out.push(
                presence_with(
                    ("bg-frame", layer.key as usize),
                    layer.alive,
                    VALUE,
                    move |frame| {
                        fades.borrow_mut().insert(key, frame.opacity as f32);
                        picture(image, place, par, radius)
                    },
                )
                .appear()
                .absolute()
                .inset_0()
                .into_any_element(),
            );
        }
        out
    }

    /// Слои переднего плана (`.launcher-bg__front`).
    fn fronts(
        &mut self,
        view: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Background>,
    ) -> Vec<AnyElement> {
        let (_, _, shift) = self.parallax();
        let mut out = Vec::new();
        for layer in self.controller.fronts() {
            let request = self.controller.front_request(&layer);
            let Some(image) = self.layer_image((true, layer.key), request, window, cx) else {
                continue;
            };
            let natural = image.size(0);
            let aspect = f64::from(natural.width.0) / f64::from(natural.height.0.max(1));
            let place = front_place(layer.layout, view, aspect);
            let radius = self.radius;
            out.push(
                presence(
                    ("bg-front", layer.key as usize),
                    layer.alive,
                    VALUE,
                    move || picture(image, place, shift, radius),
                )
                .appear()
                .absolute()
                .inset_0()
                .into_any_element(),
            );
        }
        out
    }

    /// `.launcher-bg__tool`: картинка фона под окно (считается в фоне, пока фон нужен) и шарики.
    /// Строится и уходящим: перетекание доигрывает с тем же рисунком.
    fn tool_layer(
        &mut self,
        view: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Background>,
    ) -> impl FnOnce() -> Div + 'static {
        let paper = cx.global::<Theme>().id() == "paper";
        let device = device_size(window, view.0, view.1);
        let key = (device, paper);
        if self.tool && self.tool_key != Some(key) {
            self.tool_key = Some(key);
            let task = cx.background_spawn(async move {
                let pixels = tool_backdrop(device, view, paper);
                Arc::new(RenderImage::new([image::Frame::new(pixels)]))
            });
            self.tool_task = Some(cx.spawn(async move |this, cx| {
                let image = task.await;
                this.update(cx, |bg, cx| {
                    bg.tool_raster.set_render(image);
                    cx.notify();
                })
                .ok();
            }));
        }
        let image = self.tool_raster.image(window, cx);
        let radius = self.radius;
        let zoom = zoom(window);
        let this = cx.entity().downgrade();
        let states: Vec<(bool, bool)> = (0..ORB_COUNT)
            .map(|index| (self.memory.lit(index), self.memory.miss()))
            .collect();
        move || {
            let whole = Placement {
                x: 0.,
                y: 0.,
                width: view.0,
                height: view.1,
            };
            let orbs =
                ORBS.iter()
                    .zip(states)
                    .enumerate()
                    .map(|(index, (&(tone, at, orbit), state))| {
                        let place = (view.0 * at.0 - ORB / 2., view.1 * at.1 - ORB / 2.);
                        orb(
                            index,
                            gpui::rgb(tone),
                            place,
                            orbit,
                            state,
                            zoom,
                            this.clone(),
                        )
                    });
            div()
                .size_full()
                .children(image.map(|image| picture(image, whole, (0., 0.), radius)))
                .children(orbs)
        }
    }
}

/// Шарик `.launcher-bg__orb`: качается `tool-orbit`, подсветка и ошибка — переходом .12 с.
fn orb(
    index: usize,
    tone: Rgba,
    (left, top): (f32, f32),
    orbit: &'static str,
    (lit, miss): (bool, bool),
    zoom: f32,
    this: WeakEntity<Background>,
) -> impl IntoElement {
    let timing = Timing::new(ORB_TIMING.0, ORB_TIMING.1);
    let target = |on: bool| if on { 1. } else { 0. };
    keyframes(("bg-orb", index), orbit, move |frame| {
        let top = top + frame.y as f32;
        tween(("bg-orb-lit", index), target(lit), timing, move |lit| {
            tween(("bg-orb-miss", index), target(miss), timing, move |miss| {
                orb_body(index, tone, (left, top), (lit, miss), zoom, this)
            })
        })
    })
}

fn orb_body(
    index: usize,
    tone: Rgba,
    (left, top): (f32, f32),
    (lit, miss): (f32, f32),
    z: f32,
    this: WeakEntity<Background>,
) -> impl IntoElement {
    // filter: brightness(1.5) saturate(1.15) у подсветки, brightness(.35) saturate(.4) у ошибки.
    let brightness = 1. + 0.5 * lit - 0.65 * miss;
    let saturate = 1. + 0.15 * lit - 0.6 * miss;
    let color = filtered(tone, brightness, saturate);
    let white = filtered(gpui::white().into(), brightness, saturate);
    // box-shadow между списками разной формы не перетекает: смена посередине перехода.
    let (outer, inset): (Vec<BoxShadow>, Vec<BoxShadow>) = if lit >= 0.5 {
        (
            vec![
                shadow(z, 0., 0., 40., 12., alpha(color, 0.8)),
                shadow(z, 0., 0., 110., 36., alpha(color, 0.45)),
            ],
            vec![ring(z, 1., alpha(white, 0.3))],
        )
    } else {
        (
            vec![shadow(z, 0., 24., 56., 0., alpha(color, 0.24))],
            vec![
                ring(z, 1., alpha(white, 0.14)),
                BoxShadow {
                    inset: true,
                    ..shadow(z, -12., -16., 32., 0., alpha(gpui::black(), 0.18))
                },
            ],
        )
    };
    let radius = ORB / 2.;
    div()
        .id(("bg-orb-hit", index))
        .absolute()
        .left(u(left))
        .top(u(top))
        .size(u(ORB))
        .rounded_full()
        .bg(color)
        .shadow(outer)
        // Игра тайная: курсор не меняется, подсказок нет. Щелчок ловит круг, а не квадрат.
        .on_click(move |event, window, cx| {
            let z = zoom(window);
            let at = event.position();
            let dx = f32::from(at.x) / z - (left + radius);
            let dy = f32::from(at.y) / z - (top + radius);
            if dx * dx + dy * dy <= radius * radius {
                // Уходящий фон (доигрывает 500 мс) игру не начинает: она бы шла на другой странице.
                this.update(cx, |bg, _| {
                    if bg.tool {
                        bg.memory.orb_clicked(index);
                    }
                })
                .ok();
            }
        })
        .child(
            svg()
                .data(ORB_GLOSS.as_bytes())
                .absolute()
                .inset_0()
                .size_full()
                .text_color(white),
        )
        .child(div().absolute().inset_0().rounded_full().shadow(inset))
}

impl Render for Background {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::live::rendered(cx.entity_id());
        let view = viewport(window);
        self.sync(view, window, cx);
        let theme = cx.global::<Theme>().clone();
        let backdrop = self.backdrop(view, theme.color("l-shade-rgb").into());
        self.tell_glass(backdrop, window);
        let radius = self.radius;
        let frames = self.frames(view, window, cx);
        let fronts = self.fronts(view, window, cx);
        self.release_gone(window, cx);
        if crate::liquid::active(&theme) {
            self.liquid_under(view, cx);
        }
        let tool = self.tool_layer(view, window, cx);
        // `size_full`: в кэше ([`crate::cache`]) фон — корень раскладки своего места.
        div()
            .absolute()
            .inset_0()
            .size_full()
            .child(Self::pointer(cx))
            // Виден, пока картинка грузится, и если она не загрузилась вовсе.
            .children(strips(&theme.gradient("l-bg-fallback"), radius))
            .children(frames)
            .child(
                presence("bg-tool", self.tool, VALUE, tool)
                    .absolute()
                    .inset_0(),
            )
            .children(scrims(&theme, radius))
            .children(fronts)
    }
}

/// Сцены стенда без `parallax` (`dev-hooks`): фон не следит за курсором, как у эталона, где
/// `snapCss` ставит сдвиги параллакса в 0; увеличение 1,05 остаётся.
#[cfg(feature = "dev-hooks")]
pub struct StillParallax;

#[cfg(feature = "dev-hooks")]
impl gpui::Global for StillParallax {}

/// Сцена сверочного стенда `background/*` (вид `background`): фон во всё окно на корне окна,
/// игра — как `resolveStartupGame` по `library.lastLaunched` сцены, эпизод — `prefs.ghostingBuild`
/// Ghosting; цель `rail-tool` — пункт LiveSplit колонки (`onSelectTool`: фон игры уходит, входит
/// фон инструмента).
#[cfg(feature = "dev-hooks")]
pub struct BackgroundProbe {
    background: gpui::Entity<Background>,
}

#[cfg(feature = "dev-hooks")]
impl BackgroundProbe {
    pub fn new(
        services: &Services,
        scene: &hlsr_ui::fixtures::Scene,
        prefs: &serde_json::Map<String, serde_json::Value>,
        cx: &mut Context<BackgroundProbe>,
    ) -> BackgroundProbe {
        use hlsr_ui::controllers::game_availability::startup_game;
        let last = scene
            .store
            .get("library")
            .and_then(|library| library.get("lastLaunched"));
        let game = startup_game(last);
        let episode = game == "218"
            && prefs
                .get("ghostingBuild")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|build| matches!(build, "Episode One" | "Episode Two"));
        let selection = if episode {
            Selection::of_episodes(&game)
        } else {
            Selection::of_game(&game)
        };
        let background = cx.new(|cx| Background::new(services, cx));
        background.update(cx, |bg, cx| bg.show(selection, cx));
        BackgroundProbe { background }
    }
}

#[cfg(feature = "dev-hooks")]
impl Render for BackgroundProbe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::theme::frame(window, cx);
        let theme = cx.global::<Theme>();
        let background = self.background.clone();
        let tool = crate::snap::target(
            "rail-tool",
            div()
                .id("probe-rail-tool")
                .absolute()
                .left(u(16.))
                .top(u(400.))
                .size(u(48.))
                .on_click(move |_, _, cx| {
                    background.update(cx, |bg, cx| {
                        bg.show(None, cx);
                        bg.set_tool(true, cx);
                    });
                }),
        );
        div()
            .size_full()
            .relative()
            .rounded(px(WINDOW_RADIUS))
            .bg(theme.color("l-bg"))
            .child(self.background.clone())
            .child(tool)
            // #app::after: контур окна inset 0 0 0 1px rgba(255,255,255,.12).
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(WINDOW_RADIUS))
                    .border_1()
                    .border_color(alpha(gpui::white(), 0.12)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_covers_the_window_from_the_top_with_parallax_zoom() {
        // 2560×1440 в окне 1600×900 при --l-bg-par-zoom 1,05: рамка 1680×945 вокруг центра.
        let place = frame_place((1600., 900.), (2560., 1440.), 1.05, (3., -2.));
        let near =
            |got: [f32; 4], want: [f32; 4]| got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-3);
        let got = [place.x, place.y, place.width, place.height];
        assert!(near(got, [-37., -24.5, 1680., 945.]), "{got:?}");
        // Без параллакса кадр выше окна обрезается снизу (center top).
        let tall = frame_place((1600., 900.), (1000., 1000.), 1., (0., 0.));
        let got = [tall.x, tall.y, tall.width, tall.height];
        assert!(near(got, [0., 0., 1600., 1600.]), "{got:?}");
    }

    #[test]
    fn filters_and_tool_backdrop_follow_css() {
        let white: Rgba = gpui::white().into();
        assert_eq!(filtered(white, 1., 1.), white);
        let dim = filtered(white, 0.35, 0.4);
        assert!((dim.r - 0.35).abs() < 1e-5 && (dim.g - dim.b).abs() < 1e-5);
        assert_eq!(
            filtered(white, 1.5, 1.15).r,
            1.,
            "яркость упирается в белый"
        );

        let image = tool_backdrop((160, 90), (1600., 900.), false);
        assert_eq!(image.dimensions(), (160, 90));
        // Левый верх — начало 160deg (#0a0d12, полоса добавляет до 6), правый низ — конец
        // (#0d131a); порядок байт BGRA.
        let [b, g, r, a] = image.get_pixel(0, 0).0;
        assert!(r <= 16 && g <= 19 && b <= 24 && a == 255, "{r} {g} {b}");
        let [b, _, r, _] = image.get_pixel(159, 89).0;
        assert!(r >= 13 && b >= 26, "{r} {b}");
        let paper = tool_backdrop((160, 90), (1600., 900.), true);
        assert!(
            paper.get_pixel(0, 0).0[2] > image.get_pixel(0, 0).0[2],
            "Paper теплее"
        );
    }

    /// Сервисы, которые только записывают вызовы окна.
    #[derive(Default)]
    struct Calls(std::sync::Mutex<Vec<String>>);

    impl hlsr_ui::services::MainProcess for Calls {
        fn invoke(
            &self,
            channel: &str,
            args: Vec<serde_json::Value>,
        ) -> hlsr_ui::services::BoxFuture<'static, hlsr_ui::services::Reply> {
            self.0.lock().unwrap().push(format!("{channel} {args:?}"));
            Box::pin(async { Ok(serde_json::Value::Null) })
        }
        fn send(&self, _: &str, _: Vec<serde_json::Value>) {}
        fn send_sync(&self, _: &str, _: Vec<serde_json::Value>) -> serde_json::Value {
            serde_json::Value::Null
        }
        fn store_read(&self, _: &str) -> hlsr_ui::services::StoreRead {
            hlsr_ui::services::StoreRead::Missing
        }
        fn store_write(&self, _: &str, _: serde_json::Value) -> bool {
            false
        }
        fn store_quarantine(&self, _: &str) -> serde_json::Value {
            serde_json::Value::Null
        }
        fn subscribe(&self) -> tokio::sync::broadcast::Receiver<hlsr_ui::services::ServiceEvent> {
            tokio::sync::broadcast::channel(1).1
        }
    }

    /// Корень окна: сколько раз окно перерисовано.
    struct Counted {
        background: gpui::Entity<Background>,
        renders: Rc<std::cell::Cell<usize>>,
    }

    impl Render for Counted {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div().size_full().child(self.background.clone())
        }
    }

    /// Параллакс после движения мыши перерисовывает окно с кадрами монитора окна (тестовый — 60 Гц)
    /// и при будильнике 280 Гц: шаг модели не будит вид сам (иначе кадр шёл на каждый будильник).
    #[gpui::test]
    fn parallax_frames_are_capped(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let services: Services = Arc::new(Calls::default());
        let renders = Rc::new(std::cell::Cell::new(0));
        let window = cx.open_window(size(zpx(1., 1600.), zpx(1., 900.)), {
            let renders = renders.clone();
            move |_, cx| Counted {
                background: cx.new(|cx| Background::new(&services, cx)),
                renders,
            }
        });
        window
            .update(cx, |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
        // Кадры монитора 280 Гц: сколько из них окно перерисовало.
        let frames = |cx: &mut gpui::TestAppContext, count: usize| {
            let before = renders.get();
            for _ in 0..count {
                cx.executor()
                    .advance_clock(std::time::Duration::from_micros(3_571));
                window
                    .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                    .unwrap();
                cx.run_until_parked();
            }
            renders.get() - before
        };
        frames(cx, 280);
        assert!(frames(cx, 280) <= 2, "без движения окно стоит");
        let move_to = |cx: &mut gpui::TestAppContext, x: f32| {
            window
                .update(cx, |_, window, cx| {
                    let event = MouseMoveEvent {
                        position: point(zpx(1., x), zpx(1., 100.)),
                        pressed_button: None,
                        modifiers: gpui::Modifiers::default(),
                    };
                    window.dispatch_event(gpui::PlatformInput::MouseMove(event), cx);
                })
                .unwrap();
        };
        // Кадра игры нет («Персонализация», «Главная»): сдвигать нечего — окно стоит.
        move_to(cx, 1500.);
        assert!(
            frames(cx, 140) <= 2,
            "параллакс без кадра игры кадров не просит"
        );
        window
            .update(cx, |root, _, cx| {
                root.background
                    .update(cx, |bg, cx| bg.show(Selection::of_game("218"), cx))
            })
            .unwrap();
        frames(cx, 2);
        move_to(cx, 100.);
        let drawn = frames(cx, 140);
        let steps = window
            .update(cx, |root, _, cx| {
                root.background.read(cx).controller.parallax_steps()
            })
            .unwrap();
        assert!(steps > 20, "параллакс идёт: {steps} шагов");
        assert!(drawn <= 42, "{drawn} кадров за 0,5 с");
    }

    #[gpui::test]
    fn orb_click_starts_the_game_only_inside_the_circle(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        // Без движения: шарики стоят на месте (tool-orbit снят), переходы мгновенные.
        cx.update(|cx| AnimGate::set_no_anim(cx, true));
        let services: Services = Arc::new(Calls::default());
        let (bg, cx) = cx.add_window_view(|_, cx| Background::new(&services, cx));
        bg.update(cx, |bg, cx| bg.set_tool(true, cx));
        cx.run_until_parked();
        let (view, zoom) = cx.update(|window, _| (viewport(window), zoom(window)));
        let at = |x: f32, y: f32| point(zpx(zoom, x), zpx(zoom, y));
        let (left, top) = (view.0 * 0.72 - ORB / 2., view.1 * 0.26 - ORB / 2.);
        // Угол квадрата вокруг красного шарика — мимо круга.
        cx.simulate_click(at(left + 8., top + 8.), gpui::Modifiers::default());
        bg.update(cx, |bg, _| assert!(bg.memory.game().is_none()));
        cx.simulate_click(
            at(left + ORB / 2., top + ORB / 2.),
            gpui::Modifiers::default(),
        );
        bg.update(cx, |bg, _| {
            assert!(bg.memory.game().is_some(), "щелчок по шарику начинает игру");
            assert!(
                (0..ORB_COUNT).all(|orb| bg.memory.lit(orb)),
                "старт: горят все три"
            );
        });
        // Уход со страницы LiveSplit сбрасывает игру.
        bg.update(cx, |bg, cx| bg.set_tool(false, cx));
        bg.update(cx, |bg, _| assert!(bg.memory.game().is_none()));
    }
}
