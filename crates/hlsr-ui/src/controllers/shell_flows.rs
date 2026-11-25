//! Сценарии оболочки (`LauncherShell.vue` 0.4.7, методы): запуск и остановка игры, лицензия и
//! Steam (`offerLaunchFix`, `explainDeliveryError`), установка, меню игры (обновления, ремонт и
//! удаление с подтверждением), перенос сборки, инструменты и LiveSplit (`offerTimer`, окно запуска,
//! сплиты), статусы доставки и автообновление игр, перенос библиотеки, события процесса,
//! окно «Недоступно», «что нового», выбор папки библиотеки, приветствие, настройки загрузок и
//! карточка обновления лаунчера, запуск оболочки (`mounted`).
//!
//! Вызовы — через сервисы окна ([`Services`], каналы 0.4.7); тосты, окна, prefs, буфер обмена,
//! часы и задачи — через [`ShellPort`]. Состояние оболочки ([`ShellState`]: раздел, игра,
//! инструмент, открытые окна, статусы доставки…) читают адаптеры после [`ShellPort::changed`].
//!
//! Методы 0.4.7 `async` идут задачами порта: синхронная часть (до первого `await`) выполняется
//! сразу — вызовы каналов уходят в том же порядке, что в 0.4.7, — продолжение ставится задачей.
//! Сверка — настоящий скрипт `LauncherShell.vue`, выполненный в Node на тех же сценариях
//! (`tools/reference/ui/shell-scenarios.js`, `tests/shell_flows.rs`).
//!
//! Не здесь: логотип и тема (`onLogoDoubleClick`, `setTheme` — шапка и персонализация), колонка
//! (`toggleRail`), профиль и SteamID (`openRunnerProfile`, `onToggleSteamId`, `pushHideSteamId`,
//! `onServerHideSteamId`, `onAccountAction`), секретная последовательность (`keys.rs`), время игры
//! (`game_demo`).

use super::game_availability::{self, GAME_LIST};
use super::game_demo;
use super::shell::{Confirm, ConfirmAnswer, ConfirmFuture};
use super::tasks::LocalFuture;
use crate::services::{Reply, Services, Standing};
use hlsr_core::js::{self, object::JsObject};
use hlsr_delivery::progress_states;
use serde_json::{Map, Value, json};
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::Duration;

/// Значки тостов оболочки.
const ICON_ERROR: &str = "fas fa-exclamation-triangle";
const ICON_CHECK: &str = "fas fa-check";
const ICON_INFO: &str = "fas fa-info-circle";
const ICON_NOTICE: &str = "fas fa-exclamation-circle";

/// `GHOSTING_GAME_ID` (`launcher-data.js`).
pub const GHOSTING_GAME_ID: &str = "218";

/// Подсказка после падения игры появляется через столько.
pub const CRASH_HINT_DELAY: Duration = Duration::from_millis(600);

/// `DELIVERY_IDS` (`launcher-data.js`): игра оболочки → игра доставки.
pub fn delivery_id(launcher_id: &str) -> Option<&'static str> {
    match launcher_id {
        "70" => Some("hl1-won"),
        "220" => Some("hl2-4260"),
        "218" => Some("ghosting-renewed"),
        _ => None,
    }
}

/// `launcherData.shortNames[id]`.
pub fn short_name(id: &str) -> Option<&'static str> {
    Some(match id {
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

/// `licenseGameOf(gameId)`: под чьей лицензией Steam идёт сборка — название и ключ ссылки магазина.
pub fn license_game_of(launcher_id: &str) -> (&'static str, &'static str) {
    if launcher_id == "70" {
        ("Half-Life", "steamHalfLife")
    } else {
        ("Half-Life 2", "steamHalfLife2")
    }
}

/// Что изменилось в состоянии оболочки (адаптерам — что перерисовать).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellChange {
    /// Открылось или закрылось окно, сменился его повод.
    Windows,
    /// Раздел, вкладка, игра или инструмент.
    Selection,
    Tools,
    Delivery,
    Stats,
    Online,
    Update,
    /// Сборка стала или перестала быть официальной (`build-official`).
    Build,
}

/// Окно запуска LiveSplit: игры по порядку колонки с наборами сплитов от main, игра по умолчанию,
/// выбранные раньше сплиты (`{gameId: id набора}`).
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSplitLaunch {
    pub games: Vec<LiveSplitGame>,
    pub default_game: Option<String>,
    pub chosen: Map<String, Value>,
}

/// Игра окна LiveSplit.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSplitGame {
    /// Игра оболочки («70», «218»).
    pub id: String,
    /// Игра доставки, к которой привязаны сплиты (`null` — сплитов нет).
    pub game_id: Option<String>,
    pub label: String,
    /// Наборы как их отдал main: `{id, kind, category}`.
    pub presets: Vec<Value>,
}

/// Ответ окна LiveSplit: `{game, gameId, preset}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSplitPick {
    pub game: String,
    pub game_id: Option<String>,
    pub preset: Option<String>,
}

/// Ожидание ответа окна LiveSplit (`None` — отмена или окно заменили).
pub struct LiveSplitFuture {
    rx: tokio::sync::oneshot::Receiver<Option<LiveSplitPick>>,
}

impl LiveSplitFuture {
    /// Ответ и его ожидание.
    pub fn pair() -> (
        tokio::sync::oneshot::Sender<Option<LiveSplitPick>>,
        LiveSplitFuture,
    ) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (tx, LiveSplitFuture { rx })
    }
}

impl Future for LiveSplitFuture {
    type Output = Option<LiveSplitPick>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.rx)
            .poll(cx)
            .map(|answer| answer.unwrap_or(None))
    }
}

/// Окно FFmpeg перед «Демо» (`ffmpegGate` 0.4.9): что скачается, идёт ли загрузка, ошибка.
#[derive(Debug, Clone, PartialEq)]
pub struct FfmpegGate {
    /// `demo-ffmpeg-status` → `ffmpeg`: `origins`, `pinnedVersion`, `build`, `sizeBytes`.
    pub status: Value,
    pub installing: bool,
    pub percent: u32,
    pub error: Option<String>,
}

/// Интерфейс оболочки со стороны сценариев.
pub trait ShellPort {
    /// `$t(key, ...args)`.
    fn tr(&self, key: &str, args: &[&str]) -> String;
    /// `notify(text, icon)`.
    fn notify(&self, text: &str, icon: &str);
    /// `notify(text, icon, ms)` 0.4.9: `ms` — сколько держать тост (0 — по умолчанию).
    fn notify_for(&self, text: &str, icon: &str, ms: u64) {
        let _ = ms;
        self.notify(text, icon);
    }
    /// `askConfirm(request)`.
    fn ask_confirm(&self, request: Confirm) -> ConfirmFuture;
    /// Открыть окно запуска LiveSplit; открытое прежнее отвечает «отмена».
    fn ask_livesplit(&self, request: LiveSplitLaunch) -> LiveSplitFuture;
    /// `window.hlsrElectron.clipboard.writeText(text)`.
    fn write_clipboard(&self, text: &str);
    /// `prefsStore.save(prefs)`: изменённые ключи.
    fn save_prefs(&self, changes: &Map<String, Value>);
    /// `new Date().getHours()`.
    fn hour(&self) -> u32;
    /// Поставить задачу потока интерфейса.
    fn spawn(&self, task: LocalFuture);
    /// `setTimeout(f, delay)`.
    fn after(&self, delay: Duration, f: Box<dyn FnOnce()>);
    /// Состояние оболочки изменилось.
    fn changed(&self, what: ShellChange);
}

/// Состояние оболочки (`data()` `LauncherShell.vue`), которое трогают сценарии.
#[derive(Debug, Clone)]
pub struct ShellState {
    /// «home» | «games» | «personal».
    pub section: String,
    /// «games» | «tools».
    pub active_tab: String,
    pub selected_game_id: String,
    pub selected_tool_id: String,
    /// Карточки инструментов (`ToolCard`).
    pub tools: Vec<Value>,
    pub settings_open: bool,
    pub settings_section: String,
    pub changelog_open: bool,
    pub changelog_version: Option<String>,
    /// Будущий выпуск из карточки обновления: `{version, items}`.
    pub changelog_extra: Option<Value>,
    pub game_notes_open: bool,
    pub game_notes_version: String,
    pub game_notes_data: Option<Value>,
    pub unavailable_open: bool,
    /// Окно о падении игры (`crashReport`, событие `game-crash-report`) (G3).
    pub crash_report: Option<Value>,
    /// Окно ремонта по файлам: файлы проверки (`repairRequest.files`) (G3).
    pub repair_files: Option<Rc<[Value]>>,
    /// Окно FFmpeg перед «Демо» (`ffmpegGate`) (G3).
    pub ffmpeg_gate: Option<FfmpegGate>,
    pub library_setup_open: bool,
    pub welcome_open: bool,
    pub profile_open: bool,
    pub profile_target: String,
    pub friends_open: bool,
    pub update_dismissed: bool,
    /// `updateStatus` (`updater-get-state`/`updater-state`).
    pub update_status: Value,
    pub install_path: String,
    /// Сборки по играм: `{gameId: build}`.
    pub selected_builds: JsObject<Value>,
    pub auto_update_started: BTreeSet<String>,
    pub download_settings_ready: bool,
    /// Сплиты LiveSplit по играм доставки.
    pub live_split_splits: Map<String, Value>,
    /// Статусы доставки по играм оболочки (`DeliveryStatus`).
    pub delivery: JsObject<Value>,
    pub online_snapshot: Value,
    /// `game-stats-get` по играм.
    pub stats: JsObject<Value>,
    /// Настройки лаунчера в памяти (`this.prefs`).
    pub prefs: Map<String, Value>,
    /// Окно опроса о конфиге перед первым запуском (`configSetup`, 0.4.9).
    pub config_setup_open: bool,
    /// Опрос ответил «С нуля» или «Свой», шестерёнка игры: оболочка открывает «Персонализацию» на
    /// этой игре (`configsGameId` + `section`), флаг снимает оболочка.
    pub configs_open: Option<String>,
    /// Сценарий оболочки выбрал инструмент (`onSelectTool`, предложение LiveSplit): вид
    /// переходит на его вкладку, флаг снимает оболочка.
    pub tool_open: Option<String>,
    /// `configsOpenToken`: вход на «Персонализацию» снаружи на разделе `configs_section`.
    pub configs_open_token: u32,
    /// `configsSection`: раздел окна игры, на котором открывается страница.
    pub configs_section: String,
    /// `configsImportRequested`: «У меня есть свой конфиг» — страница начинает импорт.
    pub configs_import_requested: bool,
    /// `configTourRequested`: «С нуля» — обучение стартует на странице.
    pub config_tour_requested: bool,
    /// `welcomeChecked`: проверка библиотеки и приветствие прошли (обучение может ждать).
    pub welcome_checked: bool,
    /// Официальна ли сборка (`MainProcess::build`, событие `build-official`): вход, профили,
    /// спонсорство, «Главная» и счётчики онлайна — только у официальной.
    pub build: Standing,
}

impl ShellState {
    /// Официальная сборка: онлайн-функции как в 0.4.9.
    pub fn official(&self) -> bool {
        self.build == Standing::Official
    }
}

/// То, с чем оболочка стартует.
#[derive(Debug, Clone)]
pub struct ShellInit {
    /// `prefsStore.load()`.
    pub prefs: Map<String, Value>,
    /// `library.json` (`library.get(…)`); нет — `Null`.
    pub library: Value,
    /// `DEFAULT_INSTALL_PATH` (`app-default-library-path` preload).
    pub default_install_path: String,
    /// Язык интерфейса (`currentLanguage`): «en» | «ru».
    pub language: String,
}

/// `liveSplitChoicesOf(prefs)`: до 0.4.3 в `toolSplits` лежал один набор `{livesplit: …}` — он
/// относился к Ghosting.
pub fn live_split_choices_of(prefs: &Map<String, Value>) -> Map<String, Value> {
    let mut saved = match prefs.get("toolSplits") {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    if let Some(Value::String(old)) = saved.get("livesplit").cloned() {
        if !saved.get("ghosting-renewed").is_some_and(js::truthy) {
            saved.insert("ghosting-renewed".into(), Value::String(old));
        }
        saved.shift_remove("livesplit");
    }
    saved
}

struct Inner {
    services: Services,
    port: Rc<dyn ShellPort>,
    state: RefCell<ShellState>,
    language: RefCell<String>,
    /// `hideIdSequence`: номер запроса «Скрыть SteamID», пока он в пути (0 — нет запроса).
    hide_id_sequence: Cell<u32>,
    /// `episodeCardShown` прошлого кадра нижней панели (G2).
    episode_card: Cell<bool>,
    /// Ответ окна ремонта (`repairRequest.resolve`): пути или отказ (G3).
    repair_reply: RefCell<Option<tokio::sync::oneshot::Sender<Option<Vec<String>>>>>,
    /// Вход в «Демо», когда окно FFmpeg ответит «да» (`ffmpegGate.resolve`) (G3).
    ffmpeg_enter: RefCell<Option<Box<dyn FnOnce()>>>,
    /// Ответ окна опроса (`configSetup.resolve`): выбор или «Закрыть».
    config_setup_reply: RefCell<Option<tokio::sync::oneshot::Sender<Option<String>>>>,
    /// `configSetupBusy`: опрос идёт (второй «Играть» ничего не делает).
    config_setup_busy: Cell<bool>,
}

/// Сценарии оболочки; клон — те же сценарии.
#[derive(Clone)]
pub struct ShellFlows {
    inner: Rc<Inner>,
}

/// Слабая ссылка на сценарии: адаптер окон не держит оболочку (порт держит адаптер).
#[derive(Clone)]
pub struct WeakShellFlows(std::rc::Weak<Inner>);

impl WeakShellFlows {
    pub fn upgrade(&self) -> Option<ShellFlows> {
        self.0.upgrade().map(|inner| ShellFlows { inner })
    }
}

/// Ответ канала, приведённый к значению: отказ — `None`.
type Answer = Option<Value>;

fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|value| !value.is_null())
}

fn text_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn status_is(value: &Value, status: &str) -> bool {
    text_of(value, "status") == Some(status)
}

/// `result.error.code` (ошибка — объект).
fn error_code(result: Option<&Value>) -> Option<Value> {
    let error = result.and_then(|result| result.get("error"))?;
    if !js::truthy(error) {
        return None;
    }
    error.get("code").cloned()
}

/// `String(value)` аргумента `$t`: `null`/нет значения — пусто (как `join` у метки сверки).
fn arg(value: Option<&Value>) -> String {
    value.map(js::to_string).unwrap_or_default()
}

/// Ответ `invoke` без отказа (`await` без `try` — отказ обрывает сценарий).
fn ok(reply: Reply) -> Option<Value> {
    reply.ok()
}

/// Следующий круг очереди (`.then` после `await`, отрисовка Vue).
pub(crate) struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

pub(crate) fn yield_now() -> YieldNow {
    YieldNow(false)
}

