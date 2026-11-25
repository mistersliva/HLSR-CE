//! Корень оболочки (`App.vue` + `LauncherShell.vue` 0.4.9): кадр окна (скругление 16, контур;
//! в полном экране радиус 0 без контура, смена — окно гаснет .13 с и проявляется .24 с),
//! разделы `home`/`games`/`configs` (`keep-alive` у «Персонализации» и «Демо»), слоты «Игр»
//! (герой, нижняя панель, инструмент, «Демо»), шапка ([`super::topbar`]), стопка окон и
//! `overlayOpen` ([`Layers`]), тосты (`notify`), Discord (`discord-rpc-activity`), секретные
//! достижения оболочки (`wake-up`, `back-to-past`; `chronicler` и `night-shift` — в сценариях),
//! пасхалки ([`super::eggs`]: HESOYAM и пони; HLSR-CE — «LEGACY», интерфейс 0.2.9 [`LegacyShell`]).
//!
//! Логика — контроллеры `hlsr-ui`: [`ShellController`] (тосты, подтверждение, язык),
//! [`NavController`] (раздел, колонка, игра), [`LogoController`], [`ShellFlows`] (сценарии
//! `LauncherShell` и `mounted()`). Контроллеры зовут слушателей без `cx`, иногда посреди обновления
//! корня: их перемены копятся и доходят до вида задачей потока окна ([`ShellRoot::sync`]).
//!
//! Экраны других задач смонтированы заглушками (`X::new(&services, cx)`); состояние оболочки им
//! даёт [`ShellContext`] (глобал окна), язык — `lang_rev`: смена языка пересоздаёт всё внутри
//! рамки (`:key="langRev"`).

use super::background::Background;
use super::eggs::{Cheat, Eggs, LegacyCode};
use super::rail::GameSidebar;
use super::topbar::Topbar;
use super::window_controls::WindowControls;
use crate::cache::cached;
use crate::configs::{page::GameConfigsPanel, setup_modal::ConfigSetupModal, tour::ConfigTour};
use crate::demo::panel::DemoPanel;
use crate::dialogs::{
    changelog::ChangelogModal, confirm::ConfirmModal, library_setup::LibrarySetupModal,
    welcome::WelcomeModal,
};
use crate::env::GpuiEnv;
use crate::games::mod_page::ModPage;
use crate::games::{
    download_bar::DownloadBar,
    hero::HeroPanel,
    modals::GameModals,
    tool_panel::{ToolControls, ToolPanel},
};
use crate::glass::RailGlass;
use crate::header::{
    account::AccountMenu, notifications::NotificationCenter, search::RunnerSearch,
    sponsor_button::SponsorsButton, update_card::UpdateCard,
};
use crate::home::panel::HomePanel;
use crate::layer::{Layers, Toasts, tab_root};
use crate::legacy::LegacyShell;
use crate::motion::{AnimGate, Timing, VALUE, swap, tween};
use crate::profile::{friends::FriendsPanel, modal::ProfileModal};
use crate::settings::SettingsModal;
use crate::sponsors::{modal::SponsorsModal, thanks::SupportThanksModal};
use crate::theme::Theme;
use gpui::{
    AnyView, App, AppContext, ClipboardItem, Context, Entity, FocusHandle, Global, KeyDownEvent,
    Pixels, Render, StyleRefinement, Subscription, Task, WeakEntity, Window, div, prelude::*,
    white,
};
use hlsr_core::language::Language;
use hlsr_core::launcher_prefs::{LauncherPrefs, prefs_defaults};
use hlsr_core::paths::Paths;
use hlsr_ui::controllers::background::Selection;
use hlsr_ui::controllers::game_demo;
use hlsr_ui::controllers::nav::{LogoController, NavController, NavPrefs};
use hlsr_ui::controllers::shell::{ConfirmFuture, ShellController};
use hlsr_ui::controllers::shell_flows::{
    LiveSplitFuture, LiveSplitLaunch, LiveSplitPick, ShellChange, ShellFlows, ShellInit, ShellPort,
};
use hlsr_ui::controllers::tasks::LocalFuture;
use hlsr_ui::controllers::wall_clock::WallClock;
use hlsr_ui::env::Env;
use hlsr_ui::keys::SecretSequence;
use hlsr_ui::services::{ServiceEvent, Services, Standing, StoreRead};
use serde_json::{Map, Value, json};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// Заглушка экрана другой задачи: `pub struct X` + `X::new(services, cx)` + пустой `Render`.
/// Владелец файла заменяет его содержимое; место в корне не трогает.
#[macro_export]
macro_rules! stub_view {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        pub struct $name;

        impl $name {
            pub fn new(
                _services: &hlsr_ui::services::Services,
                _cx: &mut gpui::Context<$name>,
            ) -> $name {
                $name
            }
        }

        impl gpui::Render for $name {
            fn render(
                &mut self,
                _: &mut gpui::Window,
                _: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                gpui::Empty
            }
        }
    };
}

/// Папки данных приложения (ставит `hlsr-app` до окна): `launcher-prefs.json`. Без него (тесты,
/// витрина) prefs живут только в памяти.
pub struct AppPaths(pub Paths);

impl Global for AppPaths {}

/// Состояние оболочки для экранов других задач: контроллеры (клоны — те же контроллеры),
/// сервисы окна и сам корень (`notify`, prefs, запрос LiveSplit).
#[derive(Clone)]
pub struct ShellContext {
    pub services: Services,
    pub shell: ShellController,
    pub nav: NavController,
    pub flows: ShellFlows,
    pub root: WeakEntity<ShellRoot>,
}

