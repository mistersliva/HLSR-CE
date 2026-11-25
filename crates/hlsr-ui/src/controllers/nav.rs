//! Навигация оболочки (`LauncherShell.vue` 0.4.9): раздел шапки (`home`/`games`/`configs`),
//! игра, инструмент и «Демо» в «Играх», выдвинутая колонка значков, данные колонки (игры,
//! инструменты, онлайн), раздел Discord (`rpcActivity`) и логотип (двойной щелчок
//! перекрашивает, тройной — «Назад в прошлое!»). Вид только читает состояние; его ведёт этот
//! контроллер, как `data()` оболочки, и сообщает о переменах ([`NavController::on_change`]).
//!
//! Службы (`game-delivery-selected`, `game-stats-get`, `tool-list`, окна настроек и журнала,
//! Discord, папка игр) подключает корень оболочки: сюда приходят слушатели `on_*`.

use crate::Rgba;
use crate::controllers::game_availability::{self, ColumnGame};
use crate::controllers::shell::ShellController;
use crate::env::Env;
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// Разделы шапки по порядку вкладок (`sections`) и ключи их названий.
pub const SECTIONS: [&str; 3] = ["home", "games", "configs"];
pub const SECTION_TITLES: [&str; 3] = ["#L_TAB_HOME", "#L_TAB_GAMES", "#L_TAB_CONFIGS"];
/// Сколько логотип остаётся перекрашенным (`setTimeout` в `onLogoDoubleClick`).
pub const LOGO_REPAINT: Duration = Duration::from_millis(1500);
/// Тройной щелчок по логотипу: каждый не позже стольких мс после прошлого (`onLogoClick`).
pub const LOGO_CLICK_GAP_MS: f64 = 650.0;
/// Значок тоста о недоступной игре.
pub const COMING_SOON_ICON: &str = "fas fa-clock";

/// `launcher-data.js` `shortNames`: название игры в колонке.
pub fn short_name(game_id: &str) -> Option<&'static str> {
    Some(match game_id {
        "70" => "Half-Life",
        "50" => "Opposing Force",
        "130" => "Blue Shift",
        "220" => "Half-Life 2",
        "218" => "HL2 Ghosting",
        "380" => "HL2: Episode One",
        "420" => "HL2: Episode Two",
        _ => return None,
    })
}

/// `launcher-data.js` `gameArt[id].logo`: логотип игры (`GameLogo.game`).
pub fn game_logo(game_id: &str) -> &'static str {
    match game_id {
        "70" => "half-life",
        "50" => "opposing-force",
        "130" => "blue-shift",
        "220" => "half-life-2",
        "380" => "episode-one",
        "420" => "episode-two",
        "218" => "ghosting",
        _ => "",
    }
}

/// `toolArt[toolId].logo`: инструмент с логотипом в кольце игр; остальные — значок `icon`.
pub fn tool_logo(tool_id: &str) -> &'static str {
    match tool_id {
        "livesplit" => "livesplit",
        _ => "",
    }
}

/// Идентификаторы онлайна игры в снимке (`DELIVERY_IDS`; Half-Life 2 — сумма трёх сборок,
/// `onlineByGame`). Игры без записи онлайна не показывают.
pub fn online_ids(game_id: &str) -> &'static [&'static str] {
    match game_id {
        "70" => &["hl1-won"],
        "220" => &["hl2-2707", "hl2-4260", "hl2-5288"],
        "218" => &["ghosting-renewed"],
        _ => &[],
    }
}

/// Онлайн игры колонки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Online {
    /// Записи для игры нет (`onlineFor` → `null`): подписи нет.
    None,
    /// Снимок свежий и все сборки известны: сумма игроков.
    Known(i64),
    /// Снимка нет, он устарел или чего-то не хватает: «Онлайн: нет данных».
    Unknown,
}

/// `onlineByGame` для одной игры.
pub fn online_of(snapshot: Option<&Value>, game_id: &str) -> Online {
    let ids = online_ids(game_id);
    if ids.is_empty() {
        return Online::None;
    }
    let fresh = snapshot
        .and_then(|snapshot| snapshot.get("status"))
        .and_then(Value::as_str)
        == Some("ok");
    let games = snapshot.and_then(|snapshot| snapshot.get("games"));
    let counts: Option<Vec<i64>> = ids
        .iter()
        .map(|id| {
            games
                .and_then(|games| games.get(*id))
                .and_then(|entry| entry.get("online"))
                .and_then(Value::as_f64)
                // typeof === "number": число любого вида; целое число игроков — в колонке.
                .map(|online| online.round() as i64)
        })
        .collect();
    match counts {
        // Огромные `online` насыщаются в i64: сумма тоже насыщается, а не переполняется.
        Some(counts) if fresh => {
            Online::Known(counts.iter().fold(0, |sum, &n| sum.saturating_add(n)))
        }
        _ => Online::Unknown,
    }
}