impl ShellFlows {
    /// `data()` оболочки: prefs, путь библиотеки, сплиты; `app-version` — синхронно.
    pub fn new(services: Services, port: Rc<dyn ShellPort>, init: ShellInit) -> ShellFlows {
        let current_version = services.send_sync("app-version", Vec::new());
        let library_path = init
            .library
            .get("libraryPath")
            .filter(|value| js::truthy(value))
            .map(js::to_string);
        let state = ShellState {
            section: "games".into(),
            active_tab: "games".into(),
            selected_game_id: hlsr_delivery::game_availability::DEFAULT_GAME_ID.into(),
            selected_tool_id: String::new(),
            tools: Vec::new(),
            settings_open: false,
            settings_section: "general".into(),
            changelog_open: false,
            changelog_version: None,
            changelog_extra: None,
            game_notes_open: false,
            game_notes_version: String::new(),
            game_notes_data: None,
            unavailable_open: false,
            crash_report: None,
            repair_files: None,
            ffmpeg_gate: None,
            library_setup_open: false,
            welcome_open: false,
            profile_open: false,
            profile_target: String::new(),
            friends_open: false,
            update_dismissed: false,
            update_status: json!({
                "state": "idle",
                "enabled": false,
                "currentVersion": current_version,
                "version": null,
                "releaseNotes": null,
                "progress": null,
                "error": null,
            }),
            install_path: library_path.unwrap_or(init.default_install_path),
            selected_builds: JsObject::new(),
            auto_update_started: BTreeSet::new(),
            download_settings_ready: false,
            live_split_splits: live_split_choices_of(&init.prefs),
            delivery: JsObject::new(),
            online_snapshot: json!({"status": "unknown", "launcherOnline": null, "games": {}}),
            stats: JsObject::new(),
            prefs: init.prefs,
            config_setup_open: false,
            configs_open: None,
            tool_open: None,
            configs_open_token: 0,
            configs_section: String::new(),
            configs_import_requested: false,
            config_tour_requested: false,
            welcome_checked: false,
            build: services.build(),
        };
        ShellFlows {
            inner: Rc::new(Inner {
                services,
                port,
                state: RefCell::new(state),
                language: RefCell::new(init.language),
                hide_id_sequence: Cell::new(0),
                episode_card: Cell::new(false),
                repair_reply: RefCell::default(),
                ffmpeg_enter: RefCell::default(),
                config_setup_reply: RefCell::default(),
                config_setup_busy: Cell::new(false),
            }),
        }
    }

    /// Слабая ссылка (адаптеры, порт).
    pub fn downgrade(&self) -> WeakShellFlows {
        WeakShellFlows(Rc::downgrade(&self.inner))
    }

    /// Состояние оболочки (копия).
    pub fn state(&self) -> ShellState {
        self.inner.state.borrow().clone()
    }

    /// Изменить состояние напрямую (тесты, адаптеры окон). Уведомлений нет.
    pub fn with_state<R>(&self, f: impl FnOnce(&mut ShellState) -> R) -> R {
        f(&mut self.inner.state.borrow_mut())
    }

    /// Язык интерфейса (`currentLanguage`).
    pub fn language(&self) -> String {
        self.inner.language.borrow().clone()
    }

    pub fn services(&self) -> &Services {
        &self.inner.services
    }

    fn port(&self) -> &dyn ShellPort {
        self.inner.port.as_ref()
    }

    fn tr(&self, key: &str, args: &[&str]) -> String {
        self.port().tr(key, args)
    }

    fn notify(&self, text: &str, icon: &str) {
        self.port().notify(text, icon);
    }

    fn changed(&self, what: ShellChange) {
        self.port().changed(what);
    }

    fn spawn(&self, task: impl Future<Output = ()> + 'static) {
        self.port().spawn(Box::pin(task));
    }