impl Global for ShellContext {}

impl ShellContext {
    pub fn get(cx: &App) -> Option<&ShellContext> {
        cx.try_global::<ShellContext>()
    }
}

/// Официальна ли сборка (`ShellState::build`); без оболочки — ещё неизвестно.
pub fn standing(cx: &App) -> Standing {
    ShellContext::get(cx).map_or(Standing::Pending, |shell| {
        shell.flows.with_state(|s| s.build)
    })
}

/// Вход, профили, спонсорство, «Главная» и счётчики онлайна — только у официальной сборки.
pub fn official(cx: &App) -> bool {
    standing(cx) == Standing::Official
}

/// `prefsStore`: `launcher-prefs.json` или только память.
pub struct Prefs {
    file: Option<LauncherPrefs>,
    data: Map<String, Value>,
}

impl Prefs {
    fn open(paths: Option<&Paths>) -> Prefs {
        let file = paths.map(LauncherPrefs::open);
        let Value::Object(mut data) = prefs_defaults() else {
            unreachable!("умолчания — объект");
        };
        if let Some(file) = &file {
            for (key, value) in data.iter_mut() {
                if let Some(saved) = file.get(key) {
                    *value = saved.clone();
                }
            }
            // Размеры панелей (`pane`, PaneSplitter) — без умолчания, но хранятся тут же.
            if let Some(pane) = file.get("pane") {
                data.insert("pane".to_owned(), pane.clone());
            }
        }
        Prefs { file, data }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    pub fn data(&self) -> &Map<String, Value> {
        &self.data
    }

    /// Записать ключи (`prefsStore.save` с изменёнными ключами): файл — одной записью на набор.
    pub fn save(&mut self, changes: &Map<String, Value>) {
        for (key, value) in changes {
            self.data.insert(key.clone(), value.clone());
        }
        if let Some(file) = &mut self.file
            && !file.set_many(changes)
        {
            let keys: Vec<&String> = changes.keys().collect();
            tracing::warn!("[shell] launcher-prefs.json: {keys:?} не записаны");
        }
    }
}

/// `prefs.railExpanded` для навигации.
struct NavPrefsOf(Rc<RefCell<Prefs>>);

impl NavPrefs for NavPrefsOf {
    fn rail_expanded(&self) -> bool {
        self.0.borrow().get("railExpanded") == Some(&Value::Bool(true))
    }

    fn set_rail_expanded(&self, expanded: bool) {
        let mut changes = Map::new();
        changes.insert("railExpanded".into(), Value::Bool(expanded));
        self.0.borrow_mut().save(&changes);
    }
}

/// Открытый запрос окна запуска LiveSplit (окно — задача G3).
pub struct LiveSplitAsk {
    pub request: LiveSplitLaunch,
    pub reply: tokio::sync::oneshot::Sender<Option<LiveSplitPick>>,
}

/// Что контроллеры накопили до следующего [`ShellRoot::sync`].
#[derive(Default)]
struct Pending {
    changes: Vec<ShellChange>,
    toasts: Vec<(String, String, u64)>,
    clipboard: Option<String>,
}

/// Порт сценариев оболочки: тексты, тосты, окна, prefs, часы, задачи потока окна.
struct Port {
    shell: ShellController,
    env: Rc<GpuiEnv>,
    prefs: Rc<RefCell<Prefs>>,
    pending: Rc<RefCell<Pending>>,
    livesplit: Rc<RefCell<Option<LiveSplitAsk>>>,
    poke: Rc<dyn Fn()>,
    clock: WallClock,
}

impl ShellPort for Port {
    fn tr(&self, key: &str, args: &[&str]) -> String {
        self.shell.tr(key, args)
    }

    fn notify(&self, text: &str, icon: &str) {
        self.shell.notify(text, icon);
    }

    fn notify_for(&self, text: &str, icon: &str, ms: u64) {
        self.shell.notify_for(text, icon, ms);
    }

    fn ask_confirm(&self, request: hlsr_ui::controllers::shell::Confirm) -> ConfirmFuture {
        self.shell.ask_confirm(request)
    }

    fn ask_livesplit(&self, request: LiveSplitLaunch) -> LiveSplitFuture {
        let (reply, answer) = LiveSplitFuture::pair();
        if let Some(previous) = self
            .livesplit
            .borrow_mut()
            .replace(LiveSplitAsk { request, reply })
        {
            let _ = previous.reply.send(None);
        }
        (self.poke)();
        answer
    }

    fn write_clipboard(&self, text: &str) {
        self.pending.borrow_mut().clipboard = Some(text.to_owned());
        (self.poke)();
    }

    fn save_prefs(&self, changes: &Map<String, Value>) {
        self.prefs.borrow_mut().save(changes);
    }

    fn hour(&self) -> u32 {
        self.clock
            .local(self.clock.now_ms())
            .map_or(0, |local| local.hour)
    }

    fn spawn(&self, task: LocalFuture) {
        self.env.spawn_local(task);
    }

    fn after(&self, delay: Duration, f: Box<dyn FnOnce()>) {
        self.env.after(delay, f);
    }