/// Игра колонки.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RailGame {
    pub id: String,
    pub title: String,
    pub logo: String,
    pub available: bool,
    /// `online`, `unknown` или пусто (онлайна у игры нет).
    pub online_state: String,
    pub online: i32,
}

/// Инструмент колонки.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RailTool {
    pub id: String,
    pub title: String,
    pub logo: String,
    pub icon: String,
}

/// Игры колонки: порядок и доступность — `game_availability`, онлайн — снимок
/// `public-online-get`.
pub fn rail_games(columns: &[ColumnGame], snapshot: Option<&Value>) -> Vec<RailGame> {
    columns
        .iter()
        .map(|column| {
            let id = column.game.id;
            let online = online_of(snapshot, id);
            RailGame {
                id: id.into(),
                title: short_name(id).unwrap_or(column.game.name).into(),
                logo: game_logo(id).into(),
                available: column.available,
                online_state: match online {
                    Online::None => "",
                    Online::Known(_) => "online",
                    Online::Unknown => "unknown",
                }
                .into(),
                online: match online {
                    Online::Known(count) => count.clamp(0, i64::from(i32::MAX)) as i32,
                    _ => 0,
                },
            }
        })
        .collect()
}

/// Инструмент колонки из `ToolCard` (`tool-list`, `tool-status`): `toolId`, `shortName`,
/// `icon`.
pub fn rail_tool(card: &Value) -> Option<RailTool> {
    let id = card.get("toolId")?.as_str().filter(|id| !id.is_empty())?;
    let text = |key: &str| card.get(key).and_then(Value::as_str).unwrap_or_default();
    Some(RailTool {
        id: id.into(),
        title: text("shortName").into(),
        logo: tool_logo(id).into(),
        icon: text("icon").into(),
    })
}

/// Хранилище `prefs.railExpanded` (`launcher-prefs.json`).
pub trait NavPrefs {
    fn rail_expanded(&self) -> bool;
    fn set_rail_expanded(&self, expanded: bool);
}

type GameListener = Rc<dyn Fn(&str)>;
/// Слушатель `backgroundSrc`: игра, чей арт в фоне, или `None`.
type BackgroundListener = Rc<dyn Fn(Option<&str>)>;
type ToolBackgroundListener = Rc<dyn Fn(bool)>;

struct State {
    section: String,
    active_tab: String,
    selected_game: String,
    selected_tool: String,
    /// HLSR-CE: мод на вкладке `mods`.
    selected_mod: String,
    rail_expanded: bool,
    rail_peek: bool,
    /// `configsGameId`: игра «Персонализации»; пусто — вкладку ещё не открывали.
    configs_game: String,
    columns: Vec<ColumnGame>,
    online: Option<Value>,
    /// Счётчики онлайна видны (официальная сборка, [`NavController::show_online`]).
    online_shown: bool,
    tools: Vec<Value>,
}

struct Inner {
    shell: ShellController,
    prefs: Rc<dyn NavPrefs>,
    state: RefCell<State>,
    on_change: RefCell<Option<Rc<dyn Fn()>>>,
    on_game_selected: RefCell<Option<GameListener>>,
    on_tool_selected: RefCell<Option<GameListener>>,
    on_footer_tool: RefCell<Option<GameListener>>,
    /// `backgroundSrc` оболочки (`LauncherShell.vue:495`): игра, чей арт в фоне, — только в
    /// «Играх» с вкладкой игр (`showsGame`), иначе `None`; слушатель зовётся при смене.
    on_background_game: RefCell<Option<BackgroundListener>>,
    background_game: RefCell<Option<String>>,
    /// `:tool-background` фона (`LauncherShell.vue:32`): «Игры» с вкладкой инструментов.
    on_tool_background: RefCell<Option<ToolBackgroundListener>>,
    tool_background: Cell<bool>,
}

/// Контроллер навигации; клон — тот же контроллер.
#[derive(Clone)]
pub struct NavController {
    inner: Rc<Inner>,
}