    /// `ipcRenderer.invoke(channel, ...args)`: вызов уходит сразу, ответ — по `await`.
    fn invoke(
        &self,
        channel: &str,
        args: Vec<Value>,
    ) -> crate::services::BoxFuture<'static, Reply> {
        self.inner.services.invoke(channel, args)
    }

    /// `ipcRenderer.invoke(...)` без ожидания ответа.
    fn fire(&self, channel: &str, args: Vec<Value>) {
        let reply = self.invoke(channel, args);
        self.spawn(async move {
            let _ = reply.await;
        });
    }

    /// `prefsStore.save(prefs)` после правки ключей: в порт — изменённые ключи в порядке prefs.
    fn save_prefs(&self, changes: Vec<(&str, Value)>) {
        let mut changed = Map::new();
        {
            let mut state = self.inner.state.borrow_mut();
            let mut keys = Vec::new();
            for (key, value) in changes {
                let value = if key == "speedLimit" {
                    normalize_speed_limit(&value)
                } else {
                    value
                };
                if state.prefs.get(key) != Some(&value) {
                    keys.push(key.to_owned());
                }
                state.prefs.insert(key.to_owned(), value);
            }
            for (key, value) in &state.prefs {
                if keys.contains(key) {
                    changed.insert(key.clone(), value.clone());
                }
            }
        }
        if !changed.is_empty() {
            self.port().save_prefs(&changed);
        }
    }

    fn pref(&self, key: &str) -> Option<Value> {
        self.inner.state.borrow().prefs.get(key).cloned()
    }

    // ---- Вычисляемые свойства ----------------------------------------------------------------

    /// `currentDelivery`.
    pub fn current_delivery(&self) -> Option<Value> {
        let state = self.inner.state.borrow();
        state
            .delivery
            .get(&state.selected_game_id)
            .filter(|status| js::truthy(status))
            .cloned()
    }

    fn current_game_id_of_delivery(&self) -> Option<Value> {
        self.current_delivery()
            .map(|status| status.get("gameId").cloned().unwrap_or(Value::Null))
    }

    /// `deliveryBusy`.
    pub fn delivery_busy(&self) -> bool {
        self.inner.state.borrow().delivery.values().any(|status| {
            js::truthy(status)
                && status
                    .get("state")
                    .and_then(Value::as_str)
                    .is_some_and(progress_states::is_busy)
        })
    }

    /// `unavailableReason`: причина главной кнопки текущей игры.
    pub fn unavailable_reason(&self) -> String {
        self.current_delivery()
            .and_then(|status| status.get("mainButton").cloned())
            .and_then(|button| button.get("reason").cloned())
            .filter(js::truthy)
            .map(|reason| js::to_string(&reason))
            .unwrap_or_else(|| "UNKNOWN_UNAVAILABLE".into())
    }

    /// `currentGameFullTitle`: «Half-Life 2: Ghosting Mod: Renewed».
    pub fn current_game_full_title(&self) -> String {
        let id = self.inner.state.borrow().selected_game_id.clone();
        let header = game_demo::header(&id, None).or_else(|| {
            game_availability::column_games()
                .first()
                .and_then(|game| game_demo::header(game.game.id, None))
        });
        match header {
            Some(header) => match header.hero_subtitle {
                Some(sub) if !sub.is_empty() => format!("{}: {sub}", header.hero_title),
                _ => header.hero_title,
            },
            None => String::new(),
        }
    }

    // ---- Тексты ошибок ----------------------------------------------------------------------

    /// `notifyDeliveryError(result)`.
    pub fn notify_delivery_error(&self, result: Option<&Value>) {
        let code = error_code(result).filter(js::truthy);
        let text = code.map(|code| {
            let key = format!("#L_D_ERR_{}", js::to_string(&code));
            let text = self.tr(&key, &[]);
            (text.clone(), !text.is_empty() && text != key)
        });
        let text = match text {
            Some((text, true)) => text,
            _ => self.tr("#L_D_ERROR", &[]),
        };
        self.notify(&text, ICON_ERROR);
    }

    /// `buildTransferError(result)`.
    fn build_transfer_error(&self, result: Option<&Value>) {
        const KNOWN: [&str; 9] = [
            "NOT_INSTALLED",
            "BAD_FORMAT",
            "NEWER_FORMAT",
            "UNKNOWN_GAME",
            "CORRUPT",
            "TOO_LARGE",
            "PATH_INVALID",
            "GAME_RUNNING",
            "BUSY",
        ];
        const PACK: [&str; 7] = [
            "PACK_BAD_FORMAT",
            "PACK_NEWER_FORMAT",
            "PACK_UNKNOWN_GAME",
            "PACK_VERSION_UNKNOWN",
            "PACK_ORIGINALS_UNAVAILABLE",
            "PACK_TOO_LARGE",
            "PACK_DEPOT_BUILD",
        ];
        let code = result
            .and_then(|result| result.get("code"))
            .and_then(Value::as_str);
        if let Some(code) = code.filter(|code| PACK.contains(code) || KNOWN.contains(code)) {
            self.notify(&self.tr(&format!("#L_BT_ERR_{code}"), &[]), ICON_ERROR);
            return;
        }
        if code == Some("FILE_LOCKED") {
            let file = result
                .and_then(|result| result.get("file"))
                .filter(|file| js::truthy(file))
                .map(js::to_string)
                .unwrap_or_default();
            self.notify(&self.tr("#L_BT_ERR_FILE_LOCKED", &[&file]), ICON_ERROR);
            return;
        }
        if result.is_some_and(|result| result.get("error").is_some_and(js::truthy)) {
            self.notify_delivery_error(result);
            return;
        }
        self.notify(&self.tr("#L_BT_ERR_IO", &[]), ICON_ERROR);
    }

    // ---- Запуск и остановка ------------------------------------------------------------------

    /// `onPlay()`.
    pub fn on_play(&self) {
        let this = self.clone();
        self.spawn(async move { this.play().await });
    }

    async fn play(&self) {
        let Some(delivery) = self.current_delivery() else {
            self.notify(&self.tr("#L_STUB_PLAY", &[]), "fas fa-play");
            return;
        };
        let game_id = delivery.get("gameId").cloned().unwrap_or(Value::Null);
        if self.offer_config_setup(&game_id).await {
            return;
        }
        if self.offer_timer().await {
            return;
        }
        // «Играть» с выбранным эпизодом есть, только когда он скачан (episode-button.js).
        let options = match self.current_episode_id() {
            Some(episode) => json!({ "episode": episode }),
            None => json!({}),
        };
        let Some(result) = ok(self
            .invoke("game-process-launch", vec![game_id, options])
            .await)
        else {
            return;
        };
        if !js::truthy(&result) || status_is(&result, "error") {
            let result = js::truthy(&result).then_some(&result);
            self.notify_delivery_error(result);
            let error = result.and_then(|result| field(result, "error")).cloned();
            let code = error.as_ref().and_then(|error| error.get("code")).cloned();
            let this = self.clone();
            self.spawn(async move { this.offer_launch_fix(code, error).await });
            return;
        }
        if status_is(&result, "already-running") {
            self.notify(&self.tr("#L_P_ALREADY_RUNNING", &[]), ICON_INFO);
        }
        self.notify_dependencies(result.get("dependencies"));
        if result
            .get("packs")
            .is_some_and(|packs| status_is(packs, "failed"))
        {
            self.notify(&self.tr("#L_P_PACKS_FAILED", &[]), ICON_ERROR);
        }
    }

    /// `notifyDependencies(dependencies)`: Bunnymod XT и инжектор перед запуском HL1.
    fn notify_dependencies(&self, dependencies: Option<&Value>) {
        let Some(Value::Array(list)) = dependencies else {
            return;
        };
        for dep in list {
            let title = arg(dep.get("title"));
            if status_is(dep, "updated") {
                let to = arg(dep.get("to"));
                self.notify(
                    &self.tr("#L_P_DEP_UPDATED", &[&title, &to]),
                    "fas fa-sync-alt",
                );
            } else if status_is(dep, "failed") {
                self.notify(&self.tr("#L_P_DEP_FAILED", &[&title]), ICON_ERROR);
            }
        }
    }

    /// `offerLaunchFix(code, error)`: окно с исправлением по коду ошибки запуска.
    async fn offer_launch_fix(&self, code: Option<Value>, error: Option<Value>) {
        let code = code.as_ref().and_then(Value::as_str).unwrap_or_default();
        let selected = self.inner.state.borrow().selected_game_id.clone();
        match code {
            "LIBRARY_PATH_CASE_CONFLICT" => {
                let details = error
                    .as_ref()
                    .and_then(|error| field(error, "details"))
                    .filter(|details| js::truthy(details))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let folder = details
                    .get("folder")
                    .filter(|value| js::truthy(value))
                    .map(js::to_string)
                    .unwrap_or_default();
                let twin = details
                    .get("twin")
                    .filter(|value| js::truthy(value))
                    .map(js::to_string);
                let copy = self
                    .confirm(Confirm {
                        title: self.tr("#L_C_CASE_CONFLICT_TITLE", &[]),
                        text: self.tr(
                            "#L_C_CASE_CONFLICT_TEXT",
                            &[&folder, twin.as_deref().unwrap_or_default()],
                        ),
                        confirm_label: Some(self.tr("#L_C_COPY_PATH", &[])),
                        ..Confirm::default()
                    })
                    .await;
                if copy && let Some(twin) = twin {
                    self.port().write_clipboard(&twin);
                    self.notify(&self.tr("#L_C_PATH_COPIED", &[]), "fas fa-clipboard-check");
                }
            }
            "STEAM_APP_NOT_OWNED" => self.offer_store(&selected).await,
            "STEAM_ELEVATED" => {
                let restart = self
                    .confirm(Confirm {
                        title: self.tr("#L_UA_STEAM_ELEVATED", &[]),
                        text: self.tr("#L_UA_STEAM_ELEVATED_T", &[]),
                        confirm_label: Some(self.tr("#L_C_RESTART_ELEVATED", &[])),
                        ..Confirm::default()
                    })
                    .await;
                if restart {
                    self.fire("game-launch-restart-elevated", Vec::new());
                }
            }
            "STEAM_NOT_RUNNING" | "STEAM_PROCESS_MISMATCH" => {
                let open = self
                    .confirm(Confirm {
                        title: self.tr("#L_C_STEAM_TITLE", &[]),
                        text: self.tr("#L_P_OPEN_STEAM_PROMPT", &[]),
                        confirm_label: Some(self.tr("#L_UA_ACT_OPEN_STEAM", &[])),
                        ..Confirm::default()
                    })
                    .await;
                if open {
                    self.fire("game-launch-open-steam", Vec::new());
                }
            }
            "SOURCE_SDK_NOT_INSTALLED" => {
                let open = self
                    .confirm(Confirm {
                        title: self.tr("#L_C_SDK_TITLE", &[]),
                        text: self.tr("#L_P_INSTALL_SDK_PROMPT", &[]),
                        confirm_label: Some(self.tr("#L_C_OPEN", &[])),
                        ..Confirm::default()
                    })
                    .await;
                if open {
                    self.fire("game-launch-install-sdk", Vec::new());
                }
            }
            _ => {}
        }
    }

    /// Лицензии нет: открыть магазин Steam.
    async fn offer_store(&self, selected: &str) {
        let (title, link) = license_game_of(selected);
        let open = self
            .confirm(Confirm {
                title: self.tr("#L_C_STEAM_LICENSE_TITLE", &[]),
                text: self.tr("#L_C_STEAM_LICENSE_TEXT", &[title]),
                confirm_label: Some(self.tr("#L_C_STEAM_STORE", &[])),
                ..Confirm::default()
            })
            .await;
        if open {
            self.fire("open-external-link", vec![json!(link)]);
        }
    }

    /// `askConfirm` → ответ «да» (`!!accepted`).
    async fn confirm(&self, request: Confirm) -> bool {
        self.port().ask_confirm(request).await.accepted()
    }

    /// `onStopGame()`.
    pub fn on_stop_game(&self) {
        let Some(game_id) = self.current_game_id_of_delivery().filter(js::truthy) else {
            return;
        };
        let reply = self.invoke("game-process-stop", vec![game_id.clone()]);
        let this = self.clone();
        self.spawn(async move {
            let Some(result) = ok(reply.await) else {
                return;
            };
            if !js::truthy(&result) || status_is(&result, "error") {
                this.notify_delivery_error(js::truthy(&result).then_some(&result));
                return;
            }
            if !status_is(&result, "timeout") {
                return;
            }
            let force = this
                .confirm(Confirm {
                    title: this.tr("#L_C_STOP_TITLE", &[]),
                    text: this.tr("#L_P_STOP_TIMEOUT", &[]),
                    confirm_label: Some(this.tr("#L_C_STOP_FORCE", &[])),
                    danger: true,
                    ..Confirm::default()
                })
                .await;
            if !force {
                return;
            }
            let Some(forced) = ok(this
                .invoke("game-process-stop", vec![game_id, json!({"force": true})])
                .await)
            else {
                return;
            };
            if !js::truthy(&forced) || status_is(&forced, "error") {
                this.notify(&this.tr("#L_P_STOP_FAILED", &[]), ICON_ERROR);
            }
        });
    }

    /// `game-process-running`.
    pub fn on_process_running(&self) {
        self.notify(&self.tr("#L_P_RUNNING", &[]), "fas fa-play");
        if self.port().hour() == 3
            && self.pref("launcherTheme").as_ref().and_then(Value::as_str) == Some("midnight")
        {
            self.unlock_achievement("night-shift");
        }
        let selected = self.inner.state.borrow().selected_game_id.clone();
        self.load_stats(&selected);
    }

    /// `game-process-exited`.
    pub fn on_process_exited(&self) {
        self.notify(&self.tr("#L_P_EXITED", &[]), ICON_CHECK);
        let selected = self.inner.state.borrow().selected_game_id.clone();
        self.load_stats(&selected);
    }

    /// `game-process-crashed`: код выхода и через 600 мс подсказка про дамп. 0.4.9: только
    /// остановка, которую попросил игрок (`requested`); настоящее падение — окно
    /// `game-crash-report` (G3).
    pub fn on_process_crashed(&self, payload: &Value) {
        if !payload.get("requested").is_some_and(js::truthy) {
            return;
        }
        let code = payload
            .get("exitCode")
            .filter(|code| !code.is_null())
            .map(js::to_string);
        let text = match &code {
            Some(code) => self.tr("#L_P_CRASHED_CODE", &[code]),
            None => self.tr("#L_P_CRASHED", &[]),
        };
        self.notify(&text, ICON_ERROR);
        let this = self.clone();
        self.port().after(
            CRASH_HINT_DELAY,
            Box::new(move || {
                this.notify(&this.tr("#L_P_CRASH_HINT", &[]), "fas fa-clipboard-check")
            }),
        );
    }

    /// `unlockAchievement(id)`: поздравление придёт событием `achievement-unlocked`.
    pub fn unlock_achievement(&self, id: &str) {
        self.fire("runner-secret-unlock", vec![json!(id)]);
    }

    /// `achievement-unlocked`.
    pub fn on_achievement_unlocked(&self, item: &Value) {
        let Some(title) = item.get("title").and_then(Value::as_str) else {
            return;
        };
        if !title.starts_with("#L_ACH_") {
            return;
        }
        let icon = item
            .get("icon")
            .and_then(Value::as_str)
            .unwrap_or("fas fa-trophy");
        let name = self.tr(title, &[]);
        self.notify(&self.tr("#L_ACH_UNLOCKED", &[&name]), icon);
    }

    // ---- Установка и обновление --------------------------------------------------------------

    /// `onInstall()`.
    pub fn on_install(&self) {
        let this = self.clone();
        self.spawn(async move { this.install().await });
    }

    fn install(&self) -> Pin<Box<dyn Future<Output = ()> + '_>> {
        Box::pin(async move {
            let Some(status) = self.current_delivery() else {
                return;
            };
            if !status.get("installable").is_some_and(js::truthy) {
                self.notify(&self.tr("#L_D_UNAVAILABLE", &[]), ICON_INFO);
                return;
            }
            let selected = self.inner.state.borrow().selected_game_id.clone();
            if status.get("requiresLicense").is_some_and(js::truthy) {
                let (title, _) = license_game_of(&selected);
                let go = self
                    .confirm(Confirm {
                        title: self.tr("#L_C_LICENSE_TITLE", &[title]),
                        text: self.tr("#L_C_LICENSE_TEXT", &[title]),
                        confirm_label: Some(self.tr("#L_C_LICENSE_GO", &[])),
                        ..Confirm::default()
                    })
                    .await;
                if !go {
                    return;
                }
            }
            let selected = self.inner.state.borrow().selected_game_id.clone();
            let Some(result) = ok(self
                .invoke("game-delivery-install", vec![json!(selected)])
                .await)
            else {
                return;
            };
            if js::truthy(&result) && status_is(&result, "error") {
                self.explain_delivery_error(&result).await;
            }
        })
    }

    /// `explainDeliveryError(result)`.
    async fn explain_delivery_error(&self, result: &Value) {
        let code = error_code(Some(result));
        let code = code.as_ref().and_then(Value::as_str).unwrap_or_default();
        let selected = self.inner.state.borrow().selected_game_id.clone();
        if code == "STEAM_APP_NOT_OWNED" {
            self.offer_store(&selected).await;
            return;
        }
        if [
            "STEAM_NOT_RUNNING",
            "STEAM_TIMEOUT",
            "LICENSE_CHECK_UNAVAILABLE",
        ]
        .contains(&code)
        {
            let (title, _) = license_game_of(&selected);
            let retry = self
                .confirm(Confirm {
                    title: self.tr("#L_C_LICENSE_UNCHECKED_TITLE", &[title]),
                    text: self.tr("#L_C_LICENSE_UNCHECKED_TEXT", &[title]),
                    confirm_label: Some(self.tr("#L_C_LICENSE_RETRY", &[])),
                    ..Confirm::default()
                })
                .await;
            if retry {
                self.on_install();
            }
            return;
        }
        self.notify_delivery_error(Some(result));
    }

    /// `onPauseDownload()`.
    pub fn on_pause_download(&self) {
        self.fire("game-delivery-pause", Vec::new());
    }

    /// `onCancelDownload(episode)`: `episode` — эпизод на паузе, main убирает архив только его.
    pub fn on_cancel_download(&self, episode: Option<&str>) {
        let game_id = self.current_game_id_of_delivery().unwrap_or(Value::Null);
        let mut args = vec![game_id];
        if let Some(episode) = episode {
            args.push(json!({ "episode": episode }));
        }
        self.fire("game-delivery-cancel", args);
    }

    /// `onResumeOperation()`: продолжить обновление или установку.
    pub fn on_resume_operation(&self) {
        let update = self
            .current_delivery()
            .is_some_and(|status| text_of(&status, "operation") == Some("update"));
        if update {
            self.on_apply_update();
        } else {
            self.on_install();
        }
    }

    /// `onApplyUpdate()`.
    pub fn on_apply_update(&self) {
        let Some(game_id) = self.current_game_id_of_delivery().filter(js::truthy) else {
            return;
        };
        let reply = self.invoke("game-menu-apply-update", vec![game_id]);
        let this = self.clone();
        self.spawn(async move {
            let Some(result) = ok(reply.await) else {
                return;
            };
            if !js::truthy(&result) || status_is(&result, "error") {
                this.notify_delivery_error(js::truthy(&result).then_some(&result));
                return;
            }
            if status_is(&result, "up-to-date") {
                this.notify(&this.tr("#L_U_G_UP_TO_DATE", &[]), ICON_CHECK);
            }
        });
    }

    // ---- Меню игры ---------------------------------------------------------------------------

    /// `onMenuAction(action)`.
    pub fn on_menu_action(&self, action: &str) {
        let game_id = self.current_game_id_of_delivery().unwrap_or(Value::Null);
        match action {
            "settings" => self.open_game_settings(),
            "shortcut" => {
                let run = self.run_menu_action(
                    "game-menu-create-shortcut",
                    game_id,
                    Some("#L_GM_SHORTCUT_DONE"),
                );
                self.spawn(async move {
                    run.await;
                });
            }
            "screenshots" => {
                let run = self.run_menu_action("game-menu-open-screenshots", game_id, None);
                self.spawn(async move {
                    run.await;
                });
            }
            "folder" => {
                let run = self.run_menu_action("game-menu-open-folder", game_id, None);
                self.spawn(async move {
                    run.await;
                });
            }
            "updates" => self.on_check_game_updates(game_id),
            "verify" => self.on_verify_files(game_id),
            "repair" => self.on_repair_game(game_id),
            "uninstall" => match self.current_episode_id() {
                Some(episode) => self.on_remove_episode(episode),
                None => self.on_uninstall_game(game_id),
            },
            "exportBuild" => self.on_export_build(game_id),
            "exportSettings" => self.on_export_settings(game_id),
            "importBuild" => self.on_import_build(game_id),
            _ => {}
        }
    }

    /// `runMenuAction(channel, gameId, successKey)`: вызов уходит сразу; ответ — `None`, если это
    /// ошибка (тост уже показан).
    fn run_menu_action(
        &self,
        channel: &str,
        game_id: Value,
        success_key: Option<&'static str>,
    ) -> impl Future<Output = Answer> + 'static {
        let reply = self.invoke(channel, vec![game_id]);
        let this = self.clone();
        async move {
            let result = match reply.await {
                Ok(result) => result,
                Err(_) => {
                    this.notify_delivery_error(None);
                    return None;
                }
            };
            if !js::truthy(&result) || status_is(&result, "error") {
                this.notify_delivery_error(js::truthy(&result).then_some(&result));
                return None;
            }
            if let Some(key) = success_key {
                this.notify(&this.tr(key, &[]), ICON_CHECK);
            }
            Some(result)
        }
    }

    /// `onCheckGameUpdates(gameId)`: обновление — только при настоящем предложении с версией.
    fn on_check_game_updates(&self, game_id: Value) {
        let run = self.run_menu_action("game-menu-check-updates", game_id, None);
        let this = self.clone();
        self.spawn(async move {
            let Some(result) = run.await else {
                return;
            };
            let update = field(&result, "update").filter(|value| js::truthy(value));
            if update.is_some_and(|update| update.get("stale").is_some_and(js::truthy)) {
                this.notify(&this.tr("#L_U_G_STALE", &[]), ICON_NOTICE);
                return;
            }
            if update.is_some_and(|update| update.get("unknown").is_some_and(js::truthy)) {
                this.notify(&this.tr("#L_U_G_OFFLINE", &[]), ICON_NOTICE);
                return;
            }
            let plan = field(&result, "plan").filter(|value| js::truthy(value));
            let target = update
                .and_then(|update| update.get("availableVersion"))
                .filter(|value| js::truthy(value));
            let up_to_date = plan.is_none_or(|plan| text_of(plan, "kind") == Some("up-to-date"));
            let offered =
                update.is_some_and(|update| update.get("updateAvailable").is_some_and(js::truthy));
            let (Some(update), Some(target), false, true) = (update, target, up_to_date, offered)
            else {
                this.notify(&this.tr("#L_GM_NO_UPDATES", &[]), ICON_CHECK);
                return;
            };
            let target = js::to_string(target);
            let key = if update.get("updateRequired").is_some_and(js::truthy) {
                "#L_U_G_REQUIRED"
            } else {
                "#L_U_G_AVAILABLE"
            };
            this.notify(&this.tr(key, &[&target]), "fas fa-download");
        });
    }

    /// `onUninstallGame(gameId)`.
    fn on_uninstall_game(&self, game_id: Value) {
        let run = self.run_menu_action("game-menu-uninstall", game_id.clone(), None);
        let this = self.clone();
        self.spawn(async move {
            let Some(plan) = run.await else {
                return;
            };
            if !status_is(&plan, "confirmation-required") {
                return;
            }
            let size = this.size_text(js::value::to_number(plan.get("totalBytes")));
            let confirmed = this
                .confirm(Confirm {
                    title: this.tr("#L_C_UNINSTALL_TITLE", &[]),
                    text: this.tr("#L_GM_UNINSTALL_CONFIRM", &[&size]),
                    note: Some(this.tr("#L_GM_UNINSTALL_NOTE", &[])),
                    confirm_label: Some(this.tr("#L_C_UNINSTALL", &[])),
                    danger: true,
                    ..Confirm::default()
                })
                .await;
            if !confirmed {
                return;
            }
            let Some(result) = ok(this
                .invoke(
                    "game-menu-uninstall",
                    vec![game_id, json!({"confirmed": true})],
                )
                .await)
            else {
                return;
            };
            if !js::truthy(&result) || status_is(&result, "error") {
                this.notify_delivery_error(js::truthy(&result).then_some(&result));
                return;
            }
            if status_is(&result, "partial") {
                this.notify(&this.tr("#L_GM_UNINSTALL_PARTIAL", &[]), ICON_NOTICE);
                return;
            }
            let freed = this.size_text(js::value::to_number(result.get("freedBytes")));
            this.notify(&this.tr("#L_GM_UNINSTALL_DONE", &[&freed]), ICON_CHECK);
        });
    }

    // ---- Эпизоды Ghosting и проверка файлов (G2) ----------------------------------------------

    /// `currentGame.id`: выбранная игра колонки, иначе первая.
    pub fn current_game_id(&self) -> String {
        let id = self.inner.state.borrow().selected_game_id.clone();
        let games = game_availability::column_games();
        games
            .iter()
            .find(|entry| entry.game.id == id)
            .or(games.first())
            .map_or(id, |entry| entry.game.id.to_owned())
    }

    /// `currentBuild`: вариант Ghosting (эпизод) помнит лаунчер (`prefs.ghostingBuild`; эпизода
    /// нет в списке — первая сборка), у остальных — выбранная сборка, иначе первая.
    pub fn current_build(&self) -> String {
        let game = self.current_game_id();
        let builds = game_demo::demo_state(&game).builds;
        let first = builds.first().copied().unwrap_or_default();
        if game == GHOSTING_GAME_ID {
            let saved = self.pref("ghostingBuild");
            let saved = saved.as_ref().and_then(Value::as_str);
            return saved
                .filter(|build| builds.contains(build))
                .unwrap_or(first)
                .to_owned();
        }
        let state = self.inner.state.borrow();
        state
            .selected_builds
            .get(&state.selected_game_id)
            .filter(|build| js::truthy(build))
            .map_or_else(|| first.to_owned(), js::to_string)
    }

    /// `currentEpisodeId`: выбранный эпизод Ghosting (`"ep1" | "ep2"`).
    pub fn current_episode_id(&self) -> Option<&'static str> {
        game_demo::demo_state(&self.current_game_id()).episode_of(&self.current_build())
    }

    /// Наблюдатель `episodeCardShown` (immediate): карточка Ghosting с эпизодами появилась —
    /// нужен их размер из манифестов. Зовёт нижняя панель на каждый свой кадр.
    /// shortcut: панель зовёт только пока видна — уход из «Игр» и возврат не спрашивают размер
    /// заново, как 0.4.9; если понадобится — звать `false` из корня при смене раздела.
    pub fn set_episode_card_shown(&self, shown: bool) {
        if shown && !self.inner.episode_card.get() {
            self.refresh_episodes();
        }
        self.inner.episode_card.set(shown);
    }

    /// `refreshEpisodes()`: размер эпизодов main кладёт в статус Ghosting и рассылает его.
    pub fn refresh_episodes(&self) {
        self.fire("game-episodes-status", Vec::new());
    }

    /// `onInstallEpisode()`: «Скачать» / «Продолжить» эпизода — какой бы ни был выбран, качаются
    /// оба: Episode Two main ставит вместе с недостающим Episode One.
    pub fn on_install_episode(&self) {
        let reply = self.invoke_or_null("game-episode-install", vec![json!("ep2")]);
        let this = self.clone();
        self.spawn(async move {
            let result = reply.await;
            if !js::truthy(&result) || status_is(&result, "error") {
                this.notify_delivery_error(js::truthy(&result).then_some(&result));
            }
            this.refresh_episodes();
        });
    }

    /// `onRemoveEpisode(id)`: Episode Two монтирует Episode One — без него он не запустится и
    /// уходит следом.
    fn on_remove_episode(&self, id: &'static str) {
        let installed = |episode: &str| {
            self.current_delivery().is_some_and(|status| {
                status
                    .get("episodes")
                    .and_then(|episodes| episodes.get(episode))
                    .and_then(|episode| episode.get("installed"))
                    .is_some_and(js::truthy)
            })
        };
        let with_two = id == "ep1" && installed("ep2");
        let build = self.current_build();
        let request = Confirm {
            title: self.tr("#L_GM_UNINSTALL_EPISODE", &[]),
            text: self.tr(
                if with_two {
                    "#L_GM_EPISODE_REMOVE_BOTH"
                } else {
                    "#L_GM_EPISODE_REMOVE"
                },
                &[&build],
            ),
            note: Some(self.tr("#L_GM_EPISODE_REMOVE_NOTE", &[])),
            confirm_label: Some(self.tr("#L_C_UNINSTALL", &[])),
            danger: true,
            ..Confirm::default()
        };
        let this = self.clone();
        self.spawn(async move {
            if !this.confirm(request).await {
                return;
            }
            let result = this
                .invoke_or_null("game-episode-remove", vec![json!(id)])
                .await;
            if !js::truthy(&result) || status_is(&result, "error") {
                this.notify_delivery_error(js::truthy(&result).then_some(&result));
            } else {
                this.notify(&this.tr("#L_GM_EPISODE_REMOVED", &[]), ICON_CHECK);
            }
            this.refresh_episodes();
        });
    }

    /// `onVerifyFiles(gameId)`: разница в стоковых cfg — не повреждение, о ней мягкая строка.
    fn on_verify_files(&self, game_id: Value) {
        let run = self.run_menu_action("game-menu-verify-files", game_id, None);
        let this = self.clone();
        self.spawn(async move {
            let Some(result) = run.await else {
                return;
            };
            let cfg = this.stock_cfg_parts(&result);
            let fine = result.get("ok").is_some_and(js::truthy);
            let text = if !fine {
                let damaged = js::to_template_string(result.get("damaged"));
                let sentence = if cfg.is_empty() {
                    String::new()
                } else {
                    format!(". {}.", this.tr("#L_GM_CFG_DIFF_N", &[&cfg]))
                };
                this.tr("#L_D_FILES_DAMAGED_N", &[&damaged]) + &sentence
            } else if !cfg.is_empty() {
                this.tr("#L_GM_FILES_OK_CFG", &[&cfg])
            } else {
                this.tr("#L_GM_NO_DAMAGE", &[])
            };
            this.notify(&text, if fine { ICON_CHECK } else { ICON_NOTICE });
        });
    }

    /// `stockCfgParts(integrity)`: только ненулевые части — «2 нет, 1 отличаются».
    fn stock_cfg_parts(&self, integrity: &Value) -> String {
        let mut parts = Vec::new();
        for (field, key) in [
            ("stockMissing", "#L_GM_CFG_MISSING_N"),
            ("configChanged", "#L_GM_CFG_DIFFER_N"),
        ] {
            if let Some(count) = integrity.get(field).filter(|count| js::truthy(count)) {
                parts.push(self.tr(key, &[&js::to_template_string(Some(count))]));
            }
        }
        parts.join(", ")
    }

    // ---- Ремонт по файлам, перенос сборки, падение игры, окно FFmpeg (G3) ---------------------

    /// `formatBytes(bytes)` 0.4.9: `formatSize` на языке окна («24,7 МБ»); не число или ≤ 0 —
    /// пусто.
    fn size_text(&self, bytes: f64) -> String {
        if !bytes.is_finite() || bytes <= 0.0 {
            return String::new();
        }
        super::boot::format_size(bytes, &|key, args| self.tr(key, args))
    }

    /// `notifyRepairError(result)`: по файлу — с его именем, «обнови игру», отмена — молча.
    fn notify_repair_error(&self, result: Option<&Value>) {
        let error = result.and_then(|result| field(result, "error"));
        let code = error.and_then(|error| text_of(error, "code"));
        let details = error.and_then(|error| field(error, "details"));
        let detail = |key: &str| details.and_then(|details| field(details, key));
        if code == Some("CANCELLED") {
            return;
        }
        if detail("reason").and_then(Value::as_str) == Some("UPDATE_AVAILABLE") {
            self.notify(
                &self.tr("#L_GM_REPAIR_UPDATE_FIRST", &[]),
                "fas fa-sync-alt",
            );
            return;
        }
        if let Some(path) = detail("path").filter(|path| js::truthy(path)) {
            let key = match code {
                Some("REPAIR_SOURCE_MISSING") => Some("#L_GM_REPAIR_FILE_UNAVAILABLE"),
                Some("CHECKSUM_MISMATCH" | "SIZE_MISMATCH") => Some("#L_GM_REPAIR_FILE_BROKEN"),
                _ => None,
            };
            if let Some(key) = key {
                self.notify(&self.tr(key, &[&js::to_string(path)]), ICON_ERROR);
                return;
            }
        }
        self.notify_delivery_error(result);
    }

    /// `askRepair(files)`: окно ремонта; открытое прежнее отвечает отказом.
    fn ask_repair(&self, files: Vec<Value>) -> impl Future<Output = Option<Vec<String>>> + 'static {
        self.on_repair_resolved(None);
        let (reply, answer) = tokio::sync::oneshot::channel();
        *self.inner.repair_reply.borrow_mut() = Some(reply);
        self.with_state(|state| state.repair_files = Some(files.into()));
        self.changed(ShellChange::Windows);
        async move { answer.await.ok().flatten() }
    }

    /// `onRepairResolved(chosen)`: ответ окна ремонта — отмеченные пути или `None`.
    pub fn on_repair_resolved(&self, chosen: Option<Vec<String>>) {
        let reply = self.inner.repair_reply.borrow_mut().take();
        if self.with_state(|state| state.repair_files.take()).is_some() {
            self.changed(ShellChange::Windows);
        }
        if let Some(reply) = reply {
            let _ = reply.send(chosen);
        }
    }

    /// `onRepairGame(gameId)`: проверка, окно с галочками, загрузка отмеченных файлов с зеркала,
    /// итог по повторной проверке и «Починить ещё раз».
    fn on_repair_game(&self, game_id: Value) {
        let reply = self.invoke_or_null("game-menu-repair", vec![game_id.clone()]);
        let this = self.clone();
        self.spawn(async move {
            let check = reply.await;
            if !js::truthy(&check) || status_is(&check, "error") {
                this.notify_repair_error(js::truthy(&check).then_some(&check));
                return;
            }
            if status_is(&check, "no-damage") {
                this.notify(&this.tr("#L_GM_NO_DAMAGE", &[]), ICON_CHECK);
                return;
            }
            if !status_is(&check, "confirmation-required") {
                return;
            }
            let files = match check.get("files") {
                Some(Value::Array(files)) => files.clone(),
                _ => Vec::new(),
            };
            let Some(files) = this
                .ask_repair(files)
                .await
                .filter(|files| !files.is_empty())
            else {
                return;
            };
            let repaired = this
                .invoke_or_null(
                    "game-menu-repair",
                    vec![
                        game_id.clone(),
                        json!({"confirmed": true, "recheck": false, "files": files}),
                    ],
                )
                .await;
            if !js::truthy(&repaired) || status_is(&repaired, "error") {
                this.notify_repair_error(js::truthy(&repaired).then_some(&repaired));
                return;
            }
            let count = |key: &str| match repaired.get(key) {
                Some(Value::Array(list)) => list.clone(),
                _ => Vec::new(),
            };
            let done = count("repaired").len().to_string();
            let remaining = count("remaining");
            if remaining.is_empty() {
                let integrity = repaired.get("integrity").cloned().unwrap_or(Value::Null);
                let cfg = this.stock_cfg_parts(&integrity);
                let cfg = if cfg.is_empty() {
                    cfg
                } else {
                    format!(". {}.", this.tr("#L_GM_CFG_DIFF_N", &[&cfg]))
                };
                this.notify(
                    &(this.tr("#L_GM_REPAIR_DONE_N", &[&done]) + &cfg),
                    ICON_CHECK,
                );
                return;
            }
            let again = this
                .confirm(Confirm {
                    title: this.tr("#L_C_REPAIR_TITLE", &[]),
                    text: this.tr(
                        "#L_GM_REPAIR_RESULT",
                        &[&done, &remaining.len().to_string()],
                    ),
                    items: remaining.iter().map(js::to_string).collect(),
                    confirm_label: Some(this.tr("#L_GM_REPAIR_AGAIN", &[])),
                    cancel_label: Some(this.tr("#L_CLOSE", &[])),
                    ..Confirm::default()
                })
                .await;
            if again {
                this.on_repair_game(game_id);
            }
        });
    }

    /// `game-crash-report`: окно о падении (`{gameId, launcherId, exitCode, archive, file, hint}`).
    pub fn on_crash_report(&self, report: &Value) {
        if !report.get("gameId").is_some_and(js::truthy) {
            return;
        }
        self.with_state(|state| state.crash_report = Some(report.clone()));
        self.changed(ShellChange::Windows);
    }

    /// Окно о падении: `close`, `open-folder`, `open-discord`, `export-diagnostics`.
    pub fn on_crash_action(&self, action: &str) {
        let report = self.with_state(|state| state.crash_report.clone());
        match action {
            "open-folder" => {
                let game = report.as_ref().and_then(|report| report.get("gameId"));
                self.on_open_crash_dump(&game.map(js::to_string).unwrap_or_default());
            }
            "open-discord" => self.on_open_discord(),
            _ => {
                self.with_state(|state| state.crash_report = None);
                self.changed(ShellChange::Windows);
                if action == "export-diagnostics" {
                    self.open_diagnostics();
                }
            }
        }
    }

    /// `onSelectDemo()` колонки: без FFmpeg — сначала окно с предложением скачать (`demoFfmpegReady`);
    /// `enter` — вход в «Демо», если можно. `in_demo` — «Демо» уже открыто.
    pub fn on_select_demo(&self, in_demo: bool, enter: impl FnOnce() + 'static) {
        if self.with_state(|state| state.ffmpeg_gate.is_some()) {
            return;
        }
        if in_demo {
            enter();
            return;
        }
        let reply = self.invoke_or_null("demo-ffmpeg-status", Vec::new());
        let this = self.clone();
        self.spawn(async move {
            let answer = reply.await;
            let status = status_is(&answer, "ok")
                .then(|| field(&answer, "ffmpeg").cloned())
                .flatten()
                .filter(js::truthy);
            let gate = status.filter(|status| {
                text_of(status, "state") == Some("not-installed")
                    && status.get("installable").is_some_and(js::truthy)
            });
            // Со вторым щелчком, пока ждали ответ, окно уже могло открыться.
            if this.with_state(|state| state.ffmpeg_gate.is_some()) {
                return;
            }
            let Some(status) = gate else {
                enter();
                return;
            };
            *this.inner.ffmpeg_enter.borrow_mut() = Some(Box::new(enter));
            this.with_state(|state| {
                state.ffmpeg_gate = Some(FfmpegGate {
                    status,
                    installing: false,
                    percent: 0,
                    error: None,
                })
            });
            this.changed(ShellChange::Windows);
        });
    }

    /// `closeFfmpegGate(entered)`.
    fn close_ffmpeg_gate(&self, entered: bool) {
        let enter = self.inner.ffmpeg_enter.borrow_mut().take();
        if self.with_state(|state| state.ffmpeg_gate.take()).is_some() {
            self.changed(ShellChange::Windows);
        }
        if let (true, Some(enter)) = (entered, enter) {
            enter();
        }
    }

    /// `onFfmpegGate(accepted)`: «Скачать» ставит FFmpeg; «Позже» закрывает; «Отменить загрузку»
    /// и Esc во время загрузки — `demo-ffmpeg-cancel` (окно закроет ответ установки).
    pub fn on_ffmpeg_gate(&self, accepted: bool) {
        let Some(gate) = self.with_state(|state| state.ffmpeg_gate.clone()) else {
            return;
        };
        if !accepted {
            if gate.installing {
                self.fire("demo-ffmpeg-cancel", Vec::new());
            } else {
                self.close_ffmpeg_gate(false);
            }
            return;
        }
        if gate.installing {
            return;
        }
        self.with_state(|state| {
            if let Some(gate) = &mut state.ffmpeg_gate {
                gate.installing = true;
                gate.percent = 0;
                gate.error = None;
            }
        });
        self.changed(ShellChange::Windows);
        let reply = self.invoke_or_null("demo-ffmpeg-install", Vec::new());
        let this = self.clone();
        self.spawn(async move {
            let answer = reply.await;
            this.with_state(|state| {
                if let Some(gate) = &mut state.ffmpeg_gate {
                    gate.installing = false;
                }
            });
            if status_is(&answer, "ok") {
                return this.close_ffmpeg_gate(true);
            }
            if status_is(&answer, "cancelled") {
                return this.close_ffmpeg_gate(false);
            }
            let error = this.ffmpeg_error(&answer);
            this.with_state(|state| {
                if let Some(gate) = &mut state.ffmpeg_gate {
                    gate.error = Some(error);
                }
            });
            this.changed(ShellChange::Windows);
        });
    }

    /// `demoErrorMessage(answer || {code: "DEMO_FAILED"})` для установки FFmpeg.
    /// shortcut: только коды установки инструмента; полная таблица `ERROR_KEYS` demo-tool.js —
    /// задача G11 (`demo_tool.rs`), тогда звать её.
    fn ffmpeg_error(&self, answer: &Value) -> String {
        let code = text_of(answer, "code").unwrap_or("DEMO_FAILED");
        let key = match code {
            "NETWORK_ERROR" | "NETWORK_TIMEOUT" => "#L_DEMO_ERR_NETWORK",
            "SIZE_MISMATCH"
            | "CHECKSUM_MISMATCH"
            | "EXTRACT_FAILED"
            | "ARCHIVE_LIMIT_EXCEEDED"
            | "ARCHIVE_LIST_FAILED"
            | "ARCHIVE_UNSAFE_ENTRY" => "#L_DEMO_ERR_DOWNLOAD",
            "ANTIVIRUS_BLOCKED" => "#L_DEMO_ERR_ANTIVIRUS",
            "PATH_NOT_WRITABLE" => "#L_DEMO_ERR_TOOLS_DIR",
            "ALREADY_RUNNING" => "#L_DEMO_ERR_INSTALLING",
            "FFMPEG_INSTALLING" => "#L_DEMO_ERR_FFMPEG_INSTALLING",
            "FFMPEG_NOT_IN_PATH" | "FFMPEG_INSTALL_UNSUPPORTED" => "#L_DEMO_ERR_FFMPEG_PATH",
            "FFMPEG_BROKEN" => "#L_DEMO_ERR_FFMPEG_BROKEN",
            "FFMPEG_VERSION_MISMATCH" => "#L_DEMO_ERR_FFMPEG_VERSION",
            "NO_SPACE" => "#L_DEMO_ERR_NO_SPACE",
            "TOOLS_UNAVAILABLE" => "#L_DEMO_ERR_TOOLS",
            "UNTRUSTED" => "#L_DEMO_ERR_UNTRUSTED",
            "REMOTE_DISABLED" => "#L_DEMO_ERR_REMOTE_DISABLED",
            "MANIFEST_INVALID" => "#L_DEMO_ERR_MANIFEST",
            "CANCELLED" => "#L_DEMO_ERR_CANCELLED",
            "DEMO_FAILED" | "JOB_FAILED" => "#L_DEMO_ERR_FAILED",
            other => return self.tr("#L_DEMO_ERR_GENERIC", &[other]),
        };
        self.tr(key, &[])
    }

    /// `demo-ffmpeg-progress`: процент загрузки в окне.
    pub fn on_ffmpeg_progress(&self, payload: &Value) {
        let percent = js::value::to_number(payload.get("percent"));
        let percent = if percent.is_nan() { 0. } else { percent };
        let changed = self.with_state(|state| match &mut state.ffmpeg_gate {
            Some(gate) if gate.installing && js::truthy(payload) => {
                gate.percent = percent.round().clamp(0., 100.) as u32;
                true
            }
            _ => false,
        });
        if changed {
            self.changed(ShellChange::Windows);
        }
    }

    /// `ffmpegGateItems`: откуда скачается (источник и запасной), сборка, размер.
    pub fn ffmpeg_gate_items(&self, status: &Value) -> Vec<String> {
        let origins: Vec<&Value> = match status.get("origins") {
            Some(Value::Array(origins)) => origins.iter().filter(|o| js::truthy(o)).collect(),
            _ => Vec::new(),
        };
        let text = |value: Option<&Value>| arg(value);
        let mut items = Vec::new();
        if let Some(first) = origins.first() {
            let site = text(first.get("site"));
            items.push(match first.get("release").filter(|r| js::truthy(r)) {
                Some(release) => self.tr("#L_DEMO_GATE_FROM", &[&site, &js::to_string(release)]),
                None => self.tr("#L_DEMO_GATE_FROM_SITE", &[&site]),
            });
        }
        if let Some(spare) = origins.get(1) {
            items.push(self.tr("#L_DEMO_GATE_SPARE", &[&text(spare.get("site"))]));
        }
        let or_empty = |key: &str| {
            status
                .get(key)
                .filter(|value| js::truthy(value))
                .map(js::to_string)
                .unwrap_or_default()
        };
        items.push(self.tr(
            "#L_DEMO_GATE_BUILD",
            &[&or_empty("pinnedVersion"), &or_empty("build")],
        ));
        let size = js::value::to_number(status.get("sizeBytes"));
        items.push(self.tr("#L_DEMO_GATE_SIZE", &[&self.size_text(size)]));
        items
    }

    /// `invoke(...).catch(() => null)`.
    fn invoke_or_null(
        &self,
        channel: &str,
        args: Vec<Value>,
    ) -> impl Future<Output = Value> + 'static {
        let reply = self.invoke(channel, args);
        async move { reply.await.unwrap_or(Value::Null) }
    }

    /// `confirmPackExport(request)` 0.4.9: в сборке изменены файлы (свой HUD поверх файлов
    /// игры) — архив сделать можно, при импорте они переедут, оригиналы скачаются.
    async fn confirm_pack_export(&self, request: &Value) -> Value {
        let key = if request.get("custom").is_some_and(js::truthy) {
            "#L_BT_PACK_MODIFIED_CUSTOM"
        } else {
            "#L_BT_PACK_MODIFIED_BACKUP"
        };
        let go = self
            .confirm(Confirm {
                title: self.tr("#L_BT_PACK_MODIFIED_TITLE", &[]),
                text: self.tr(key, &[&js::to_template_string(request.get("modified"))]),
                note: Some(self.tr("#L_BT_PACK_MODIFIED_NOTE", &[])),
                confirm_label: Some(self.tr("#L_BT_PACK_MODIFIED_OK", &[])),
                ..Confirm::default()
            })
            .await;
        if !go {
            return Value::Null;
        }
        let token = request.get("token").cloned().unwrap_or(Value::Null);
        self.invoke_or_null("game-build-export-confirm", vec![token])
            .await
    }

    /// `onExportBuild(gameId)`: полная сборка в архив.
    fn on_export_build(&self, game_id: Value) {
        let reply = self.invoke_or_null("game-build-export", vec![game_id]);
        let this = self.clone();
        self.spawn(async move {
            let mut result = reply.await;
            if js::truthy(&result) && status_is(&result, "confirmation-required") {
                result = this.confirm_pack_export(&result).await;
                if !js::truthy(&result) {
                    return;
                }
            }
            let truthy = js::truthy(&result);
            if truthy && status_is(&result, "canceled") {
                return;
            }
            if !truthy || !status_is(&result, "ok") {
                this.build_transfer_error(truthy.then_some(&result));
                return;
            }
            if text_of(&result, "kind") == Some("pack") {
                let size = this.size_text(js::value::to_number(result.get("size")));
                this.notify(
                    &this.tr("#L_BT_PACK_EXPORTED", &[&size]),
                    "fas fa-file-archive",
                );
            } else {
                let files = js::to_template_string(result.get("files"));
                this.notify(&this.tr("#L_BT_EXPORTED", &[&files]), "fas fa-file-export");
            }
        });
    }

    /// `onExportSettings(gameId)`.
    fn on_export_settings(&self, game_id: Value) {
        let reply = self.invoke_or_null("game-build-export-settings", vec![game_id]);
        let this = self.clone();
        self.spawn(async move {
            let result = reply.await;
            let truthy = js::truthy(&result);
            if truthy && status_is(&result, "canceled") {
                return;
            }
            if !truthy || !status_is(&result, "ok") {
                this.build_transfer_error(truthy.then_some(&result));
                return;
            }
            let files = js::to_template_string(result.get("files"));
            this.notify(&this.tr("#L_BT_EXPORTED", &[&files]), "fas fa-file-export");
        });
    }

    /// `confirmPackImport(request)`: архив заменит установленную версию.
    async fn confirm_pack_import(&self, request: &Value) -> Value {
        let installed = arg(request.get("installedVersion"));
        let version = arg(request.get("version"));
        let replace = self
            .confirm(Confirm {
                title: self.tr("#L_BT_PACK_REPLACE_TITLE", &[]),
                text: self.tr("#L_BT_PACK_REPLACE", &[&installed, &version]),
                note: Some(self.tr("#L_BT_PACK_REPLACE_NOTE", &[])),
                confirm_label: Some(self.tr("#L_BT_PACK_REPLACE_OK", &[])),
                danger: true,
                ..Confirm::default()
            })
            .await;
        if !replace {
            return Value::Null;
        }
        let token = request.get("token").cloned().unwrap_or(Value::Null);
        self.invoke_or_null("game-build-import-confirm", vec![token])
            .await
    }

    /// `onImportBuild(gameId)`: после импорта сборка проверяет обновление и ставит его.
    fn on_import_build(&self, game_id: Value) {
        let reply = self.invoke_or_null("game-build-import", vec![game_id]);
        let this = self.clone();
        self.spawn(async move {
            let mut result = reply.await;
            if js::truthy(&result) && status_is(&result, "confirmation-required") {
                result = this.confirm_pack_import(&result).await;
            }
            if !js::truthy(&result) || status_is(&result, "canceled") {
                return;
            }
            if !status_is(&result, "ok") {
                this.build_transfer_error(Some(&result));
                return;
            }
            if let Some(launcher) = result.get("launcherId").filter(|value| js::truthy(value)) {
                let launcher = js::to_string(launcher);
                if game_availability::is_available(&launcher) {
                    this.with_state(|state| state.selected_game_id = launcher.clone());
                    this.changed(ShellChange::Selection);
                    this.fire("game-delivery-selected", vec![json!(launcher)]);
                }
            }
            if text_of(&result, "kind") == Some("pack") {
                // Итог 0.4.9: изменённые файлы — сколько, куда легли, сколько оригиналов
                // скачано (`importPack`); с ними тост держится 15 с.
                let mut parts =
                    vec![this.tr("#L_BT_PACK_IMPORTED", &[&arg(result.get("version"))])];
                let modified = result.get("modified").filter(|value| js::truthy(value));
                if let Some(modified) = modified {
                    let n = |key: &str| js::to_template_string(modified.get(key));
                    let some = |key: &str| modified.get(key).is_some_and(js::truthy);
                    parts.push(this.tr("#L_BT_PACK_RESTORED", &[&n("changed"), &n("restored")]));
                    if some("custom") {
                        let dir = arg(modified.get("customDir"));
                        parts.push(this.tr("#L_BT_PACK_MOVED_CUSTOM", &[&n("custom"), &dir]));
                    }
                    if some("backup") {
                        let dir = arg(modified.get("backupDir"));
                        parts.push(this.tr("#L_BT_PACK_MOVED_BACKUP", &[&n("backup"), &dir]));
                    }
                    if some("rejected") {
                        parts.push(this.tr("#L_BT_PACK_REJECTED", &[&n("rejected")]));
                    }
                }
                if let Some(dropped) = result.get("dropped").filter(|value| js::truthy(value)) {
                    parts.push(this.tr("#L_BT_PACK_DROPPED", &[&js::to_string(dropped)]));
                }
                let ms = if modified.is_some() { 15_000 } else { 0 };
                this.port()
                    .notify_for(&parts.join(" "), "fas fa-file-archive", ms);
                return;
            }
            if !result.get("installed").is_some_and(js::truthy) {
                this.notify(&this.tr("#L_BT_IMPORT_PENDING", &[]), "fas fa-file-import");
                return;
            }
            let files = js::to_template_string(result.get("files"));
            this.notify(&this.tr("#L_BT_IMPORTED", &[&files]), "fas fa-file-import");

            let target = result.get("gameId").cloned().unwrap_or(Value::Null);
            let check = this
                .invoke_or_null("game-menu-check-updates", vec![target.clone()])
                .await;
            let update = (js::truthy(&check) && !status_is(&check, "error"))
                .then(|| check.get("update").cloned())
                .flatten()
                .filter(js::truthy);
            let Some(update) = update else {
                return;
            };
            let plan = check.get("plan").filter(|plan| js::truthy(plan));
            if !update.get("updateAvailable").is_some_and(js::truthy)
                || plan.is_none_or(|plan| text_of(plan, "kind") == Some("up-to-date"))
            {
                return;
            }
            let version = update
                .get("availableVersion")
                .filter(|value| js::truthy(value))
                .map(js::to_string)
                .unwrap_or_default();
            this.notify(&this.tr("#L_BT_UPDATING", &[&version]), "fas fa-download");
            let applied = this
                .invoke_or_null("game-menu-apply-update", vec![target])
                .await;
            if !js::truthy(&applied) || status_is(&applied, "error") {
                this.notify_delivery_error(js::truthy(&applied).then_some(&applied));
            }
        });
    }

    /// `game-build-import-applied`: cfg из сборки легли после установки.
    pub fn on_build_import_applied(&self, payload: &Value) {
        if !js::truthy(payload) {
            return;
        }
        if let Some(error) = payload.get("error").filter(|error| js::truthy(error)) {
            self.build_transfer_error(Some(&json!({"code": error})));
        } else {
            let files = js::to_template_string(payload.get("files"));
            self.notify(&self.tr("#L_BT_IMPORT_APPLIED", &[&files]), ICON_CHECK);
        }
    }

    // ---- Выбор игры и сборки -----------------------------------------------------------------

    /// `onSelectGame(id)` (колонка значков).
    pub fn on_select_game(&self, id: &str) {
        if !game_availability::is_available(id) {
            self.notify(&self.tr("#L_GAME_COMING_SOON", &[]), "fas fa-clock");
            return;
        }
        self.with_state(|state| {
            state.section = "games".into();
            state.active_tab = "games".into();
            state.selected_game_id = id.to_owned();
        });
        self.changed(ShellChange::Selection);
        self.fire("game-delivery-selected", vec![json!(id)]);
        self.load_stats(id);
    }

    /// `onSelectBuild(build)`.
    pub fn on_select_build(&self, build: &str) {
        let selected = self.inner.state.borrow().selected_game_id.clone();
        // У Ghosting одна сборка, эпизоды — её дополнения: main сборок для них не знает.
        if selected == GHOSTING_GAME_ID {
            self.save_prefs(vec![("ghostingBuild", json!(build))]);
            self.changed(ShellChange::Selection);
            return;
        }
        let reply = self.invoke("game-select-build", vec![json!(selected), json!(build)]);
        let build = build.to_owned();
        let this = self.clone();
        self.spawn(async move {
            let Some(result) = ok(reply.await) else {
                return;
            };
            if status_is(&result, "unknown-build") {
                return;
            }
            if status_is(&result, "busy") {
                this.notify(&this.tr("#L_D_BUILD_LOCKED", &[]), "fas fa-lock");
                return;
            }
            let selected = this.inner.state.borrow().selected_game_id.clone();
            this.with_state(|state| {
                state.selected_builds.insert(selected, Value::String(build));
            });
            this.changed(ShellChange::Selection);
            if let Some(delivery) = result.get("delivery").filter(|value| js::truthy(value)) {
                this.on_delivery_status(delivery);
            }
        });
    }

    /// `select-game` из argv второго запуска.
    pub fn on_select_game_from_args(&self, payload: &Value) {
        let Some(launcher) = payload.get("launcherId").filter(|value| js::truthy(value)) else {
            return;
        };
        let id = js::to_string(launcher);
        if game_availability::is_available(&id) {
            self.with_state(|state| state.selected_game_id = id);
            self.changed(ShellChange::Selection);
        }
    }

    /// `runner-profile-state` (`onRunnerProfileUpdated`): вход, выход и синхронизация профиля
    /// меняют источник статистики (снимок сервера или своя) — статистика выбранной игры заново.
    pub fn on_runner_profile_updated(&self) {
        let selected = self.inner.state.borrow().selected_game_id.clone();
        self.load_stats(&selected);
    }

    /// `loadStats(gameId)`.
    pub fn load_stats(&self, game_id: &str) {
        let reply = self.invoke("game-stats-get", vec![json!(game_id)]);
        let game_id = game_id.to_owned();
        let this = self.clone();
        self.spawn(async move {
            let Some(summary) = ok(reply.await) else {
                return;
            };
            let summary = if js::truthy(&summary) {
                summary
            } else {
                json!({"available": false})
            };
            this.with_state(|state| {
                state.stats.insert(game_id, summary);
            });
            this.changed(ShellChange::Stats);
        });
    }

    // ---- Статусы доставки --------------------------------------------------------------------

    /// `game-delivery-status`.
    pub fn on_delivery_status(&self, status: &Value) {
        let Some(launcher) = status.get("launcherId").filter(|value| js::truthy(value)) else {
            return;
        };
        let launcher = js::to_string(launcher);
        self.with_state(|state| {
            state.delivery.insert(launcher, status.clone());
        });
        self.changed(ShellChange::Delivery);
        self.maybe_auto_update(status);
    }

    /// `maybeAutoUpdate(status)`: игра с кнопкой «Обновить» обновляется сама, один раз на версию.
    fn maybe_auto_update(&self, status: &Value) {
        if !self.inner.state.borrow().download_settings_ready {
            return;
        }
        let Some(update) = status.get("update").filter(|update| js::truthy(update)) else {
            return;
        };
        let flag = |key: &str| update.get(key).is_some_and(js::truthy);
        if !flag("updateAvailable") || flag("stale") || flag("unknown") {
            return;
        }
        let button = status.get("mainButton").filter(|button| js::truthy(button));
        if button.and_then(|button| text_of(button, "action")) != Some("update") {
            return;
        }
        if status
            .get("process")
            .filter(|process| js::truthy(process))
            .is_some_and(|process| process.get("running").is_some_and(js::truthy))
        {
            return;
        }
        let version = update
            .get("availableVersion")
            .filter(|value| js::truthy(value));
        let game_id = status.get("gameId").filter(|value| js::truthy(value));
        let (Some(version), Some(game_id)) = (version, game_id) else {
            return;
        };
        let key = format!("{}:{}", js::to_string(game_id), js::to_string(version));
        let fresh = self.with_state(|state| state.auto_update_started.insert(key));
        if !fresh {
            return;
        }
        // Ошибка придёт обычным статусом доставки.
        self.fire("game-menu-apply-update", vec![game_id.clone()]);
    }

    /// `loadDeliveryStatuses()`: статусы всех игр, незавершённые операции, перенос библиотеки.
    pub fn load_delivery_statuses(&self) {
        let reply = self.invoke("game-delivery-info", Vec::new());
        let this = self.clone();
        self.spawn(async move {
            let Some(info) = ok(reply.await) else {
                return;
            };
            if !js::truthy(&info) {
                return;
            }
            let Some(statuses) = ok(this
                .invoke("game-delivery-get-status", vec![Value::Null])
                .await)
            else {
                return;
            };
            if let Value::Array(list) = &statuses {
                for status in list {
                    this.on_delivery_status(status);
                }
            }
            let non_empty = |key: &str| match info.get(key) {
                Some(Value::Array(list)) => !list.is_empty(),
                _ => false,
            };
            if non_empty("pending") {
                this.notify(&this.tr("#L_D_PENDING", &[]), ICON_NOTICE);
            }
            if non_empty("legacyLayout") {
                let entries = info.get("legacyLayout").cloned().unwrap_or(Value::Null);
                this.offer_library_migration(&entries).await;
            }
        });
    }

    /// `offerLibraryMigration(entries)`: игры в старой раскладке папок.
    async fn offer_library_migration(&self, entries: &Value) {
        let ids: Vec<String> = match entries {
            Value::Array(list) => list.iter().map(|entry| arg(entry.get("gameId"))).collect(),
            _ => Vec::new(),
        };
        let names = ids.join(", ");
        let migrate = self
            .confirm(Confirm {
                title: self.tr("#L_C_MIGRATE_TITLE", &[]),
                text: self.tr("#L_MIGRATE_CONFIRM", &[&names]),
                items: ids,
                confirm_label: Some(self.tr("#L_C_MIGRATE", &[])),
                ..Confirm::default()
            })
            .await;
        if !migrate {
            return;
        }
        let Some(result) = ok(self.invoke("game-library-migrate", Vec::new()).await) else {
            return;
        };
        if js::truthy(&result) && status_is(&result, "ok") {
            self.notify(&self.tr("#L_MIGRATE_DONE", &[]), ICON_CHECK);
        } else {
            self.notify_delivery_error(js::truthy(&result).then_some(&result));
        }
    }

    // ---- Колонка: нижние кнопки --------------------------------------------------------------

    /// `onTool(id)`: настройки, журнал изменений, Discord, папка игр.
    pub fn on_tool(&self, id: &str) {
        match id {
            "settings" => {
                self.with_state(|state| state.settings_open = true);
                self.changed(ShellChange::Windows);
            }
            "changelog" => self.open_changelog(),
            "discord" => self.fire("open-external-link", vec![json!("discord")]),
            "folder" => {
                let reply = self.invoke("open-games-folder", Vec::new());
                let this = self.clone();
                self.spawn(async move {
                    let Some(result) = ok(reply.await) else {
                        return;
                    };
                    if !js::truthy(&result) || !status_is(&result, "ok") {
                        let code = result
                            .get("code")
                            .filter(|code| js::truthy(code))
                            .map(js::to_string)
                            .unwrap_or_else(|| "UNKNOWN".into());
                        this.notify(&this.tr("#L_FOLDER_ERROR", &[&code]), ICON_ERROR);
                    }
                });
            }
            _ => self.notify(&self.tr("#L_STUB_FEATURE", &[]), ICON_INFO),
        }
    }

    /// Журнал изменений открыт: секретное достижение «Летописец» (`watch: changelogOpen`).
    fn open_changelog(&self) {
        let was = self.with_state(|state| std::mem::replace(&mut state.changelog_open, true));
        // Наблюдатель Vue идёт до отрисовки окна (flush: "pre"): достижение — раньше `data()`
        // журнала (`sendSync app-version`).
        if !was {
            self.unlock_achievement("chronicler");
        }
        self.changed(ShellChange::Windows);
    }

    /// `openGameSettings()` 0.4.9: шестерёнка игры открывает «Персонализацию» на выбранной игре,
    /// раздел «Конфиг».
    pub fn open_game_settings(&self) {
        self.with_state(|state| {
            state.configs_open = Some(state.selected_game_id.clone());
            state.configs_open_token += 1;
            state.configs_section = "configs".into();
        });
        self.changed(ShellChange::Selection);
    }

    /// `openDiagnostics()`.
    pub fn open_diagnostics(&self) {
        self.with_state(|state| {
            state.settings_section = "diagnostics".into();
            state.settings_open = true;
        });
        self.changed(ShellChange::Windows);
    }

    /// HLSR-CE: «Свои моды» в колонке — настройки на разделе модов.
    pub fn open_mods(&self) {
        self.with_state(|state| {
            state.settings_section = "mods".into();
            state.settings_open = true;
        });
        self.changed(ShellChange::Windows);
    }

    /// `closeSettings()`.
    pub fn close_settings(&self) {
        self.with_state(|state| {
            state.settings_open = false;
            state.settings_section = "general".into();
        });
        self.changed(ShellChange::Windows);
    }

    /// `closeChangelog()`.
    pub fn close_changelog(&self) {
        self.with_state(|state| {
            state.changelog_open = false;
            state.changelog_version = None;
            state.changelog_extra = None;
        });
        self.changed(ShellChange::Windows);
    }

    /// Закрыть окно «Недоступно», заметки версии, приветствие и т. п. (`@close`); «settings» —
    /// `settingsOpen = false` из `applyPrefs` (раздел остаётся).
    pub fn close_window(&self, window: &str) {
        self.with_state(|state| match window {
            "unavailable" => state.unavailable_open = false,
            "game-notes" => state.game_notes_open = false,
            "settings" => state.settings_open = false,
            "profile" => state.profile_open = false,
            "friends" => state.friends_open = false,
            _ => {}
        });
        self.changed(ShellChange::Windows);
    }

    /// `@explain` главной кнопки: окно «Недоступно».
    pub fn explain(&self) {
        self.with_state(|state| state.unavailable_open = true);
        self.changed(ShellChange::Windows);
    }

    /// `onGameSettingsSaved()`.
    pub fn on_game_settings_saved(&self) {
        self.notify(&self.tr("#L_GS_SAVED", &[]), ICON_CHECK);
    }

    // ---- Окно «Недоступно» -------------------------------------------------------------------

    /// `onUnavailableAction(actionId)`.
    pub fn on_unavailable_action(&self, action: &str) {
        let game_id = self.inner.state.borrow().selected_game_id.clone();
        match action {
            "recheck" => {
                let reply = self.invoke("game-delivery-selected", vec![json!(game_id)]);
                let this = self.clone();
                self.spawn(async move {
                    if reply.await.is_ok() {
                        this.notify(&this.tr("#L_UA_RECHECKED", &[]), "fas fa-sync-alt");
                    }
                });
            }
            "open-steam" => self.fire("game-launch-open-steam", Vec::new()),
            "install-sdk" => self.fire("game-launch-install-sdk", Vec::new()),
            "open-library-settings" => {
                self.with_state(|state| {
                    state.unavailable_open = false;
                    state.settings_section = "downloads".into();
                    state.settings_open = true;
                });
                self.changed(ShellChange::Windows);
            }
            "verify-files" => {
                self.with_state(|state| state.unavailable_open = false);
                self.changed(ShellChange::Windows);
                self.on_menu_action("verify");
            }
            "repair" => {
                self.with_state(|state| state.unavailable_open = false);
                self.changed(ShellChange::Windows);
                self.on_menu_action("repair");
            }
            "open-diagnostics" => {
                self.with_state(|state| state.unavailable_open = false);
                self.open_diagnostics();
            }
            "open-discord" => self.fire("open-discord", Vec::new()),
            "check-launcher-update" => {
                self.with_state(|state| state.unavailable_open = false);
                self.fire("updater-check", Vec::new());
                self.with_state(|state| {
                    state.settings_section = "about".into();
                    state.settings_open = true;
                });
                self.changed(ShellChange::Windows);
                self.notify(
                    &self.tr("#L_UA_LAUNCHER_UPDATE_CHECK", &[]),
                    "fas fa-arrow-up",
                );
            }
            _ => {}
        }
    }

    // ---- Что нового, обновления из колокольчика, дамп ----------------------------------------

    /// `onWhatsNew(payload)`: журнал изменений лаунчера или заметки версии игры.
    pub fn on_whats_new(&self, payload: &Value) {
        let target = payload.get("target").and_then(Value::as_str);
        let version = payload
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if target == Some("launcher") {
            let extra = {
                let state = self.inner.state.borrow();
                let status = &state.update_status;
                let notes = status.get("releaseNotes").filter(|notes| js::truthy(notes));
                match (&version, notes) {
                    (Some(version), Some(notes))
                        if !version.is_empty()
                            && status.get("version").and_then(Value::as_str)
                                == Some(version.as_str()) =>
                    {
                        let items: Vec<Value> = js::to_string(notes)
                            .split('\n')
                            .map(js::trim)
                            .filter(|line| !line.is_empty())
                            .map(|line| json!(line))
                            .collect();
                        Some(json!({"version": version, "items": items}))
                    }
                    _ => None,
                }
            };
            self.with_state(|state| {
                state.changelog_version = version;
                state.changelog_extra = extra;
            });
            self.open_changelog();
            return;
        }
        let Some(version) = version.filter(|version| !version.is_empty()) else {
            return;
        };
        if target != Some("game") {
            return;
        }
        let game_id = payload
            .get("gameId")
            .filter(|value| js::truthy(value))
            .cloned()
            .or_else(|| self.current_game_id_of_delivery())
            .unwrap_or(Value::Null);
        let reply = self.invoke("game-release-notes", vec![game_id, json!(version)]);
        let this = self.clone();
        self.spawn(async move {
            let Some(notes) = ok(reply.await) else {
                return;
            };
            this.with_state(|state| {
                state.game_notes_version = version;
                state.game_notes_data = js::truthy(&notes).then_some(notes);
                state.game_notes_open = true;
            });
            this.changed(ShellChange::Windows);
        });
    }

    /// `onRunUpdateAction(payload)`: кнопка «Обновить» в уведомлении.
    pub fn on_run_update_action(&self, payload: &Value) {
        match payload.get("target").and_then(Value::as_str) {
            Some("proton") => {
                self.with_state(|state| {
                    state.settings_section = "proton".into();
                    state.settings_open = true;
                });
                self.changed(ShellChange::Windows);
                let reply = self.invoke("proton-update", Vec::new());
                let this = self.clone();
                self.spawn(async move {
                    if let Err(message) = reply.await {
                        let text = format!("{}: {message}", this.tr("#L_S_PROTON_ERROR", &[]));
                        this.notify(&text, ICON_ERROR);
                    }
                });
            }
            Some("launcher") => {
                self.with_state(|state| state.update_dismissed = false);
                self.changed(ShellChange::Update);
                self.start_update_download();
            }
            Some("game") => {
                let game_id = payload
                    .get("gameId")
                    .filter(|value| js::truthy(value))
                    .cloned()
                    .or_else(|| self.current_game_id_of_delivery().filter(js::truthy));
                let Some(game_id) = game_id else {
                    return;
                };
                let reply = self.invoke("game-menu-apply-update", vec![game_id]);
                let this = self.clone();
                self.spawn(async move {
                    let Some(result) = ok(reply.await) else {
                        return;
                    };
                    if !js::truthy(&result) || status_is(&result, "error") {
                        this.notify_delivery_error(js::truthy(&result).then_some(&result));
                    }
                });
            }
            _ => {}
        }
    }

    /// `onOpenCrashDump(gameId)`: без игры — последнее падение.
    pub fn on_open_crash_dump(&self, game_id: &str) {
        let target = if game_id.is_empty() {
            Value::Null
        } else {
            json!(game_id)
        };
        let reply = self.invoke("game-crash-open-dump", vec![target]);
        let this = self.clone();
        self.spawn(async move {
            let Some(result) = ok(reply.await) else {
                return;
            };
            if !js::truthy(&result) || !status_is(&result, "ok") {
                this.notify(&this.tr("#L_CR_NO_DUMP", &[]), ICON_INFO);
            }
        });
    }

    /// `onOpenDiscord()`.
    pub fn on_open_discord(&self) {
        self.fire("open-discord", Vec::new());
    }

    // ---- Шапка: профиль, друзья, карточка обновления ------------------------------------------

    /// `openRunnerProfile(id)`: свой профиль из рейтинга, поиска или списка спонсоров открывается
    /// как свой (без голоса за себя, с вкладкой настроек); пустой `id` — свой профиль или вход.
    pub fn open_runner_profile(&self, id: &str) {
        // Вход и профили — только в официальной сборке.
        if !self.with_state(|state| state.official()) {
            return;
        }
        let show = |this: &ShellFlows, target: String| {
            this.with_state(|state| {
                state.profile_target = target;
                state.friends_open = false;
                state.profile_open = true;
            });
            this.changed(ShellChange::Windows);
        };
        if id.is_empty() {
            show(self, String::new());
            return;
        }
        let reply = self.invoke("runner-state", Vec::new());
        let this = self.clone();
        let target = id.to_owned();
        self.spawn(async move {
            // Без состояния входа — как чужой: сервер голос за себя всё равно не примет.
            let own = ok(reply.await).is_some_and(|state| {
                state
                    .get("account")
                    .filter(|account| js::truthy(account))
                    .and_then(|account| account.get("steamId64"))
                    .and_then(Value::as_str)
                    == Some(target.as_str())
            });
            show(&this, if own { String::new() } else { target });
        });
    }

    /// `onAccountAction(id)`: «Профиль» и «Друзья» меню аккаунта; друзья открываются вместо
    /// профиля, а не поверх него.
    pub fn on_account_action(&self, id: &str) {
        if !self.with_state(|state| state.official()) {
            return;
        }
        match id {
            "open-profile" => self.open_runner_profile(""),
            "friends" => {
                self.with_state(|state| {
                    if !state.friends_open {
                        state.profile_open = false;
                    }
                    state.friends_open = !state.friends_open;
                });
                self.changed(ShellChange::Windows);
            }
            _ => {}
        }
    }

    /// `onCloseProfile()`.
    pub fn close_profile(&self) {
        self.close_window("profile");
    }

    /// `friendsOpen = false` панели друзей.
    pub fn close_friends(&self) {
        self.close_window("friends");
    }

    /// `onToggleSteamId()`: «Показывать SteamID» в настройках профиля; значение уходит и на сервер.
    pub fn on_toggle_steam_id(&self) {
        let hide = self.pref("hideSteamId") != Some(Value::Bool(true));
        self.save_prefs(vec![("hideSteamId", Value::Bool(hide))]);
        self.changed(ShellChange::Windows);
        self.push_hide_steam_id(hide);
    }

    /// `pushHideSteamId(value)`: «Скрыть SteamID» хранится и на сервере: по нему лаунчеры других
    /// игроков не показывают ID в профиле и поиске. Переключатель меняется сразу, запрос уходит
    /// следом; пока он в пути, ответы сервера со старым значением настройку не откатывают.
    fn push_hide_steam_id(&self, value: bool) {
        let sequence = self.inner.hide_id_sequence.get() + 1;
        self.inner.hide_id_sequence.set(sequence);
        let reply = self.invoke("runner-customize", vec![json!({"hideSteamId": value})]);
        let this = self.clone();
        self.spawn(async move {
            // Без сети: отправится при следующем открытии профиля.
            let _ = reply.await;
            if this.inner.hide_id_sequence.get() == sequence {
                this.inner.hide_id_sequence.set(0);
            }
        });
    }

    /// `onServerHideSteamId(value)`: значение флага с сервера (не булево — флага нет).
    pub fn on_server_hide_steam_id(&self, value: Option<bool>) {
        if self.inner.hide_id_sequence.get() != 0 {
            return;
        }
        let local = self.pref("hideSteamId") == Some(Value::Bool(true));
        match value {
            // У старых профилей флага на сервере нет: отправляется локальный.
            None => {
                if local {
                    self.push_hide_steam_id(true);
                }
            }
            Some(value) => {
                if value == local {
                    return;
                }
                self.save_prefs(vec![("hideSteamId", Value::Bool(value))]);
                self.changed(ShellChange::Windows);
            }
        }
    }

    /// `@dismiss="updateDismissed = true"` карточки обновления.
    pub fn dismiss_update(&self) {
        self.with_state(|state| state.update_dismissed = true);
        self.changed(ShellChange::Update);
    }

    /// `@whats-new` карточки: `onWhatsNew({ target: 'launcher', version: updateStatus.version })`.
    pub fn on_update_whats_new(&self) {
        let version = self
            .inner
            .state
            .borrow()
            .update_status
            .get("version")
            .cloned()
            .unwrap_or(Value::Null);
        self.on_whats_new(&json!({"target": "launcher", "version": version}));
    }

    /// `build-official`: сборка перестала быть официальной — профиль и друзья закрываются.
    pub fn on_build(&self, payload: &Value) {
        let Some(build) = payload.as_str().and_then(Standing::parse) else {
            return;
        };
        let closed = self.with_state(|state| {
            state.build = build;
            let open = state.profile_open || state.friends_open;
            if !state.official() {
                state.profile_open = false;
                state.friends_open = false;
            }
            open && !state.official()
        });
        if closed {
            self.changed(ShellChange::Windows);
        }
        self.changed(ShellChange::Build);
    }

    /// `public-online`.
    pub fn on_public_online(&self, snapshot: &Value) {
        if snapshot.is_object() {
            self.with_state(|state| state.online_snapshot = snapshot.clone());
            self.changed(ShellChange::Online);
        }
    }

    // ---- Инструменты -------------------------------------------------------------------------

    /// `onSelectTool(toolId)`.
    pub fn on_select_tool(&self, tool_id: &str) {
        self.with_state(|state| {
            state.section = "games".into();
            state.active_tab = "tools".into();
            state.selected_tool_id = tool_id.to_owned();
        });
        self.changed(ShellChange::Selection);
        self.load_tools();
    }

    /// `syncToolSplits()`.
    fn sync_tool_splits(&self) {
        let splits = Value::Object(self.inner.state.borrow().live_split_splits.clone());
        self.fire("tool-splits-set", vec![json!("livesplit"), splits]);
    }

    /// `loadTools()`.
    pub fn load_tools(&self) {
        let reply = self.invoke("tool-list", Vec::new());
        let this = self.clone();
        self.spawn(async move {
            let Ok(Value::Array(list)) = reply.await else {
                return;
            };
            this.with_state(|state| {
                if state.selected_tool_id.is_empty()
                    && let Some(first) = list.first()
                {
                    state.selected_tool_id = js::to_template_string(first.get("toolId"));
                }
                state.tools = list;
            });
            this.changed(ShellChange::Tools);
            this.sync_tool_splits();
        });
    }

    /// `tool-progress`.
    pub fn on_tool_progress(&self, payload: &Value) {
        let Some(tool_id) = payload.get("toolId").filter(|value| js::truthy(value)) else {
            return;
        };
        let percent = payload.get("percent").cloned().unwrap_or(Value::Null);
        let updated = self.with_state(|state| {
            let Some(tool) = state
                .tools
                .iter_mut()
                .find(|tool| tool.get("toolId") == Some(tool_id))
            else {
                return false;
            };
            if let Value::Object(map) = tool {
                map.insert("state".into(), json!("installing"));
                map.insert("percent".into(), percent);
            }
            true
        });
        if updated {
            self.changed(ShellChange::Tools);
        }
    }

    /// `tool-status`.
    pub fn on_tool_status(&self, card: &Value) {
        let Some(tool_id) = card.get("toolId").filter(|value| js::truthy(value)) else {
            return;
        };
        self.with_state(|state| {
            match state
                .tools
                .iter_mut()
                .find(|tool| tool.get("toolId") == Some(tool_id))
            {
                Some(tool) => *tool = card.clone(),
                None => state.tools.push(card.clone()),
            }
        });
        self.changed(ShellChange::Tools);
    }

    /// `onToolInstall(toolId)`.
    pub fn on_tool_install(&self, tool_id: &str) {
        let reply = self.invoke("tool-install", vec![json!(tool_id)]);
        let this = self.clone();
        self.spawn(async move {
            let Some(result) = ok(reply.await) else {
                return;
            };
            if status_is(&result, "cancelled") {
                return;
            }
            if status_is(&result, "error") {
                let code = error_code(Some(&result))
                    .filter(js::truthy)
                    .map(|code| format!(" ({})", js::to_string(&code)))
                    .unwrap_or_default();
                let text = format!("{}{code}", this.tr("#L_TOOL_INSTALL_FAILED", &[]));
                this.notify(&text, ICON_ERROR);
                return;
            }
            this.notify(&this.tr("#L_TOOL_INSTALLED", &[]), ICON_CHECK);
            this.load_tools();
        });
    }

    /// `onToolCancel(toolId)`.
    pub fn on_tool_cancel(&self, tool_id: &str) {
        self.fire("tool-cancel", vec![json!(tool_id)]);
    }

    /// `onToolLaunch(toolId)`: у LiveSplit сначала окно игры и сплитов.
    pub fn on_tool_launch(&self, tool_id: &str) {
        let tool_id = tool_id.to_owned();
        let this = self.clone();
        self.spawn(async move {
            let mut game_choice = Value::String(String::new());
            let mut preset = Value::Null;
            if tool_id == "livesplit" {
                let Some(picked) = this.ask_live_split_launch().await else {
                    return;
                };
                game_choice = json!(picked.game);
                preset = picked
                    .preset
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null);
                this.remember_live_split(&picked);
            }
            let launch = |restart: bool| {
                let mut args = vec![json!(tool_id), preset.clone(), game_choice.clone()];
                if restart {
                    args.push(json!({"restart": true}));
                }
                this.invoke("tool-launch", args)
            };
            let Some(mut result) = ok(launch(false).await) else {
                return;
            };
            if status_is(&result, "already-running") && tool_id == "livesplit" {
                if result.get("same").is_some_and(js::truthy) {
                    this.notify(
                        &this.tr("#L_TOOL_LIVESPLIT_ALREADY", &[]),
                        "fas fa-stopwatch",
                    );
                    return;
                }
                let restart = this
                    .confirm(Confirm {
                        title: this.tr("#L_TOOL_LIVESPLIT_RESTART_TITLE", &[]),
                        text: this.tr("#L_TOOL_LIVESPLIT_RESTART_TEXT", &[]),
                        confirm_label: Some(this.tr("#L_TOOL_LIVESPLIT_RESTART", &[])),
                        ..Confirm::default()
                    })
                    .await;
                if !restart {
                    return;
                }
                let Some(again) = ok(launch(true).await) else {
                    return;
                };
                result = again;
                if status_is(&result, "stop-timeout") {
                    this.notify(&this.tr("#L_TOOL_STOP_FAILED", &[]), ICON_ERROR);
                    return;
                }
            }
            if status_is(&result, "error") {
                this.notify(&this.tr("#L_TOOL_LAUNCH_FAILED", &[]), ICON_ERROR);
            }
        });
    }

    /// `askLiveSplitLaunch()`: окно выбора игры и сплитов.
    async fn ask_live_split_launch(&self) -> Option<LiveSplitPick> {
        let games = game_availability::available_games();
        let ids: Vec<Value> = games.iter().map(|game| json!(game.id)).collect();
        let described = self
            .invoke("tool-splits-presets", vec![Value::Array(ids)])
            .await
            .unwrap_or(Value::Null);
        let mut presets_by_id: Vec<(String, Value)> = Vec::new();
        if let Value::Array(list) = &described {
            for entry in list {
                if let Some(id) = entry.get("id").filter(|value| js::truthy(value)) {
                    let id = js::to_string(id);
                    presets_by_id.retain(|(key, _)| *key != id);
                    presets_by_id.push((id, entry.clone()));
                }
            }
        }
        let choices: Vec<LiveSplitGame> = games
            .iter()
            .map(|game| {
                let entry = presets_by_id
                    .iter()
                    .find(|(id, _)| id == game.id)
                    .map(|(_, entry)| entry.clone())
                    .unwrap_or_else(|| json!({}));
                LiveSplitGame {
                    id: game.id.to_owned(),
                    game_id: entry
                        .get("gameId")
                        .filter(|value| js::truthy(value))
                        .map(js::to_string),
                    label: short_name(game.id).unwrap_or(game.name).to_owned(),
                    presets: match entry.get("presets") {
                        Some(Value::Array(list)) => list.clone(),
                        _ => Vec::new(),
                    },
                }
            })
            .collect();
        if choices.is_empty() {
            return None;
        }
        let (running, default_pref, selected, chosen) = {
            let state = self.inner.state.borrow();
            let running = state
                .delivery
                .iter()
                .find(|(_, status)| {
                    js::truthy(status)
                        && status
                            .get("process")
                            .filter(|process| js::truthy(process))
                            .is_some_and(|process| process.get("running").is_some_and(js::truthy))
                })
                .map(|(id, _)| id.to_owned());
            (
                running,
                state.prefs.get("liveSplitGame").cloned(),
                state.selected_game_id.clone(),
                state.live_split_splits.clone(),
            )
        };
        // 0.4.7 ищет игру оболочки («70») среди игр доставки (`choice.gameId === running`) — не
        // находит никогда, и по умолчанию идёт прошлая игра или выбранная.
        let running_choice = running.and_then(|running| {
            choices
                .iter()
                .find(|choice| choice.game_id.as_deref() == Some(running.as_str()))
                .map(|choice| choice.id.clone())
        });
        let default_game = running_choice.or_else(|| {
            default_pref
                .filter(js::truthy)
                .map(|value| js::to_string(&value))
                .or(Some(selected))
        });
        self.port()
            .ask_livesplit(LiveSplitLaunch {
                games: choices,
                default_game,
                chosen,
            })
            .await
    }

    /// `rememberLiveSplit(picked)`: игра и сплиты — в prefs и в main.
    fn remember_live_split(&self, picked: &LiveSplitPick) {
        if let (Some(game_id), Some(preset)) = (&picked.game_id, &picked.preset)
            && !game_id.is_empty()
            && !preset.is_empty()
        {
            self.with_state(|state| {
                state
                    .live_split_splits
                    .insert(game_id.clone(), json!(preset));
            });
        }
        let splits = Value::Object(self.inner.state.borrow().live_split_splits.clone());
        self.save_prefs(vec![
            ("liveSplitGame", json!(picked.game)),
            ("toolSplits", splits),
        ]);
        self.sync_tool_splits();
    }

    /// `onToolStop(toolId)`.
    pub fn on_tool_stop(&self, tool_id: &str) {
        let reply = self.invoke("tool-stop", vec![json!(tool_id)]);
        let tool_id = tool_id.to_owned();
        let this = self.clone();
        self.spawn(async move {
            let Some(result) = ok(reply.await) else {
                return;
            };
            if !js::truthy(&result) || status_is(&result, "error") {
                this.notify(&this.tr("#L_TOOL_STOP_FAILED", &[]), ICON_ERROR);
                return;
            }
            if !status_is(&result, "timeout") {
                return;
            }
            let force = this
                .confirm(Confirm {
                    title: this.tr("#L_TOOL_STOP_TIMEOUT_TITLE", &[]),
                    text: this.tr("#L_TOOL_STOP_TIMEOUT_TEXT", &[]),
                    confirm_label: Some(this.tr("#L_C_STOP_FORCE", &[])),
                    danger: true,
                    ..Confirm::default()
                })
                .await;
            if !force {
                return;
            }
            let Some(forced) = ok(this
                .invoke("tool-stop", vec![json!(tool_id), json!({"force": true})])
                .await)
            else {
                return;
            };
            if !js::truthy(&forced) || !status_is(&forced, "stopped") {
                this.notify(&this.tr("#L_TOOL_STOP_FAILED", &[]), ICON_ERROR);
            }
        });
    }

    /// `onToolRemove(toolId)`.
    pub fn on_tool_remove(&self, tool_id: &str) {
        let tool_id = tool_id.to_owned();
        let this = self.clone();
        self.spawn(async move {
            let confirmed = this
                .confirm(Confirm {
                    title: this.tr("#L_TOOL_REMOVE", &[]),
                    text: this.tr("#L_TOOL_REMOVE_TEXT", &[]),
                    confirm_label: Some(this.tr("#L_TOOL_REMOVE", &[])),
                    ..Confirm::default()
                })
                .await;
            if !confirmed {
                return;
            }
            if this
                .invoke("tool-remove", vec![json!(tool_id)])
                .await
                .is_err()
            {
                return;
            }
            this.notify(&this.tr("#L_TOOL_REMOVED", &[]), ICON_CHECK);
            this.load_tools();
        });
    }

    /// `onToolFolder(toolId)`.
    pub fn on_tool_folder(&self, tool_id: &str) {
        let reply = self.invoke("tool-open-folder", vec![json!(tool_id)]);
        let this = self.clone();
        self.spawn(async move {
            if let Ok(result) = reply.await
                && js::truthy(&result)
                && !status_is(&result, "ok")
            {
                this.notify(&this.tr("#L_TOOL_FOLDER_FAILED", &[]), ICON_ERROR);
            }
        });
    }

    /// `onToolHelp(toolId)`.
    pub fn on_tool_help(&self, tool_id: &str) {
        self.notify(&self.tr("#L_TOOL_HELP_HINT", &[]), "fas fa-question-circle");
        self.fire("tool-open-help", vec![json!(tool_id)]);
    }

    /// `offerTimer()`: один раз предложить LiveSplit перед первым запуском игры. Истина — игрок
    /// пошёл ставить таймер, запуск не нужен.
    async fn offer_timer(&self) -> bool {
        if self.pref("timerOffered") == Some(Value::Bool(true)) {
            return false;
        }
        let timer = self
            .inner
            .state
            .borrow()
            .tools
            .iter()
            .find(|tool| text_of(tool, "toolId") == Some("livesplit"))
            .cloned();
        let Some(timer) = timer else {
            return false;
        };
        if text_of(&timer, "state") == Some("ready") {
            self.remember_timer_offered();
            return false;
        }
        let wanted = self
            .confirm(Confirm {
                title: self.tr("#L_TOOL_OFFER_TITLE", &[]),
                text: self.tr("#L_TOOL_OFFER_TEXT", &[]),
                confirm_label: Some(self.tr("#L_C_YES", &[])),
                cancel_label: Some(self.tr("#L_C_NO", &[])),
                ..Confirm::default()
            })
            .await;
        self.remember_timer_offered();
        if !wanted {
            return false;
        }
        let tool = js::to_template_string(timer.get("toolId"));
        // Вкладку вид берёт из флага; щелчок в колонке показывает её сам и флаг не ставит.
        self.with_state(|state| state.tool_open = Some(tool.clone()));
        self.on_select_tool(&tool);
        true
    }

    fn remember_timer_offered(&self) {
        self.save_prefs(vec![("timerOffered", Value::Bool(true))]);
    }

    // ---- Опрос о конфиге и обучение (0.4.9) --------------------------------------------------

    /// `offerConfigSetup(gameId)`: игра без конфига — спросить, как его готовить. Истина — запуск
    /// не продолжать (отмена, «С нуля» и «Свой» уводят на «Персонализацию»; игрок жмёт «Играть»
    /// сам).
    async fn offer_config_setup(&self, game_id: &Value) -> bool {
        // Двойной щелчок по «Играть»: второй вызов не открывает второй опрос и не запускает игру.
        if self.inner.config_setup_busy.replace(true) {
            return true;
        }
        let launcher = self.inner.state.borrow().selected_game_id.clone();
        let stop = self.config_setup(game_id, &launcher).await;
        self.inner.config_setup_busy.set(false);
        self.changed(ShellChange::Windows);
        stop
    }

    async fn config_setup(&self, game_id: &Value, launcher: &str) -> bool {
        let Ok(state) = self
            .invoke("player-config-setup-state", vec![game_id.clone()])
            .await
        else {
            return false;
        };
        if !state.get("needed").is_some_and(js::truthy) {
            return false;
        }
        let (reply, answer) = tokio::sync::oneshot::channel();
        *self.inner.config_setup_reply.borrow_mut() = Some(reply);
        self.with_state(|state| state.config_setup_open = true);
        self.changed(ShellChange::Windows);
        let Some(choice) = answer.await.ok().flatten() else {
            return true;
        };
        // Не записалось (папка недоступна) — игрока не задерживаем: запуск сам покажет причину.
        if self
            .invoke(
                "player-config-setup-done",
                vec![game_id.clone(), json!(choice)],
            )
            .await
            .is_err()
        {
            tracing::error!("[player-config] ответ на опрос не записан");
        }
        if choice == "skip" {
            return false;
        }
        self.with_state(|state| {
            // 0.4.9 кладёт сюда id доставки («hl1-won»), которого нет среди игр страницы:
            // страница остаётся на первой игре, «Свой конфиг» кончается «не загрузилось». Здесь —
            // игра оболочки, чей конфиг спрашивали.
            state.configs_open = Some(launcher.to_owned());
            state.configs_open_token += 1;
            state.configs_section = "configs".into();
            if choice == "scratch" {
                state.config_tour_requested = true;
            } else {
                state.configs_import_requested = true;
            }
        });
        self.changed(ShellChange::Selection);
        true
    }

    /// `onConfigSetupChoose(choice)`: ответ окна опроса; `None` — «Закрыть», Esc, подложка.
    pub fn on_config_setup_choose(&self, choice: Option<&str>) {
        self.with_state(|state| state.config_setup_open = false);
        if let Some(reply) = self.inner.config_setup_reply.borrow_mut().take() {
            let _ = reply.send(choice.map(str::to_owned));
        }
        self.changed(ShellChange::Windows);
    }

    /// `configSetupBusy`.
    pub fn config_setup_busy(&self) -> bool {
        self.inner.config_setup_busy.get()
    }

    /// `onConfigTourDone`: обучение пройдено или пропущено — больше само не показывается.
    pub fn mark_config_tour_seen(&self) {
        self.save_prefs(vec![("configTourSeen", Value::Bool(true))]);
    }

    // ---- Библиотека, приветствие, настройки загрузок ----------------------------------------

    /// `checkLibrarySetup()`: первый запуск — окно выбора папки, иначе приветствие.
    pub fn check_library_setup(&self) {
        let reply = self.invoke("get-library-path", Vec::new());
        let this = self.clone();
        self.spawn(async move {
            // `info.path` у пустого ответа — исключение: сценарий обрывается. 0.4.9: сбой проверки
            // не лишает обучения навсегда (`finally`: `welcomeChecked`, если окна папки нет).
            let Some(info) = ok(reply.await).filter(js::truthy) else {
                this.set_welcome_checked();
                return;
            };
            let path = info.get("path").cloned().unwrap_or(Value::Null);
            this.with_state(|state| state.install_path = js::to_string(&path));
            if !info.get("confirmed").is_some_and(js::truthy) {
                this.with_state(|state| state.library_setup_open = true);
                this.changed(ShellChange::Windows);
            } else {
                this.changed(ShellChange::Windows);
                this.check_welcome();
            }
        });
    }

    /// `onLibraryConfirmed(confirmedPath)`.
    pub fn on_library_confirmed(&self, path: &str) {
        self.with_state(|state| {
            state.install_path = path.to_owned();
            state.library_setup_open = false;
        });
        self.changed(ShellChange::Windows);
        self.notify(&self.tr("#L_SETTINGS_SAVED", &[]), ICON_CHECK);
        self.check_welcome();
    }

    /// `checkWelcome()`: «HLSR — Новая эра!» один раз, если игрок ещё не вошёл через Steam.
    pub fn check_welcome(&self) {
        if self
            .pref("welcomeSeen")
            .is_some_and(|value| js::truthy(&value))
        {
            self.set_welcome_checked();
            return;
        }
        let reply = self.invoke("runner-state", Vec::new());
        let this = self.clone();
        self.spawn(async move {
            let Ok(state) = reply.await else {
                this.set_welcome_checked();
                return;
            };
            let signed_in = state
                .get("account")
                .filter(|account| js::truthy(account))
                .is_some_and(|account| account.get("signedIn").is_some_and(js::truthy));
            if signed_in {
                this.mark_welcome_seen();
            } else {
                this.with_state(|state| state.welcome_open = true);
            }
            this.set_welcome_checked();
        });
    }

    /// `welcomeChecked = true` (0.4.9, `finally` у `checkWelcome` и `checkLibrarySetup`): пока
    /// открыто окно выбора папки, обучение ждёт.
    fn set_welcome_checked(&self) {
        let library_open = self.with_state(|state| {
            state.welcome_checked |= !state.library_setup_open;
            state.library_setup_open
        });
        if !library_open {
            self.changed(ShellChange::Windows);
        }
    }

    fn mark_welcome_seen(&self) {
        self.save_prefs(vec![("welcomeSeen", Value::Bool(true))]);
    }

    /// `onWelcomeSignedIn()`: вход начат — окно профиля покажет код и ожидание.
    pub fn on_welcome_signed_in(&self) {
        self.mark_welcome_seen();
        self.with_state(|state| {
            state.welcome_open = false;
            state.profile_target = String::new();
            state.friends_open = false;
            state.profile_open = true;
        });
        self.changed(ShellChange::Windows);
    }

    /// `onWelcomeLater()`.
    pub fn on_welcome_later(&self) {
        self.mark_welcome_seen();
        self.with_state(|state| state.welcome_open = false);
        self.changed(ShellChange::Windows);
        // Совет войти через аватар — только когда аватар в шапке есть.
        if self.with_state(|state| state.official()) {
            self.notify(&self.tr("#L_WELCOME_LATER_TOAST", &[]), "fas fa-user");
        }
    }

    /// `initializeDownloadSettings()`; задача завершается, когда 0.4.7 вызвал бы `.then`.
    fn initialize_download_settings(&self) -> impl Future<Output = ()> + 'static {
        let reply = self.invoke("download-settings-get", Vec::new());
        let this = self.clone();
        async move {
            let saved = match reply.await {
                Ok(state) => {
                    if state.get("initialized").is_some_and(js::truthy) {
                        let limit = state.get("speedLimit").cloned().unwrap_or(Value::Null);
                        this.save_prefs(vec![("speedLimit", limit)]);
                        true
                    } else {
                        let limit = this.pref("speedLimit").unwrap_or(json!(0));
                        match this.invoke("download-settings-set", vec![limit]).await {
                            Ok(result) => result.get("ok").is_some_and(js::truthy),
                            Err(_) => false,
                        }
                    }
                }
                Err(_) => false,
            };
            if !saved {
                this.notify(&this.tr("#L_S_SPEED_SAVE_FAILED", &[]), ICON_ERROR);
            }
            // async-функция вернулась, обещание разрешилось — `.then` на следующем круге.
            yield_now().await;
            yield_now().await;
        }
    }

    /// `initializeDownloadSettings()` отдельно (без `.then` запуска оболочки).
    pub fn on_initialize_download_settings(&self) {
        let run = self.initialize_download_settings();
        self.spawn(run);
    }

    /// `syncTrayPreference()`.
    pub fn sync_tray_preference(&self) {
        let hide = self.pref("hideWhilePlaying") != Some(Value::Bool(false));
        self.fire("tray-hide-while-playing-set", vec![json!(hide)]);
    }

    /// `syncUpdaterLanguage()`.
    pub fn sync_updater_language(&self) {
        let language = self.language();
        self.fire("updater-set-language", vec![json!(language)]);
    }

    /// Язык уже сменило окно настроек (`setLanguage` 0.4.7 — `SettingsController::set_language`:
    /// записало `settings.json`, перестроило интерфейс и отправило `updater-set-language`): у
    /// оболочки — только `currentLanguage`.
    pub fn language_changed(&self, id: &str) {
        *self.inner.language.borrow_mut() = id.to_owned();
    }

    // ---- Обновление лаунчера ------------------------------------------------------------------

    /// `updater-state`.
    pub fn on_updater_state(&self, status: &Value) {
        let state = status.get("state").and_then(Value::as_str);
        let source = status.get("checkSource").and_then(Value::as_str);
        let previous_version = self
            .inner
            .state
            .borrow()
            .update_status
            .get("version")
            .cloned();
        let undismiss = (state == Some("checking") && source == Some("manual"))
            || (state == Some("available") && status.get("version").cloned() != previous_version)
            || state == Some("downloaded")
            || (state == Some("error") && source == Some("manual"));
        self.with_state(|shell| {
            if undismiss {
                shell.update_dismissed = false;
            }
            shell.update_status = status.clone();
        });
        self.changed(ShellChange::Update);
    }

    /// `checkForUpdates()`.
    pub fn check_for_updates(&self) {
        self.fire("updater-check", Vec::new());
    }

    /// `cancelUpdateDownload()`.
    pub fn cancel_update_download(&self) {
        self.fire("updater-cancel", Vec::new());
    }

    /// `startUpdateDownload()`.
    pub fn start_update_download(&self) {
        let reply = self.invoke("updater-download", Vec::new());
        let this = self.clone();
        self.spawn(async move {
            let started = reply.await.map(|value| js::truthy(&value)).unwrap_or(false);
            if started {
                return;
            }
            this.notify(&this.tr("#L_U_CHECKING", &[]), "fas fa-sync");
            this.fire("updater-check", Vec::new());
        });
    }

    /// `confirmInstallNow()`: перезапуск с установкой; при идущей загрузке игры — опасно.
    pub fn confirm_install_now(&self) {
        let busy = self.delivery_busy();
        let this = self.clone();
        self.spawn(async move {
            let question = if busy {
                this.tr("#L_U_CONFIRM_RESTART_BUSY", &[])
            } else {
                this.tr("#L_U_CONFIRM_RESTART", &[])
            };
            let restart = this
                .confirm(Confirm {
                    title: this.tr("#L_C_RESTART_TITLE", &[]),
                    text: question,
                    confirm_label: Some(this.tr("#L_C_RESTART", &[])),
                    danger: busy,
                    ..Confirm::default()
                })
                .await;
            if restart {
                this.fire("updater-install-now", Vec::new());
            }
        });
    }

    /// `installOnQuit()`.
    pub fn install_on_quit(&self) {
        self.fire("updater-install-on-quit", Vec::new());
        self.with_state(|state| state.update_dismissed = true);
        self.changed(ShellChange::Update);
        self.notify(&self.tr("#L_U_WILL_INSTALL_ON_QUIT", &[]), ICON_CHECK);
    }

    // ---- Запуск оболочки ---------------------------------------------------------------------

    /// `mounted()`: игра при запуске (`game-delivery-selected` — и для Discord), язык программы
    /// обновления, трей, папка библиотеки, настройки загрузок → инструменты и статусы доставки,
    /// статистика игр, состояние обновления, `support-thanks-ready`, онлайн. Подписки на события —
    /// у владельца сервисов ([`ShellFlows::on_event`]).
    pub fn mount(&self, last_launched: Option<&Value>) {
        let start = game_availability::startup_game(last_launched);
        self.with_state(|state| state.selected_game_id = start.clone());
        self.changed(ShellChange::Selection);
        self.fire("game-delivery-selected", vec![json!(start)]);
        self.sync_updater_language();
        self.sync_tray_preference();
        self.check_library_setup();
        let settings = self.initialize_download_settings();
        let this = self.clone();
        self.spawn(async move {
            settings.await;
            this.with_state(|state| state.download_settings_ready = true);
            this.load_tools();
            this.load_delivery_statuses();
        });
        for game in game_availability::column_games() {
            self.load_stats(game.game.id);
        }
        let reply = self.invoke("updater-get-state", Vec::new());
        let this = self.clone();
        self.spawn(async move {
            if let Ok(status) = reply.await
                && js::truthy(&status)
            {
                this.with_state(|state| state.update_status = status);
                this.changed(ShellChange::Update);
            }
        });
        self.inner.services.send("support-thanks-ready", Vec::new());
        let reply = self.invoke("public-online-get", Vec::new());
        let this = self.clone();
        self.spawn(async move {
            if let Ok(snapshot) = reply.await
                && js::truthy(&snapshot)
            {
                this.with_state(|state| state.online_snapshot = snapshot);
                this.changed(ShellChange::Online);
            }
        });
    }

    /// Событие главного процесса (`ipcRenderer.on` оболочки); `false` — канал не её.
    pub fn on_event(&self, channel: &str, payload: &Value) -> bool {
        match channel {
            "game-delivery-status" if js::truthy(payload) => self.on_delivery_status(payload),
            "game-build-import-applied" => self.on_build_import_applied(payload),
            "select-game" if js::truthy(payload) => self.on_select_game_from_args(payload),
            "game-process-running" => self.on_process_running(),
            "game-process-exited" => self.on_process_exited(),
            "game-process-crashed" => self.on_process_crashed(payload),
            "game-crash-report" => self.on_crash_report(payload),
            "demo-ffmpeg-progress" => self.on_ffmpeg_progress(payload),
            "tool-progress" if js::truthy(payload) => self.on_tool_progress(payload),
            "tool-status" if js::truthy(payload) => self.on_tool_status(payload),
            "updater-state" if js::truthy(payload) => self.on_updater_state(payload),
            "public-online" => self.on_public_online(payload),
            "build-official" => self.on_build(payload),
            "runner-profile-state" => self.on_runner_profile_updated(),
            "achievement-unlocked" if js::truthy(payload) => self.on_achievement_unlocked(payload),
            "game-delivery-status"
            | "select-game"
            | "tool-progress"
            | "tool-status"
            | "updater-state"
            | "achievement-unlocked" => {}
            _ => return false,
        }
        true
    }

    /// Ответ окна подтверждения в виде, который ждут сценарии (для адаптеров окон).
    pub fn confirm_answer(accepted: bool) -> ConfirmAnswer {
        if accepted {
            ConfirmAnswer::Accepted
        } else {
            ConfirmAnswer::Declined
        }
    }

    /// Игры колонки по порядку (`games` оболочки): id.
    pub fn game_ids() -> Vec<&'static str> {
        game_availability::column_games()
            .iter()
            .map(|game| game.game.id)
            .collect()
    }

    /// Имя игры из `games.config.js`.
    pub fn game_name(id: &str) -> Option<&'static str> {
        GAME_LIST
            .iter()
            .find(|game| game.id == id)
            .map(|game| game.name)
    }
}

/// `normalizeSpeedLimit` (`download-settings.js`): только число из списка, иначе 0.
fn normalize_speed_limit(value: &Value) -> Value {
    const LIMITS: [f64; 8] = [0.0, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0];
    match value.as_f64() {
        Some(limit) if value.is_number() && LIMITS.contains(&limit) => value.clone(),
        _ => json!(0),
    }
}