    fn changed(&self, what: ShellChange) {
        self.pending.borrow_mut().changes.push(what);
        (self.poke)();
    }
}

/// Всё внутри рамки (`.launcher-frame :key="langRev"`): пересоздаётся при смене языка.
///
/// Фон, колонка, шапка, герой, инструмент, нижние панели, «Главная», «Персонализация», «Демо» и
/// окна-диалоги — закэшированные виды ([`crate::cache`]): их рисует заново только своё
/// уведомление. Всё, что они берут у оболочки, приходит через корень, поэтому каждое уведомление
/// корня уведомляет и их ([`Frame::invalidate`];
/// фон берёт своё через [`ShellRoot::sync_background`] и в это не входит). Стекло колонки — слой
/// под ней ([`RailGlass`]): при параллаксе он меняется каждый кадр, колонка — нет.
pub(super) struct Frame {
    pub(super) background: Entity<Background>,
    pub(super) glass: Entity<RailGlass>,
    pub(super) rail: Entity<GameSidebar>,
    pub(super) topbar: Entity<Topbar>,
    pub(super) home: Option<Entity<HomePanel>>,
    pub(super) configs: Option<Entity<GameConfigsPanel>>,
    pub(super) demo: Option<Entity<DemoPanel>>,
    /// HLSR-CE: страница своего мода (вкладка `mods`).
    pub(super) mods: Option<Entity<ModPage>>,
    pub(super) hero: Entity<HeroPanel>,
    pub(super) tool: Entity<ToolPanel>,
    pub(super) download_bar: Entity<DownloadBar>,
    pub(super) tool_controls: Entity<ToolControls>,
    pub(super) search: Entity<RunnerSearch>,
    pub(super) sponsors: Entity<SponsorsButton>,
    pub(super) notifications: Entity<NotificationCenter>,
    pub(super) account: Entity<AccountMenu>,
    pub(super) update_card: Entity<UpdateCard>,
    /// Окна оболочки по порядку шаблона 0.4.9 (`deferred`, z-index — у самих окон).
    pub(super) windows: Vec<AnyView>,
}

impl Frame {
    fn new(services: &Services, cx: &mut Context<ShellRoot>) -> Frame {
        let background = cx.new(|cx| Background::new(services, cx));
        let glass = cx.new(|_| RailGlass::new(background.downgrade()));
        background.update(cx, |background, _| background.glass_over(glass.entity_id()));
        let windows = vec![
            cx.new(|cx| LibrarySetupModal::new(services, cx)).into(),
            cx.new(|cx| GameModals::new(services, cx)).into(),
            cx.new(|cx| ConfirmModal::new(services, cx)).into(),
            cx.new(|cx| SettingsModal::new(services, cx)).into(),
            cx.new(|cx| FriendsPanel::new(services, cx)).into(),
            cx.new(|cx| ProfileModal::new(services, cx)).into(),
            cx.new(|cx| ConfigSetupModal::new(services, cx)).into(),
            cx.new(|cx| ConfigTour::new(services, cx)).into(),
            cx.new(|cx| SupportThanksModal::new(services, cx)).into(),
            cx.new(|cx| SponsorsModal::new(services, cx)).into(),
            cx.new(|cx| ChangelogModal::new(services, cx)).into(),
            cx.new(|cx| WelcomeModal::new(services, cx)).into(),
        ];
        Frame {
            background,
            glass,
            rail: cx.new(|cx| GameSidebar::new(services, cx)),
            topbar: {
                let root = cx.weak_entity();
                cx.new(|_| Topbar(root))
            },
            home: None,
            configs: None,
            demo: None,
            mods: None,
            hero: cx.new(|cx| HeroPanel::new(services, cx)),
            tool: cx.new(|cx| ToolPanel::new(services, cx)),
            download_bar: cx.new(|cx| DownloadBar::new(services, cx)),
            tool_controls: cx.new(|cx| ToolControls::new(services, cx)),
            search: cx.new(|cx| RunnerSearch::new(services, cx)),
            sponsors: cx.new(|cx| SponsorsButton::new(services, cx)),
            notifications: cx.new(|cx| NotificationCenter::new(services, cx)),
            account: cx.new(|cx| AccountMenu::new(services, cx)),
            update_card: cx.new(|cx| UpdateCard::new(services, cx)),
            windows,
        }
    }