impl NavController {
    /// Начало — как `data()` оболочки: раздел «Игры», игра по умолчанию, колонка по
    /// `prefs.railExpanded`.
    pub fn new(shell: &ShellController, prefs: Rc<dyn NavPrefs>) -> NavController {
        let columns = game_availability::column_games();
        let state = State {
            section: "games".into(),
            active_tab: "games".into(),
            selected_game: hlsr_delivery::game_availability::DEFAULT_GAME_ID.into(),
            selected_tool: String::new(),
            selected_mod: String::new(),
            rail_expanded: prefs.rail_expanded(),
            rail_peek: false,
            configs_game: String::new(),
            columns,
            online: None,
            online_shown: true,
            tools: Vec::new(),
        };
        NavController {
            inner: Rc::new(Inner {
                shell: shell.clone(),
                prefs,
                state: RefCell::new(state),
                on_change: RefCell::default(),
                on_game_selected: RefCell::default(),
                on_tool_selected: RefCell::default(),
                on_footer_tool: RefCell::default(),
                on_background_game: RefCell::default(),
                background_game: RefCell::new(Some(
                    hlsr_delivery::game_availability::DEFAULT_GAME_ID.into(),
                )),
                on_tool_background: RefCell::default(),
                tool_background: Cell::new(false),
            }),
        }
    }

    /// Кому сообщать о переменах состояния: вид перерисовывается.
    pub fn on_change(&self, listener: impl Fn() + 'static) {
        *self.inner.on_change.borrow_mut() = Some(Rc::new(listener));
    }

    fn changed(&self) {
        let listener = self.inner.on_change.borrow().clone();
        if let Some(listener) = listener {
            listener();
        }
    }

    /// Состояние → слушатели фона и вид.
    fn publish(&self) {
        let state = self.inner.state.borrow();
        // computed backgroundSrc: showsGame → арт выбранной игры, иначе null.
        let shows_game = state.section == "games" && state.active_tab == "games";
        let game = shows_game.then(|| state.selected_game.clone());
        let tool_background = state.section == "games" && state.active_tab == "tools";
        drop(state);
        if self.inner.tool_background.replace(tool_background) != tool_background {
            let listener = self.inner.on_tool_background.borrow().clone();
            if let Some(listener) = listener {
                listener(tool_background);
            }
        }
        if *self.inner.background_game.borrow() != game {
            *self.inner.background_game.borrow_mut() = game.clone();
            let listener = self.inner.on_background_game.borrow().clone();
            if let Some(listener) = listener {
                listener(game.as_deref());
            }
        }
        self.changed();
    }

    // ---- Состояние -------------------------------------------------------------

    /// Раздел шапки (`section`).
    pub fn section(&self) -> String {
        self.inner.state.borrow().section.clone()
    }

    /// Внутри «Игр»: `games`, `tools` или `demo` (`activeTab`).
    pub fn active_tab(&self) -> String {
        self.inner.state.borrow().active_tab.clone()
    }