    /// Корень уведомлён — его закэшированные виды тоже: они читают то же, что и он.
    fn invalidate(&self, cx: &mut App) {
        for view in [
            self.rail.entity_id(),
            self.topbar.entity_id(),
            self.hero.entity_id(),
            self.tool.entity_id(),
            self.download_bar.entity_id(),
            self.tool_controls.entity_id(),
        ]
        .into_iter()
        .chain(self.home.as_ref().map(Entity::entity_id))
        .chain(self.configs.as_ref().map(Entity::entity_id))
        .chain(self.demo.as_ref().map(Entity::entity_id))
        .chain(self.mods.as_ref().map(Entity::entity_id))
        .chain(self.windows.iter().map(AnyView::entity_id))
        {
            cx.notify(view);
        }
    }
}

/// Корень оболочки лаунчера.
pub struct ShellRoot {
    pub(super) services: Services,
    pub(super) shell: ShellController,
    pub(super) nav: NavController,
    pub(super) logo: LogoController,
    pub(super) flows: ShellFlows,
    pub(super) prefs: Rc<RefCell<Prefs>>,
    pub(super) controls: WindowControls,
    pub(super) frame: Frame,
    /// Перекраска логотипа в пути ([`super::topbar`]).
    pub(super) logo_fade: Option<super::topbar::LogoColor>,
    toasts: Entity<Toasts>,
    pub(super) eggs: Entity<Eggs>,
    pending: Rc<RefCell<Pending>>,
    livesplit: Rc<RefCell<Option<LiveSplitAsk>>>,
    secret: SecretSequence,
    cheat: Cheat,
    /// HLSR-CE: набор кода «LEGACY».
    legacy_code: LegacyCode,
    /// HLSR-CE: интерфейс HLSR 0.2.9 вместо нового (пасхалка, до перезапуска).
    legacy: Option<Entity<LegacyShell>>,
    /// HLSR-CE: картинки 0.2.9 разбираются перед показом.
    legacy_loading: Option<Task<()>>,
    /// Фокус корня: без фокуса у элемента клавиши (F11, последовательность) идут сюда.
    focus: FocusHandle,
    /// Последняя отправленная активность Discord.
    rpc: Option<&'static str>,
    /// Что отдано фону: кадр, фон инструмента, скругление (в фон — только перемены).
    background_key: Option<(Option<&'static str>, bool, Pixels)>,
    lang_rev: i32,
    _events: Task<()>,
    /// Уведомление корня доходит до закэшированных видов рамки ([`Frame::invalidate`]).
    _invalidate: Subscription,
}

/// Перемены контроллеров → [`ShellRoot::sync`] задачей потока окна (не внутри текущего
/// обновления: контроллеры зовут слушателей и из обработчиков самого корня).
fn poker(cx: &mut Context<ShellRoot>) -> Rc<dyn Fn()> {
    let weak = cx.weak_entity();
    let app = cx.to_async();
    let executor = cx.foreground_executor().clone();
    let scheduled = Rc::new(Cell::new(false));
    Rc::new(move || {
        if scheduled.replace(true) {
            return;
        }
        let (weak, mut app, scheduled) = (weak.clone(), app.clone(), scheduled.clone());
        executor
            .spawn(async move {
                scheduled.set(false);
                weak.update(&mut app, |root, cx| root.sync(cx)).ok();
            })
            .detach();
    })
}

/// HLSR-CE: `uncappedFps` — снять предел 60 кадров/с: окна рисуют каждый vblank монитора, на
/// котором стоят (`gpui::frame_divisor` = 1), и интерфейс идёт с его частотой.
fn apply_frame_cap(prefs: &Prefs) {
    gpui::set_frame_cap(prefs.get("uncappedFps") != Some(&Value::Bool(true)));
}

/// Язык окна: `settings.config.language` хранилища (0 — en, 1 — ru).
pub(crate) fn language_of(services: &Services) -> Language {
    let settings = match services.store_read("settings") {
        StoreRead::Found(settings) => settings,
        StoreRead::Missing | StoreRead::Invalid => Value::Null,
    };
    settings
        .pointer("/config/language")
        .and_then(Value::as_i64)
        .and_then(Language::from_index)
        .unwrap_or(Language::En)
}

impl ShellRoot {
    /// `data()` и `mounted()` оболочки.
    /// Порядок вызовов главного процесса — как у `LauncherShell.vue` 0.4.9: `created` (версия,
    /// Discord, эпизоды), дети (шапка, кнопки окна), `mounted` (сценарии). `default_install_path`
    /// — `sendSync` из `preload.js` (его шлёт `LauncherRoot` до `ready`).
    pub fn new(
        services: Services,
        default_install_path: String,
        cx: &mut Context<ShellRoot>,
    ) -> ShellRoot {
        // `ready` (App.vue `mounted`) шлёт корень окна `LauncherRoot` — до заставки.
        let env = GpuiEnv::new(cx);
        let poke = poker(cx);
        let paths = cx.try_global::<AppPaths>().map(|paths| &paths.0);
        let prefs = Rc::new(RefCell::new(Prefs::open(paths)));
        let language = language_of(&services);
        let pending: Rc<RefCell<Pending>> = Rc::default();

        let shell = ShellController::new(env.clone(), language);
        {
            let (pending, poke) = (pending.clone(), poke.clone());
            shell.on_toast(move |text, icon, ms| {
                pending
                    .borrow_mut()
                    .toasts
                    .push((text.to_owned(), icon.to_owned(), ms));
                poke();
            });
        }
        shell.on_change({
            let poke = poke.clone();
            move || poke()
        });
        let nav = NavController::new(&shell, Rc::new(NavPrefsOf(prefs.clone())));
        nav.on_change({
            let poke = poke.clone();
            move || poke()
        });
        let logo = LogoController::new(env.clone(), &shell);
        logo.on_change({
            let poke = poke.clone();
            move || poke()
        });

        let livesplit: Rc<RefCell<Option<LiveSplitAsk>>> = Rc::default();
        let port = Rc::new(Port {
            shell: shell.clone(),
            env: env.clone(),
            prefs: prefs.clone(),
            pending: pending.clone(),
            livesplit: livesplit.clone(),
            poke,
            clock: WallClock::system(),
        });
        let library = match services.store_read("library") {
            StoreRead::Found(library) => library,
            StoreRead::Missing | StoreRead::Invalid => Value::Null,
        };
        let flows = ShellFlows::new(
            services.clone(),
            port,
            ShellInit {
                prefs: prefs.borrow().data().clone(),
                library: library.clone(),
                default_install_path,
                language: language.code().to_owned(),
            },
        );
        let weak = flows.downgrade();
        shell.on_unlock_achievement(move |id| {
            if let Some(flows) = weak.upgrade() {
                flows.unlock_achievement(id);
            }
        });
        let weak = flows.downgrade();
        nav.on_game_selected(move |id| {
            if let Some(flows) = weak.upgrade() {
                flows.on_select_game(id);
            }
        });
        let weak = flows.downgrade();
        nav.on_tool_selected(move |id| {
            if let Some(flows) = weak.upgrade() {
                flows.on_select_tool(id);
            }
        });
        let weak = flows.downgrade();
        nav.on_footer_tool(move |id| {
            if let Some(flows) = weak.upgrade() {
                flows.on_tool(id);
            }
        });

        cx.set_global(ShellContext {
            services: services.clone(),
            shell: shell.clone(),
            nav: nav.clone(),
            flows: flows.clone(),
            root: cx.weak_entity(),
        });
        let no_anim = prefs.borrow().get("animations") == Some(&Value::Bool(false));
        AnimGate::set_no_anim(cx, no_anim);
        apply_frame_cap(&prefs.borrow());

        let events = crate::bridge::listen(cx, services.subscribe(), |root, event, cx| {
            root.on_event(event, cx)
        });
        // Сборка могла смениться до подписки (`build-official` ушло бы мимо): перечитать.
        let official = flows.with_state(|state| {
            state.build = services.build();
            state.official()
        });
        nav.show_online(official);
        // created(): `watch.rpcActivity` и `watch.episodeCardShown` (immediate) — до детей.
        let activity = nav.rpc_activity();
        services.send("discord-rpc-activity", vec![json!(activity)]);
        let shows_game = nav.section() == "games" && nav.active_tab() == "games";
        flows.set_episode_card_shown(
            shows_game
                && !game_demo::demo_state(&flows.current_game_id())
                    .episodes
                    .is_empty(),
        );
        let frame = Frame::new(&services, cx);
        // Дети: кнопки окна спрашивают полный экран после шапки.
        WindowControls::query(cx);
        let mut root = ShellRoot {
            controls: WindowControls::default(),
            logo_fade: None,
            toasts: cx.new(|_| Toasts::new()),
            eggs: cx.new(|_| Eggs::new()),
            frame,
            services,
            shell,
            nav,
            logo,
            flows,
            prefs,
            pending,
            livesplit,
            secret: SecretSequence::default(),
            cheat: Cheat::default(),
            legacy_code: LegacyCode::default(),
            legacy: None,
            legacy_loading: None,
            focus: cx.focus_handle(),
            rpc: Some(activity),
            background_key: None,
            lang_rev: 0,
            _events: events,
            _invalidate: cx.observe_self(|root, cx| root.frame.invalidate(cx)),
        };
        // mounted(): игра при запуске (и для Discord), библиотека, загрузки, статусы,
        // `support-thanks-ready`, онлайн.
        root.flows.mount(library.get("lastLaunched"));
        // HLSR-CE: отладочная сборка может стартовать сразу в Legacy (снимки и проверка вида).
        if cfg!(debug_assertions) && std::env::var_os("HLSR_CE_LEGACY").is_some() {
            root.open_legacy(true, cx);
        }
        root.sync(cx);
        root
    }