    /// `rpcActivity`: где игрок для Discord (`discord-rpc-activity`).
    pub fn rpc_activity(&self) -> &'static str {
        let state = self.inner.state.borrow();
        match (state.section.as_str(), state.active_tab.as_str()) {
            ("home", _) => "home",
            ("configs", _) => "personalization",
            ("games", "demo") => "demo",
            ("games", "tools") => "livesplit",
            _ => "games",
        }
    }

    /// `configsGameId`: игра «Персонализации» (пусто — вкладку ещё не открывали).
    pub fn configs_game(&self) -> String {
        self.inner.state.borrow().configs_game.clone()
    }

    /// Шестерёнка, опрос и обучение открывают «Персонализацию» на своей игре.
    pub fn set_configs_game(&self, id: &str) {
        self.inner.state.borrow_mut().configs_game = id.to_owned();
        self.changed();
    }

    pub fn selected_game(&self) -> String {
        self.inner.state.borrow().selected_game.clone()
    }

    pub fn selected_tool(&self) -> String {
        self.inner.state.borrow().selected_tool.clone()
    }

    /// HLSR-CE: мод на вкладке `mods`.
    pub fn selected_mod(&self) -> String {
        self.inner.state.borrow().selected_mod.clone()
    }

    /// Место выбранной игры в колонке.
    pub fn selected_index(&self) -> Option<usize> {
        let state = self.inner.state.borrow();
        state
            .columns
            .iter()
            .position(|column| column.game.id == state.selected_game)
    }

    /// `railExpanded`: запомненное положение колонки в «Играх».
    pub fn rail_expanded(&self) -> bool {
        self.inner.state.borrow().rail_expanded
    }

    /// `railPeek`: колонка раскрыта на другом разделе до ухода с него.
    pub fn rail_peek(&self) -> bool {
        self.inner.state.borrow().rail_peek
    }

    /// `railOpen`: в «Играх» — запомненное положение, на других разделах — только до ухода.
    pub fn rail_open(&self) -> bool {
        let state = self.inner.state.borrow();
        if state.section == "games" {
            state.rail_expanded
        } else {
            state.rail_peek
        }
    }

    // ---- Действия ----------------------------------------------------------------

    /// Щелчок по вкладке шапки (`section = tab.id`). Новый раздел закрывает раскрытую на
    /// «Главной» или в «Персонализации» колонку (`watch.section`).
    pub fn select_section(&self, id: &str) {
        if !SECTIONS.contains(&id) {
            return;
        }
        self.set_section(id);
        self.publish();
    }

    /// `watch.section`: колонка, раскрытая на другом разделе, закрывается; первый заход в
    /// «Персонализацию» — на выбранной игре.
    fn set_section(&self, id: &str) {
        let mut state = self.inner.state.borrow_mut();
        if state.section != id {
            state.section = id.to_owned();
            state.rail_peek = false;
            if id == "configs" && state.configs_game.is_empty() {
                state.configs_game = state.selected_game.clone();
            }
        }
    }

    /// `toggleRail`: в «Играх» положение запоминается, на других разделах колонка
    /// раскрывается до ухода с вкладки, сохранённое не меняется.
    pub fn toggle_rail(&self) {
        {
            let mut state = self.inner.state.borrow_mut();
            if state.section == "games" {
                state.rail_expanded = !state.rail_expanded;
                self.inner.prefs.set_rail_expanded(state.rail_expanded);
            } else {
                state.rail_peek = !state.rail_peek;
            }
        }
        self.publish();
    }

    /// `onSelectGame(id)`: недоступная игра — тост «появится позже»; иначе «Игры», игра
    /// выбрана. Службы (`game-delivery-selected`, `game-stats-get`) — слушатель.
    pub fn select_game(&self, id: &str) {
        if !game_availability::is_available(id) {
            let text = self.inner.shell.tr("#L_GAME_COMING_SOON", &[]);
            self.inner.shell.notify(&text, COMING_SOON_ICON);
            return;
        }
        self.set_section("games");
        {
            let mut state = self.inner.state.borrow_mut();
            state.active_tab = "games".into();
            state.selected_game = id.to_owned();
        }
        self.publish();
        let listener = self.inner.on_game_selected.borrow().clone();
        if let Some(listener) = listener {
            listener(id);
        }
    }

    /// `selectGameFromArgs`: игра из ярлыка или аргументов запуска выбирается, если доступна;
    /// раздел и вкладка не меняются.
    pub fn select_game_from_args(&self, id: &str) {
        if !game_availability::is_available(id) {
            return;
        }
        self.inner.state.borrow_mut().selected_game = id.to_owned();
        self.publish();
    }

    /// Выбранная игра без сценария выбора (старт: `resolveStartupGame`).
    pub fn set_selected_game(&self, id: &str) {
        self.inner.state.borrow_mut().selected_game = id.to_owned();
        self.publish();
    }

    /// `onSelectTool(toolId)`: «Игры», вкладка инструмента. Список инструментов заново —
    /// слушатель (`loadTools`).
    pub fn select_tool(&self, id: &str) {
        self.show_tool(id);
        let listener = self.inner.on_tool_selected.borrow().clone();
        if let Some(listener) = listener {
            listener(id);
        }
    }

    /// «Игры», вкладка инструмента без слушателя: выбор пришёл из сценария оболочки (предложение
    /// LiveSplit перед запуском), список инструментов она уже запросила.
    pub fn show_tool(&self, id: &str) {
        self.set_section("games");
        {
            let mut state = self.inner.state.borrow_mut();
            state.active_tab = "tools".into();
            state.selected_tool = id.to_owned();
        }
        self.publish();
    }

    /// HLSR-CE: свой мод в колонке — «Игры», вкладка мода (`mods`).
    pub fn select_mod(&self, id: &str) {
        self.set_section("games");
        {
            let mut state = self.inner.state.borrow_mut();
            state.active_tab = "mods".into();
            state.selected_mod = id.to_owned();
        }
        self.publish();
    }

    /// Пункт «Демо» колонки (`onSelectDemo` после окна FFmpeg): «Игры», вкладка «Демо».
    pub fn select_demo(&self) {
        self.set_section("games");
        self.inner.state.borrow_mut().active_tab = "demo".into();
        self.publish();
    }

    /// Кнопка внизу колонки: `discord`, `folder`, `changelog`, `settings` (`onTool`).
    pub fn footer_tool(&self, id: &str) {
        let listener = self.inner.on_footer_tool.borrow().clone();
        if let Some(listener) = listener {
            listener(id);
        }
    }

    // ---- Данные ------------------------------------------------------------------

    /// `public-online-get` / `public-online`: снимок онлайна (`onlineSnapshot`).
    pub fn set_online(&self, snapshot: &Value) {
        if !snapshot.is_object() {
            return;
        }
        self.inner.state.borrow_mut().online = Some(snapshot.clone());
        self.changed();
    }

    /// Счётчики онлайна в колонке: только в официальной сборке; скрытые — без подписи.
    pub fn show_online(&self, shown: bool) {
        let was = std::mem::replace(&mut self.inner.state.borrow_mut().online_shown, shown);
        if was != shown {
            self.changed();
        }
    }

    /// `loadTools`: ответ `tool-list`. Если инструмент не выбран, выбран первый.
    pub fn set_tools(&self, list: &Value) {
        let Some(list) = list.as_array() else {
            return;
        };
        {
            let mut state = self.inner.state.borrow_mut();
            state.tools = list.clone();
            if state.selected_tool.is_empty()
                && let Some(first) = list
                    .first()
                    .and_then(|card| card.get("toolId"))
                    .and_then(Value::as_str)
            {
                state.selected_tool = first.to_owned();
            }
        }
        self.publish();
    }

    /// `onToolStatus(card)`: карточка заменяет прежнюю, новая встаёт в конец.
    pub fn apply_tool_status(&self, card: &Value) {
        let Some(id) = card.get("toolId").and_then(Value::as_str) else {
            return;
        };
        {
            let mut state = self.inner.state.borrow_mut();
            match state
                .tools
                .iter()
                .position(|tool| tool.get("toolId").and_then(Value::as_str) == Some(id))
            {
                Some(index) => state.tools[index] = card.clone(),
                None => state.tools.push(card.clone()),
            }
        }
        self.changed();
    }

    /// Игры колонки.
    pub fn games(&self) -> Vec<RailGame> {
        let state = self.inner.state.borrow();
        let mut games = rail_games(&state.columns, state.online.as_ref());
        if !state.online_shown {
            for game in &mut games {
                game.online_state.clear();
                game.online = 0;
            }
        }
        games
    }

    /// Инструменты колонки.
    pub fn tools(&self) -> Vec<RailTool> {
        self.inner
            .state
            .borrow()
            .tools
            .iter()
            .filter_map(rail_tool)
            .collect()
    }

    // ---- Слушатели (службы подключает задача 20) -----------------------------------

    /// Игра выбрана щелчком: `game-delivery-selected`, `game-stats-get`.
    pub fn on_game_selected(&self, listener: impl Fn(&str) + 'static) {
        *self.inner.on_game_selected.borrow_mut() = Some(Rc::new(listener));
    }

    /// Игра для арта фона изменилась (`backgroundSrc`): `Some(игра)` в «Играх» с вкладкой
    /// игр, иначе `None` (`LauncherBackground :src`).
    pub fn on_background_game(&self, listener: impl Fn(Option<&str>) + 'static) {
        *self.inner.on_background_game.borrow_mut() = Some(Rc::new(listener));
    }

    /// Фон инструмента (`:tool-background="section === 'games' && activeTab === 'tools'"`):
    /// слушатель зовётся при смене.
    pub fn on_tool_background(&self, listener: impl Fn(bool) + 'static) {
        *self.inner.on_tool_background.borrow_mut() = Some(Rc::new(listener));
    }

    /// Инструмент выбран щелчком: `loadTools`.
    pub fn on_tool_selected(&self, listener: impl Fn(&str) + 'static) {
        *self.inner.on_tool_selected.borrow_mut() = Some(Rc::new(listener));
    }

    /// Кнопка внизу колонки (`onTool(id)`).
    pub fn on_footer_tool(&self, listener: impl Fn(&str) + 'static) {
        *self.inner.on_footer_tool.borrow_mut() = Some(Rc::new(listener));
    }
}