    /// Корень окна, если оболочка смонтирована.
    pub fn get(cx: &App) -> Option<Entity<ShellRoot>> {
        ShellContext::get(cx)?.root.upgrade()
    }

    pub fn shell(&self) -> &ShellController {
        &self.shell
    }

    pub fn nav(&self) -> &NavController {
        &self.nav
    }

    pub fn flows(&self) -> &ShellFlows {
        &self.flows
    }

    pub fn services(&self) -> &Services {
        &self.services
    }

    /// Страница «Персонализация», если её уже открывали (`$refs.configsPanel`, `keep-alive`).
    pub fn configs_page(&self) -> Option<Entity<GameConfigsPanel>> {
        self.frame.configs.clone()
    }

    /// prefs лаунчера в памяти.
    pub fn prefs(&self) -> Rc<RefCell<Prefs>> {
        self.prefs.clone()
    }

    /// `overlayOpen`: открыто окно-диалог оболочки.
    pub fn overlay_open(cx: &App) -> bool {
        Layers::open_count(cx) > 0
    }

    /// `notify(text, icon, ms)`.
    pub fn notify(&self, text: &str, icon: &str, ms: u64) {
        self.shell.notify_for(text, icon, ms);
    }

    /// Открытый запрос окна запуска LiveSplit (берёт окно G3; ответ — `reply`).
    pub fn livesplit_request(&self) -> Option<LiveSplitAsk> {
        self.livesplit.borrow_mut().take()
    }

    /// `applyPrefs` (окно настроек): новые prefs в память и файл; без анимаций — `.no-anim`;
    /// кнопку полного экрана убрали — полный экран снимается (`watch.fullscreenButton`).
    pub fn apply_prefs(&mut self, changes: &Map<String, Value>, cx: &mut Context<ShellRoot>) {
        self.prefs.borrow_mut().save(changes);
        self.flows.with_state(|state| {
            for (key, value) in changes {
                state.prefs.insert(key.clone(), value.clone());
            }
        });
        let prefs = self.prefs.borrow();
        AnimGate::set_no_anim(cx, prefs.get("animations") == Some(&Value::Bool(false)));
        apply_frame_cap(&prefs);
        let button = prefs.get("fullscreenButton") != Some(&Value::Bool(false));
        drop(prefs);
        if !button && self.controls.fullscreen {
            WindowControls::command("fullscreen-exit", cx);
        }
        cx.notify();
    }