/// Источник случайных чисел [0, 1) (`Math.random`).
pub type Random = Rc<dyn Fn() -> f64>;

/// `Math.random` без внешних зависимостей: случайные 53 бита от `RandomState`.
pub fn system_random() -> Random {
    Rc::new(|| {
        use std::hash::{BuildHasher, Hasher};
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u8(0);
        (hasher.finish() >> 11) as f64 / (1u64 << 53) as f64
    })
}

struct LogoInner {
    env: Rc<dyn Env>,
    shell: ShellController,
    random: Random,
    color: Cell<Option<Rgba>>,
    /// Поколение перекраски: новый щелчок отодвигает возврат цвета.
    generation: Cell<u64>,
    /// `logoClicks`, `logoClickAt` (мс часов среды).
    clicks: Cell<u32>,
    click_at: Cell<f64>,
    on_change: RefCell<Option<Rc<dyn Fn()>>>,
}

/// Логотип шапки: двойной щелчок (`onLogoDoubleClick`) — на 1,5 с кольцо и точка случайного
/// цвета; три щелчка подряд (`onLogoClick`) — секретное достижение «Назад в прошлое!» (и пони).
#[derive(Clone)]
pub struct LogoController {
    inner: Rc<LogoInner>,
}

impl LogoController {
    pub fn new(env: Rc<dyn Env>, shell: &ShellController) -> LogoController {
        LogoController::with_random(env, shell, system_random())
    }