    /// `<LauncherBackground>`: кадр выбранной игры в «Играх» на вкладке игр (`backgroundSrc`, у
    /// эпизода — свой кадр без переднего плана), фон LiveSplit на вкладке инструментов
    /// (`tool-background`), скругление окна (в полном экране 0).
    fn sync_background(&mut self, radius: Pixels, cx: &mut Context<ShellRoot>) {
        let (section, tab) = (self.nav.section(), self.nav.active_tab());
        let tool = section == "games" && tab == "tools";
        // HLSR-CE: за страницей мода — арт Half-Life 2.
        let mod_art = (section == "games" && tab == "mods")
            .then(|| Selection::of_game("220"))
            .flatten();
        let selection = (section == "games" && tab == "games")
            .then(|| {
                let game = self.flows.current_game_id();
                self.flows
                    .current_episode_id()
                    .and_then(|_| Selection::of_episodes(&game))
                    .or_else(|| Selection::of_game(&game))
            })
            .flatten()
            .or(mod_art);
        let key = (selection.as_ref().map(|s| s.hero), tool, radius);
        if self.background_key == Some(key) {
            return;
        }
        self.background_key = Some(key);
        self.frame.background.update(cx, |background, cx| {
            background.show(selection, cx);
            background.set_tool(tool, cx);
            background.set_radius(radius, cx);
        });
    }

    /// `watch.rpcActivity` (immediate): Discord узнаёт раздел при каждой смене.
    fn send_rpc_activity(&mut self) {
        let activity = self.nav.rpc_activity();
        if self.rpc != Some(activity) {
            self.rpc = Some(activity);
            self.services
                .send("discord-rpc-activity", vec![json!(activity)]);
        }
    }

    /// События главного процесса (`ipcRenderer.on` оболочки и `WindowControls`).
    fn on_event(&mut self, event: ServiceEvent, cx: &mut Context<ShellRoot>) {
        if event.channel == "launcher-fullscreen-changed" {
            self.controls
                .set_fullscreen(event.payload == Value::Bool(true), cx);
            return;
        }
        self.flows.on_event(&event.channel, &event.payload);
    }

    /// Перемены контроллеров → вид: тосты, буфер обмена, игра и данные колонки, язык, Discord.
    pub(super) fn sync(&mut self, cx: &mut Context<ShellRoot>) {
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        for (text, icon, ms) in pending.toasts {
            self.toasts
                .update(cx, |toasts, cx| toasts.notify(text, Some(&icon), ms, cx));
        }
        if let Some(text) = pending.clipboard {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        if !pending.changes.is_empty() {
            // Опрос о конфиге и шестерёнка: «Персонализация» на игре (`configsGameId`, `section`).
            if let Some(game) = self.flows.with_state(|state| state.configs_open.take()) {
                self.nav.set_configs_game(&game);
                self.nav.select_section("configs");
            }
            // `onSelectTool` из сценария (предложение LiveSplit): вкладка инструмента.
            if let Some(tool) = self.flows.with_state(|state| state.tool_open.take()) {
                self.nav.show_tool(&tool);
            }
            let state = self.flows.state();
            for what in pending.changes {
                match what {
                    ShellChange::Selection
                        if self.nav.selected_game() != state.selected_game_id =>
                    {
                        self.nav.set_selected_game(&state.selected_game_id);
                    }
                    ShellChange::Online => self.nav.set_online(&state.online_snapshot),
                    ShellChange::Build => {
                        self.nav.show_online(state.official());
                        if !state.official() {
                            crate::sponsors::modal::close_sponsors(cx);
                        }
                    }
                    ShellChange::Tools => self.nav.set_tools(&Value::Array(state.tools.clone())),
                    _ => {}
                }
            }
        }
        if self.shell.lang_rev() != self.lang_rev {
            self.lang_rev = self.shell.lang_rev();
            self.frame = Frame::new(&self.services, cx);
            self.background_key = None;
        }
        self.send_rpc_activity();
        cx.notify();
    }

    /// Щелчок по вкладке шапки.
    pub(super) fn select_section(&mut self, id: &str) {
        self.nav.select_section(id);
    }

    /// `onGlobalKeydown` (секретная последовательность и HESOYAM) и F11 `WindowControls`.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if key == "f11" {
            if !event.is_held {
                self.controls.toggle(cx);
            }
            return;
        }
        // Поле ввода или окно поверх — последовательность сбрасывается.
        let in_field = window
            .context_stack()
            .iter()
            .any(|context| context.contains("TextInput") || context.contains("CodeArea"));
        if in_field || Self::overlay_open(cx) {
            self.secret.reset();
            self.cheat.reset();
            self.legacy_code.reset();
            return;
        }
        let code = dom_code(key);
        if self.secret.push(&code) {
            self.cheat.reset();
            self.legacy_code.reset();
            self.shell.unlock_achievement("wake-up");
        } else if self.cheat.push(&code) {
            self.secret.reset();
            self.legacy_code.reset();
            self.eggs.update(cx, |eggs, cx| eggs.show_cheat(cx));
        } else if self.legacy_code.push(&code) {
            self.secret.reset();
            self.cheat.reset();
            self.toggle_legacy(cx);
        }
    }

    /// HLSR-CE: код «LEGACY» — интерфейс HLSR 0.2.9 ([`LegacyShell`]) или обратно новый.
    /// Окна-диалоги, тосты и пасхалки нового лаунчера остаются поверх: их открывают сценарии
    /// (установка, запуск, параметры игры), которые Legacy зовёт.
    pub fn toggle_legacy(&mut self, cx: &mut Context<Self>) {
        if let Some(legacy) = self.legacy.take() {
            // Картинки 0.2.9 уходят из памяти и атласа вместе с интерфейсом.
            for image in legacy.read(cx).images() {
                cx.drop_image(image, None);
            }
            let text = self.shell.tr("#L_LEGACY_OFF", &[]);
            self.shell.notify(&text, "fas fa-rocket");
            cx.notify();
        } else {
            self.open_legacy(false, cx);
        }
    }

    /// HLSR-CE: интерфейс 0.2.9 показывается, когда все его картинки разобраны (в фоне, пакетом):
    /// иначе страницы появлялись бы без картинок и они догружались бы на глазах.
    fn open_legacy(&mut self, debug_page: bool, cx: &mut Context<Self>) {
        if self.legacy_loading.is_some() {
            return;
        }
        self.legacy_loading = Some(cx.spawn(async move |this, cx| {
            let images = cx
                .background_executor()
                .spawn(async { crate::legacy::decode_images() })
                .await;
            this.update(cx, |root, cx| {
                root.legacy_loading = None;
                let shell = LegacyShell::new(images);
                let shell = if debug_page {
                    shell.with_debug_page()
                } else {
                    shell
                };
                root.legacy = Some(cx.new(|_| shell));
                if !debug_page {
                    let text = root.shell.tr("#L_LEGACY_ON", &[]);
                    root.shell.notify(&text, "fas fa-history");
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// HLSR-CE: открыт ли интерфейс 0.2.9.
    pub fn legacy_active(&self) -> bool {
        self.legacy.is_some()
    }

    /// Раздел страницы рядом с колонкой.
    fn page(&mut self, window: &mut Window, cx: &mut Context<ShellRoot>) -> gpui::AnyElement {
        let section = self.nav.section();
        let tab = self.nav.active_tab();
        let build = self.flows.with_state(|state| state.build);
        // `v-if` без `keep-alive`: «Главная» создаётся при входе и уходит с разделом. Не
        // официальная сборка «Главную» не создаёт: ни ленты, ни рекордов, ни игроков, ни запросов.
        if section != "home" || build != Standing::Official {
            self.frame.home = None;
        }
        let services = self.services.clone();
        let cell = || div().flex_1().min_w_0().h_full().relative();
        let on = crate::cache::on(cx);
        let full = || StyleRefinement::default().size_full();
        match (section.as_str(), tab.as_str()) {
            // Пока сервер не ответил — пусто; самосборка — заглушка со ссылкой на выпуски.
            ("home", _) if build != Standing::Official => cell()
                .flex()
                .items_center()
                .justify_center()
                .children((build == Standing::Unofficial).then(|| {
                    crate::common::official_stub(
                        "home-official-releases",
                        "#L_OFFICIAL_HOME",
                        15.,
                        window,
                        cx,
                    )
                    .items_center()
                }))
                .into_any_element(),
            ("home", _) => {
                let home = self
                    .frame
                    .home
                    .get_or_insert_with(|| cx.new(|cx| HomePanel::new(&services, cx)))
                    .clone();
                cell().child(cached(home, full(), on)).into_any_element()
            }
            ("configs", _) => {
                let configs = self
                    .frame
                    .configs
                    .get_or_insert_with(|| cx.new(|cx| GameConfigsPanel::new(&services, cx)))
                    .clone();
                cell().child(cached(configs, full(), on)).into_any_element()
            }
            (_, "mods") => {
                let mods = self
                    .frame
                    .mods
                    .get_or_insert_with(|| cx.new(|cx| ModPage::new(&services, cx)))
                    .clone();
                cell().child(cached(mods, full(), on)).into_any_element()
            }
            (_, "demo") => {
                let demo = self
                    .frame
                    .demo
                    .get_or_insert_with(|| cx.new(|cx| DemoPanel::new(&services, cx)))
                    .clone();
                cell().child(cached(demo, full(), on)).into_any_element()
            }
            _ => {
                // .launcher-main: сцена (герой игры или инструмента) и низ (панель игры или
                // кнопки инструмента); игра и инструмент перетекают друг в друга (`l-value`).
                let tool = tab == "tools";
                let (hero, tool_hero) = (self.frame.hero.clone(), self.frame.tool.clone());
                let (bar, controls) = (
                    self.frame.download_bar.clone(),
                    self.frame.tool_controls.clone(),
                );
                let tool_key = format!("tool-{}", self.nav.selected_tool());
                let stage_key = if tool { tool_key } else { "games".to_owned() };
                // Нижние панели — полоса `--l-bottombar-h`, растушёвка над ней — вне места.
                let dock = StyleRefinement::default()
                    .w_full()
                    .h(cx.global::<Theme>().rem("l-bottombar-h"));
                cell()
                    .flex()
                    .flex_col()
                    .child(
                        swap("launcher-stage", stage_key, VALUE, move |key: &String| {
                            if key == "games" {
                                div().size_full().child(cached(hero.clone(), full(), on))
                            } else {
                                div()
                                    .size_full()
                                    .child(cached(tool_hero.clone(), full(), on))
                            }
                        })
                        .flex_1()
                        .min_h_0()
                        .relative(),
                    )
                    .child(
                        swap("launcher-dock", tool, VALUE, move |tool: &bool| {
                            if *tool {
                                div()
                                    .w_full()
                                    .child(cached(controls.clone(), dock.clone(), on))
                            } else {
                                div().w_full().child(cached(bar.clone(), dock.clone(), on))
                            }
                        })
                        .flex_none()
                        .relative(),
                    )
                    .into_any_element()
            }
        }
    }
}

/// `event.code` Chromium по клавише GPUI: стрелки и буквы — то, что ждёт последовательность.
/// shortcut: GPUI на Windows даёт букву по VK (раскладка не меняет её) — для `KeyA`/`KeyB`
/// хватает; полная таблица физических кодов — F9 (`keys/`), если понадобятся другие клавиши.
fn dom_code(key: &str) -> String {
    match key {
        "up" => "ArrowUp".into(),
        "down" => "ArrowDown".into(),
        "left" => "ArrowLeft".into(),
        "right" => "ArrowRight".into(),
        letter if letter.len() == 1 && letter.as_bytes()[0].is_ascii_alphabetic() => {
            format!("Key{}", letter.to_ascii_uppercase())
        }
        other => other.to_owned(),
    }
}

impl Render for ShellRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        let fullscreen = self.controls.fullscreen;
        let radius = Layers::radius(cx);
        self.sync_background(radius, cx);
        // Ничего не в фокусе — фокус у корня (как keydown у window в 0.4.9).
        if window.focused(cx).is_none() {
            let focus = self.focus.clone();
            window.on_next_frame(move |window, cx| {
                if window.focused(cx).is_none() {
                    window.focus(&focus, cx);
                }
            });
        }
        let legacy = self.legacy.clone();
        let legacy_on = legacy.is_some();
        let page = if legacy_on {
            div().into_any_element()
        } else {
            self.page(window, cx)
        };
        let on = crate::cache::on(cx);
        let topbar = StyleRefinement::default()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(theme.rem("l-topbar-h"));
        // Место колонки и слой стекла под ней — по её выдвижению сейчас (идёт в ней самой).
        let still = AnimGate::still(cx).any();
        let open = self.nav.rail_open();
        let now = cx.background_executor().now();
        let t = self
            .frame
            .rail
            .update(cx, |rail, _| rail.swipe_now(open, &theme, still, now));
        self.frame.glass.update(cx, |glass, _| glass.set_open(t));
        let (closed_w, open_w) = (theme.len("l-rail-w"), theme.len("l-rail-open-w"));
        let rail = StyleRefinement::default()
            .flex_none()
            .h_full()
            .w(crate::scale::u(closed_w + (open_w - closed_w) * t));
        let root = div()
            .id("launcher-root")
            .key_context("LauncherShell")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .rounded(radius)
            .bg(theme.color("l-bg"))
            .font_family(crate::text::RUBIK)
            .text_color(theme.color("l-text"))
            .on_key_down(cx.listener(Self::on_key_down))
            .when_some(legacy, |root, legacy| {
                // HLSR-CE: интерфейс 0.2.9 вместо рамки нового (пасхалка «LEGACY»).
                root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(radius)
                        .overflow_hidden()
                        .child(legacy),
                )
            })
            .when(!legacy_on, |root| {
                root.children(app_background(&theme, radius))
                    // .launcher-frame: колонка и страница в одной строке, фон — слой под ними.
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .child(cached(
                                self.frame.background.clone(),
                                StyleRefinement::default().absolute().inset_0(),
                                on,
                            ))
                            .child(self.frame.glass.clone())
                            .child(cached(self.frame.rail.clone(), rail, on))
                            .child(page),
                    )
                    .child(cached(self.frame.topbar.clone(), topbar, on))
            })
            // Окна-диалоги рисуют себя `deferred` + `anchored` по окну: место — всё окно.
            .children(self.frame.windows.iter().map(|view| {
                let style = StyleRefinement::default().absolute().inset_0();
                cached(view.clone(), style, on)
            }))
            .when(!legacy_on, |root| {
                root.child(self.frame.update_card.clone())
            })
            .child(self.toasts.clone())
            .child(self.eggs.clone())
            // #app::after: контур окна inset 0 0 0 1px rgba(255,255,255,.12); в полном экране нет.
            .when(!fullscreen, |root| {
                root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(radius)
                        .border_1()
                        .border_color(white().opacity(0.12)),
                )
            });
        let root = tab_root(root);
        // `window-switching`: гаснет .13 с (`--l-ease-in`), проявляется .24 с (`--l-ease`).
        let (target, timing) = if self.controls.switching {
            (0., Timing::new(130., [0.55, 0., 0.75, 0.2]))
        } else {
            (1., Timing::new(240., [0.22, 1., 0.36, 1.]))
        };
        tween("window-switching", target, timing, move |opacity| {
            root.opacity(opacity)
        })
    }
}

/// Подложка `.launcher-root` 0.4.9: `--l-app-image` (`gradient.jpg`, `cover`) под
/// `--l-app-scrim`. Видна только в Liquid Glass — у прочих тем запасной фон `--l-bg-fallback`
/// непрозрачен и закрывает её, там корню хватает `--l-bg`.
fn app_background(theme: &Theme, radius: Pixels) -> Option<gpui::Div> {
    crate::liquid::active(theme).then(|| {
        div()
            .absolute()
            .inset_0()
            .child(
                gpui::img(crate::liquid::APP_IMAGE)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover)
                    .rounded(radius),
            )
            .children(crate::paint::strips(&theme.gradient("l-app-scrim"), radius))
    })
}