    /// То же со своим источником случайных чисел (тесты).
    pub fn with_random(
        env: Rc<dyn Env>,
        shell: &ShellController,
        random: Random,
    ) -> LogoController {
        LogoController {
            inner: Rc::new(LogoInner {
                env,
                shell: shell.clone(),
                random,
                color: Cell::new(None),
                generation: Cell::new(0),
                clicks: Cell::new(0),
                // Первый щелчок не бывает вторым: `event.timeStamp - undefined` — NaN.
                click_at: Cell::new(f64::NEG_INFINITY),
                on_change: RefCell::default(),
            }),
        }
    }

    /// Кому сообщать о смене цвета логотипа.
    pub fn on_change(&self, listener: impl Fn() + 'static) {
        *self.inner.on_change.borrow_mut() = Some(Rc::new(listener));
    }

    /// Цвет кольца и точки; `None` — цвет темы.
    pub fn color(&self) -> Option<Rgba> {
        self.inner.color.get()
    }

    /// `channel()`: целое от 0 до 249.
    fn channel(&self) -> u8 {
        ((self.inner.random)() * 250.0).floor().clamp(0.0, 249.0) as u8
    }

    fn changed(inner: &LogoInner) {
        let listener = inner.on_change.borrow().clone();
        if let Some(listener) = listener {
            listener();
        }
    }

    /// Двойной щелчок по логотипу.
    pub fn double_click(&self) {
        let color = Rgba::rgb(self.channel(), self.channel(), self.channel());
        let inner = &self.inner;
        inner.color.set(Some(color));
        Self::changed(inner);
        // clearTimeout + setTimeout: новый щелчок отодвигает возврат цвета.
        let generation = inner.generation.get() + 1;
        inner.generation.set(generation);
        let weak = Rc::downgrade(inner);
        inner.env.after(
            LOGO_REPAINT,
            Box::new(move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if inner.generation.get() != generation {
                    return;
                }
                inner.color.set(None);
                Self::changed(&inner);
            }),
        );
    }

    /// Щелчок по логотипу (`onLogoClick`): третий подряд, каждый не позже 650 мс после прошлого,
    /// открывает «Назад в прошлое!» и отвечает `true` — пора показать пони.
    pub fn click(&self) -> bool {
        let inner = &self.inner;
        let now = inner.env.now();
        let clicks = if now - inner.click_at.get() <= LOGO_CLICK_GAP_MS {
            inner.clicks.get() + 1
        } else {
            1
        };
        inner.click_at.set(now);
        if clicks < 3 {
            inner.clicks.set(clicks);
            return false;
        }
        inner.clicks.set(0);
        inner.shell.unlock_achievement("back-to-past");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn huge_online_saturates_instead_of_overflowing() {
        let snapshot = json!({"status": "ok", "games": {
            "hl2-2707": {"online": 1e300}, "hl2-4260": {"online": 1e300}, "hl2-5288": {"online": 2}
        }});
        assert_eq!(online_of(Some(&snapshot), "220"), Online::Known(i64::MAX));
    }
}
