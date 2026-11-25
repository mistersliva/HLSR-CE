//! Настройки лаунчера (`SettingsModal.vue` 0.4.9). Открывает оболочка (`settingsOpen`,
//! `settingsSection` в [`ShellFlows`]: колонка, «Недоступно», уведомления). Окно как у профиля:
//! шапка со значком и вкладками (`l-ink`), разделы «Общие / Загрузки / Proton (Linux) /
//! Интерфейс / Диагностика / О программе» со сменой `l-slide-*` и своей прокруткой, подвал
//! «Сбросить / Отмена / Сохранить». «Сохранить» окно не закрывает (закрывает только смена
//! языка, язык меняется последним). Тема применяется сразу ([`set_theme`]) и в сброс не входит.
//! Строки «Режим отрисовки» нет (решение владельца 4).
//!
//! Логика без окна — `hlsr_ui::controllers::settings`; здесь — шаги окна и `applyPrefs`
//! оболочки (`ShellRoot::apply_prefs`, трей, сплиты LiveSplit, папка библиотеки, язык).

mod sections;
#[cfg(test)]
mod tests;
mod themes;

use crate::kit::{FieldEvent, TextInput};
use crate::layer::Modal;
use crate::motion::AnimGate;
use crate::scroll::Scroll;
use crate::shell::root::{ShellContext, ShellRoot};
use crate::theme::Theme;
use gpui::{App, AppContext, Context, Entity, IntoElement, Render, Subscription, Task, Window};
use hlsr_core::js::truthy;
use hlsr_core::language::Language;
use hlsr_ui::controllers::custom_mods::{ModsView, Note, note_for};
use hlsr_ui::controllers::settings::{
    self as logic, DiagCheck, DiscordRpc, Draft, ProtonStatus, Section,
};
use hlsr_ui::controllers::shell_flows::ShellFlows;
use hlsr_ui::services::{ServiceEvent, Services, StoreRead};
use serde_json::{Map, Value, json};
use std::collections::{BTreeSet, HashMap};
use std::time::Instant;

/// Ключи prefs, которые окно не пишет: `ruleCategory`/`keyboardFull` пишут свои функции
/// (`prefsStore.save` их не затирает), `pane` в 0.4.9 — отдельные ключи localStorage.
const NOT_DRAFT: [&str; 3] = ["ruleCategory", "keyboardFull", "pane"];
const ERROR_ICON: &str = "fas fa-exclamation-triangle";

/// `data()` открытого окна.
#[derive(Clone)]
pub(super) struct State {
    lang: Language,
    linux: bool,
    sections: Vec<Section>,
    section: usize,
    slide_next: bool,
    version: String,
    /// Выбранный язык (`language`), применяется при «Сохранить».
    language: String,
    draft: Draft,
    ui_scale: f64,
    discord: DiscordRpc,
    path_error: Option<String>,
    download_error: Option<String>,
    path_cyrillic: bool,
    proton: ProtonStatus,
    proton_choice: Option<String>,
    proton_error: Option<String>,
    diag_checks: Vec<DiagCheck>,
    diag_running: bool,
    diag_error: Option<String>,
    diag_finished: bool,
    /// Раскрытые `<details>` проверок (по id).
    details: BTreeSet<String>,
    export_open: bool,
    export_include_dump: bool,
    export_busy: bool,
    export_opening: bool,
    crash_dump: bool,
    /// HLSR-CE: «Свои моды» — снимок, идёт действие, заметка последнего действия.
    mods: ModsView,
    mods_busy: bool,
    mods_note: Option<Note>,
}

impl State {
    fn new(lang: Language) -> State {
        State {
            lang,
            linux: cfg!(target_os = "linux"),
            sections: Vec::new(),
            section: 0,
            slide_next: true,
            version: String::new(),
            language: lang.code().to_owned(),
            draft: Draft::default(),
            ui_scale: logic::DEFAULT_SCALE,
            discord: DiscordRpc::default(),
            path_error: None,
            download_error: None,
            path_cyrillic: false,
            proton: ProtonStatus::idle(),
            proton_choice: None,
            proton_error: None,
            diag_checks: Vec::new(),
            diag_running: false,
            diag_error: None,
            diag_finished: false,
            details: BTreeSet::new(),
            export_open: false,
            export_include_dump: false,
            export_busy: false,
            export_opening: false,
            crash_dump: false,
            mods: ModsView::default(),
            mods_busy: false,
            mods_note: None,
        }
    }

    fn t(&self, key: &str) -> String {
        hlsr_ui::tr(self.lang, key, &[])
    }

    fn section_id(&self) -> &'static str {
        self.sections.get(self.section).map_or("general", |s| s.id)
    }
}

/// Одно «Сохранить»: окно, из которого начато, и его значения (`this` компонента 0.4.9).
struct Save {
    generation: u64,
    state: State,
}

pub struct SettingsModal {
    services: Services,
    modal: Modal,
    state: State,
    /// Растёт при каждом открытии и закрытии: ответы прежнего окна не применяются.
    generation: u64,
    path: Option<Entity<TextInput>>,
    scrolls: Vec<Scroll>,
    _path_events: Option<Subscription>,
    /// HLSR-CE: поля параметров запуска модов (по id мода) и их подписки.
    mod_args: HashMap<String, (Entity<TextInput>, Subscription)>,
    _events: Task<()>,
}

impl SettingsModal {
    pub fn new(services: &Services, cx: &mut Context<SettingsModal>) -> SettingsModal {
        let events = crate::bridge::listen(cx, services.subscribe(), |this, event, cx| {
            this.on_event(event, cx)
        });
        let lang = crate::shell::root::language_of(services);
        SettingsModal {
            services: services.clone(),
            modal: Modal::new("settings", cx),
            state: State::new(lang),
            generation: 0,
            path: None,
            scrolls: Vec::new(),
            _path_events: None,
            mod_args: HashMap::new(),
            _events: events,
        }
    }

    fn flows(cx: &App) -> Option<ShellFlows> {
        ShellContext::get(cx).map(|shell| shell.flows.clone())
    }

    /// Что хочет оболочка: окно открыто и раздел при открытии.
    fn wanted(cx: &App) -> (bool, String) {
        Self::flows(cx).map_or((false, String::new()), |flows| {
            flows.with_state(|s| (s.settings_open, s.settings_section.clone()))
        })
    }

    fn notify(cx: &App, key: &str, icon: &str) {
        if let Some(shell) = ShellContext::get(cx) {
            shell.shell.notify(&shell.shell.tr(key, &[]), icon);
        }
    }

    /// `v-if="settingsOpen"`: новое окно — `data()` и `mounted()`; закрыто — ответы не нужны.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (open, section) = Self::wanted(cx);
        if open && !self.modal.is_open() {
            self.mount(&section, window, cx);
            self.modal.open(window, cx);
        } else if !open && self.modal.is_open() {
            self.generation += 1;
            self.modal.close(window, cx);
        }
        cx.notify();
    }

    fn mount(&mut self, section: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.generation += 1;
        let lang = ShellContext::get(cx).map_or(self.state.lang, |shell| shell.shell.language());
        let mut state = State::new(lang);
        state.sections = logic::sections(state.linux);
        state.section = logic::initial_section(state.linux, section);
        state.version = hlsr_core::js::to_string(&self.services.send_sync("app-version", vec![]));
        let mut prefs = ShellRoot::get(cx)
            .map(|root| root.read(cx).prefs().borrow().data().clone())
            .unwrap_or_default();
        for key in NOT_DRAFT {
            prefs.shift_remove(key);
        }
        let install_path = Self::flows(cx)
            .map(|flows| flows.with_state(|s| s.install_path.clone()))
            .unwrap_or_default();
        state.draft = Draft::new(&prefs, &install_path);
        state.ui_scale =
            logic::normalize_scale(self.config().get("uiScale").unwrap_or(&Value::Null));
        self.state = state;
        self.scrolls = (0..self.state.sections.len())
            .map(|_| Scroll::new())
            .collect();
        let path = match &self.path {
            Some(path) => path.clone(),
            None => {
                let path = cx.new(|cx| TextInput::new(window, cx).style(sections::path_style));
                self._path_events = Some(cx.subscribe(&path, |this, input, event, cx| {
                    if *event == FieldEvent::Input {
                        input.update(cx, |input, cx| input.set_invalid(false, cx));
                        this.state.path_error = None;
                        this.state.path_cyrillic = false;
                        cx.notify();
                    }
                }));
                self.path = Some(path.clone());
                path
            }
        };
        path.update(cx, |input, cx| {
            input.set_text(install_path, cx);
            input.set_invalid(false, cx);
        });
        // mounted(): Proton (Linux) и Discord.
        if self.state.linux {
            self.refresh_protons(cx);
        }
        self.load_discord_rpc(cx);
        if !self.state.linux {
            self.load_mods(cx);
        }
    }

    /// `$emit("close")` → `closeSettings`.
    fn close(&mut self, cx: &mut Context<Self>) {
        if let Some(flows) = Self::flows(cx) {
            flows.close_settings();
        }
        cx.notify();
    }

    /// Шаг с ответом главного процесса: `then` — в окне, если оно то же.
    fn ask(
        &mut self,
        channel: &str,
        args: Vec<Value>,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<Value, String>, &mut Context<Self>) + 'static,
    ) {
        let reply = self.services.invoke(channel, args);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let reply = reply.await;
            this.update(cx, |this, cx| {
                if this.generation == generation {
                    then(this, reply, cx);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Шаг «Сохранить»: `submit` 0.4.9 идёт до конца и после закрытия окна, поэтому `then` —
    /// всегда; писать в окно — только если оно то же ([`Self::live`]).
    fn step(
        &self,
        save: Save,
        channel: &str,
        args: Vec<Value>,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Save, Result<Value, String>, &mut Context<Self>) + 'static,
    ) {
        let reply = self.services.invoke(channel, args);
        cx.spawn(async move |this, cx| {
            let reply = reply.await;
            this.update(cx, |this, cx| {
                then(this, save, reply, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn live(&self, save: &Save) -> bool {
        self.generation == save.generation
    }

    /// `ipcRenderer.invoke` без ожидания ответа.
    fn fire(&self, channel: &str, args: Vec<Value>, cx: &mut Context<Self>) {
        let reply = self.services.invoke(channel, args);
        cx.background_spawn(async move {
            let _ = reply.await;
        })
        .detach();
    }

    fn on_event(&mut self, event: ServiceEvent, cx: &mut Context<Self>) {
        if !self.modal.is_open() {
            return;
        }
        match event.channel.as_str() {
            "diagnostics-progress" if self.state.diag_running => {
                logic::upsert_check(
                    &mut self.state.diag_checks,
                    DiagCheck::from_value(&event.payload),
                );
            }
            "discord-rpc-state" => {
                if let Some(discord) = DiscordRpc::from_value(&event.payload) {
                    self.state.discord = discord;
                }
            }
            "proton-state" if self.state.linux => {
                let previous = self.state.proton.selected.clone();
                let status = ProtonStatus::from_value(&event.payload);
                self.state.proton_choice = logic::choice_after_state(
                    self.state.proton_choice.take(),
                    previous.as_deref(),
                    &status,
                );
                self.state.proton = status;
            }
            _ => return,
        }
        cx.notify();
    }

    // ---- Разделы и черновик ---------------------------------------------------------------------

    pub(super) fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index == self.state.section || index >= self.state.sections.len() {
            return;
        }
        self.state.slide_next = logic::slide_direction(self.state.section, index) == "next";
        self.state.section = index;
        if self.state.section_id() == "proton" {
            self.refresh_protons(cx);
        }
        if self.state.section_id() == "mods" {
            self.load_mods(cx);
        }
        cx.notify();
    }

    fn go_to(&mut self, id: &str) {
        if let Some(index) = self.state.sections.iter().position(|s| s.id == id) {
            if index != self.state.section {
                self.state.slide_next = index >= self.state.section;
            }
            self.state.section = index;
        }
    }

    pub(super) fn set_flag(&mut self, key: &str, on: bool, cx: &mut Context<Self>) {
        self.state.draft.set(key, json!(on));
        cx.notify();
    }

    pub(super) fn set_language(&mut self, id: &str, cx: &mut Context<Self>) {
        self.state.language = id.to_owned();
        cx.notify();
    }

    pub(super) fn set_speed(&mut self, id: &str, cx: &mut Context<Self>) {
        let limit = logic::normalize_speed_limit(&json!(id.parse::<f64>().unwrap_or(0.)));
        self.state.draft.set("speedLimit", json!(limit));
        self.state.download_error = None;
        cx.notify();
    }

    pub(super) fn set_scale(&mut self, id: &str, cx: &mut Context<Self>) {
        self.state.ui_scale = logic::normalize_scale(&json!(id.parse::<f64>().unwrap_or(0.)));
        cx.notify();
    }

    /// `onTheme(id, accent)`: черновик следит за темой, оболочка применяет её сразу.
    pub(super) fn on_theme(
        &mut self,
        id: &str,
        accent: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.draft.set("launcherTheme", json!(id));
        if let Some(accent) = accent {
            self.state.draft.set("paperAccent", json!(accent));
        }
        set_theme(id, accent, window, cx);
        cx.notify();
    }

    fn path_text(&self, cx: &App) -> String {
        self.path
            .as_ref()
            .map(|path| path.read(cx).text().to_owned())
            .unwrap_or_else(|| self.state.draft.install_path.clone())
    }

    fn clear_path_error(&mut self, cx: &mut Context<Self>) {
        self.state.path_error = None;
        if let Some(path) = &self.path {
            path.update(cx, |input, cx| input.set_invalid(false, cx));
        }
    }

    fn set_path_text(&self, text: &str, cx: &mut Context<Self>) {
        if let Some(path) = &self.path {
            path.update(cx, |input, cx| input.set_text(text, cx));
        }
    }

    /// `resetDraft`: умолчания (тема — нет), путь по умолчанию, масштаб 100 %, тост.
    pub(super) fn reset(&mut self, cx: &mut Context<Self>) {
        let prefs = ShellRoot::get(cx)
            .map(|root| root.read(cx).prefs().borrow().data().clone())
            .unwrap_or_default();
        let default_path =
            hlsr_core::js::to_string(&self.services.send_sync("app-default-library-path", vec![]));
        let mut draft = Draft::reset(&prefs, &default_path);
        for key in NOT_DRAFT {
            draft.prefs.shift_remove(key);
        }
        self.set_path_text(&draft.install_path, cx);
        self.state.draft = draft;
        self.clear_path_error(cx);
        self.state.download_error = None;
        self.state.ui_scale = logic::DEFAULT_SCALE;
        Self::notify(cx, "#L_SETTINGS_RESET", "fas fa-undo");
        cx.notify();
    }

    // ---- settings.json ------------------------------------------------------------------------

    /// `settings.get("config")`: объект, иначе пустой; файла нет — умолчания.
    fn config(&self) -> Map<String, Value> {
        let data = match self.services.store_read("settings") {
            StoreRead::Found(data) => data,
            _ => hlsr_core::defaults::defaults("settings"),
        };
        match data.get("config") {
            Some(Value::Object(map)) => map.clone(),
            _ => Map::new(),
        }
    }

    /// `settings.set("config", {...config, [key]: value})`; файл есть, но не прочитан — нет.
    fn write_config(&self, key: &str, value: Value) -> bool {
        let mut data = match self.services.store_read("settings") {
            StoreRead::Found(data) => data,
            StoreRead::Missing => hlsr_core::defaults::defaults("settings"),
            StoreRead::Invalid => return false,
        };
        let mut config = match data.get("config") {
            Some(Value::Object(map)) => map.clone(),
            _ => Map::new(),
        };
        config.insert(key.to_owned(), value);
        match &mut data {
            Value::Object(map) => {
                map.insert("config".into(), Value::Object(config));
            }
            other => *other = json!({ "config": config }),
        }
        self.services.store_write("settings", data)
    }

    // ---- «Сохранить» ----------------------------------------------------------------------------

    /// `submit`: путь → Proton → масштаб → лимит скорости → `applyPrefs`. Окно закрыли посреди —
    /// шаги идут дальше со значениями окна, а `applyPrefs` нет (`$emit` снятого окна — ничто).
    pub(super) fn submit(&mut self, cx: &mut Context<Self>) {
        let path = self.path_text(cx);
        self.state.draft.install_path = path.clone();
        let save = Save {
            generation: self.generation,
            state: self.state.clone(),
        };
        self.step(
            save,
            "validate-install-path",
            vec![json!(path)],
            cx,
            |this, save, reply, cx| {
                // Ответ не объект или канал упал: в 0.4.9 исключение, сохранения нет.
                let Ok(check) = reply
                    .map_err(drop)
                    .and_then(|c| if c.is_object() { Ok(c) } else { Err(()) })
                else {
                    return;
                };
                let live = this.live(&save);
                if !check.get("ok").is_some_and(truthy) {
                    if live {
                        let key = logic::path_error_key(check.get("error").and_then(Value::as_str));
                        this.state.path_error = Some(this.state.t(key));
                        if let Some(path) = &this.path {
                            path.update(cx, |input, cx| input.set_invalid(true, cx));
                        }
                        this.go_to("downloads");
                    }
                    return;
                }
                if live {
                    this.state.path_cyrillic = check.get("hasCyrillic").is_some_and(truthy);
                }
                let install_path =
                    hlsr_core::js::to_string(check.get("path").unwrap_or(&Value::Null));
                this.save_proton(save, install_path, cx);
            },
        );
    }

    fn save_proton(&mut self, save: Save, install_path: String, cx: &mut Context<Self>) {
        let state = &save.state;
        let choice = state
            .proton_choice
            .clone()
            .filter(|choice| Some(choice) != state.proton.selected.as_ref());
        let Some(choice) = choice else {
            self.save_scale(save, install_path, cx);
            return;
        };
        self.step(
            save,
            "proton-select",
            vec![json!(choice)],
            cx,
            |this, save, reply, cx| {
                if let Err(error) = reply {
                    if this.live(&save) {
                        this.state.proton_error = Some(error);
                        this.go_to("proton");
                    }
                    return;
                }
                this.save_scale(save, install_path, cx);
            },
        );
    }

    fn save_scale(&mut self, save: Save, install_path: String, cx: &mut Context<Self>) {
        let scale = save.state.ui_scale;
        let previous = logic::normalize_scale(self.config().get("uiScale").unwrap_or(&Value::Null));
        if previous == scale {
            self.save_speed(save, install_path, cx);
            return;
        }
        let value = hlsr_core::json_text::js_number(scale);
        // Итог записи 0.4.9 не смотрит (`uiScalePreference.write`), как и `setLanguage`.
        self.write_config("uiScale", value.clone());
        // `apply` ловит отказ и отвечает null: сохранение идёт дальше в любом случае.
        self.step(
            save,
            "ui-scale-apply",
            vec![value],
            cx,
            |this, save, _, cx| this.save_speed(save, install_path, cx),
        );
    }

    fn save_speed(&mut self, save: Save, install_path: String, cx: &mut Context<Self>) {
        let speed = save.state.draft.speed_limit();
        self.step(
            save,
            "download-settings-set",
            vec![json!(speed)],
            cx,
            |this, save, reply, cx| {
                let live = this.live(&save);
                if !matches!(&reply, Ok(result) if result.get("ok").is_some_and(truthy)) {
                    if live {
                        this.state.download_error = Some(this.state.t("#L_S_SPEED_SAVE_FAILED"));
                        this.go_to("downloads");
                    }
                    return;
                }
                if live {
                    this.apply(save.state, install_path, cx);
                }
            },
        );
    }

    /// `applyPrefs(draft, {language})` оболочки: prefs, трей, сплиты LiveSplit, папка
    /// библиотеки («папка занята»), язык последним (окно закрывается), тост.
    fn apply(&mut self, state: State, install_path: String, cx: &mut Context<Self>) {
        let prefs = state.draft.prefs;
        let language = state.language;
        if let Some(root) = ShellRoot::get(cx) {
            root.update(cx, |root, cx| root.apply_prefs(&prefs, cx));
        }
        let Some(flows) = Self::flows(cx) else {
            return;
        };
        flows.sync_tray_preference();
        let splits = logic::live_split_choices(&prefs);
        if let Value::Object(map) = &splits {
            flows.with_state(|s| s.live_split_splits = map.clone());
        }
        self.fire("tool-splits-set", vec![json!("livesplit"), splits], cx);
        let current = flows.with_state(|s| s.install_path.clone());
        if install_path == current {
            self.finish_apply(&language, cx);
            return;
        }
        // Дальше — оболочка: ответ доходит, даже если окно уже закрыли; язык — `extra.language`
        // этого «Сохранить», не окна, открытого заново.
        let reply = self
            .services
            .invoke("set-library-path", vec![json!(install_path)]);
        cx.spawn(async move |this, cx| {
            let result = reply.await.unwrap_or_else(|_| json!({ "ok": false }));
            this.update(cx, |this, cx| {
                if !result.get("ok").is_some_and(truthy) {
                    let busy = result.get("error").and_then(Value::as_str) == Some("busy");
                    let key = if busy {
                        "#L_S_PATH_BUSY"
                    } else {
                        "#L_S_PATH_INVALID"
                    };
                    Self::notify(cx, key, ERROR_ICON);
                    return;
                }
                let path = result
                    .get("path")
                    .filter(|path| truthy(path))
                    .map_or(install_path, hlsr_core::js::to_string);
                flows.with_state(|s| s.install_path = path);
                this.finish_apply(&language, cx);
            })
            .ok();
        })
        .detach();
    }

    fn finish_apply(&mut self, language: &str, cx: &mut Context<Self>) {
        let current = ShellContext::get(cx).map(|shell| shell.shell.language().code());
        if !language.is_empty() && Some(language) != current {
            self.close(cx);
            change_language(&self.services, language, cx);
        }
        Self::notify(cx, "#L_SETTINGS_SAVED", "fas fa-check");
    }

    // ---- «Загрузки» -----------------------------------------------------------------------------

    /// `browseInstallPath`: выбор папки, затем проверка пути на кириллицу.
    pub(super) fn browse(&mut self, cx: &mut Context<Self>) {
        let current = self.path_text(cx);
        self.ask(
            "select-install-folder",
            vec![json!(current)],
            cx,
            |this, reply, cx| {
                let Ok(selected) = reply else { return };
                if !truthy(&selected) {
                    return;
                }
                let path = hlsr_core::js::to_string(&selected);
                this.set_path_text(&path, cx);
                this.state.draft.install_path = path.clone();
                this.clear_path_error(cx);
                this.ask(
                    "validate-install-path",
                    vec![json!(path)],
                    cx,
                    |this, reply, _| {
                        this.state.path_cyrillic = matches!(&reply, Ok(check)
                            if check.get("ok").is_some_and(truthy)
                                && check.get("hasCyrillic").is_some_and(truthy));
                    },
                );
            },
        );
    }

    // ---- Discord --------------------------------------------------------------------------------

    fn load_discord_rpc(&mut self, cx: &mut Context<Self>) {
        self.ask("discord-rpc-get", Vec::new(), cx, |this, reply, _| {
            if let Some(discord) = reply.ok().as_ref().and_then(DiscordRpc::from_value) {
                this.state.discord = discord;
            }
        });
    }

    /// `setDiscordRpc(enabled)`: переключатель сразу, затем ответ main; отказ — перечитать.
    pub(super) fn set_discord_rpc(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.state.discord.enabled = enabled;
        self.ask(
            "discord-rpc-set",
            vec![json!(enabled)],
            cx,
            |this, reply, cx| match reply {
                Ok(value) => {
                    if let Some(discord) = DiscordRpc::from_value(&value) {
                        this.state.discord = discord;
                    }
                }
                Err(_) => this.load_discord_rpc(cx),
            },
        );
    }

    // ---- Proton (Linux) -------------------------------------------------------------------------

    pub(super) fn refresh_protons(&mut self, cx: &mut Context<Self>) {
        self.ask(
            "proton-status",
            Vec::new(),
            cx,
            |this, reply, _| match reply {
                Ok(value) => {
                    let status = ProtonStatus::from_value(&value);
                    this.state.proton_choice =
                        logic::choice_after_refresh(this.state.proton_choice.take(), &status);
                    this.state.proton = status;
                }
                Err(error) => this.state.proton_error = Some(error),
            },
        );
    }

    /// «Скачать» и «Обновить»: ошибка снята, затем список заново.
    pub(super) fn proton_action(&mut self, channel: &'static str, cx: &mut Context<Self>) {
        self.state.proton_error = None;
        self.ask(channel, Vec::new(), cx, |this, reply, cx| match reply {
            Ok(_) => this.refresh_protons(cx),
            Err(error) => this.state.proton_error = Some(error),
        });
    }

    pub(super) fn proton_cancel(&mut self, cx: &mut Context<Self>) {
        self.fire("proton-cancel", Vec::new(), cx);
    }

    pub(super) fn proton_choose(&mut self, path: &str, cx: &mut Context<Self>) {
        self.state.proton_choice = Some(path.to_owned());
        cx.notify();
    }

    /// `addCustomProton`: выбор папки, список заново, выбрана добавленная.
    pub(super) fn proton_add(&mut self, cx: &mut Context<Self>) {
        self.state.proton_error = None;
        self.ask(
            "proton-add",
            Vec::new(),
            cx,
            |this, reply, cx| match reply {
                Ok(added) if truthy(&added) => {
                    let path = added.get("path").and_then(Value::as_str).map(str::to_owned);
                    this.ask("proton-status", Vec::new(), cx, move |this, reply, _| {
                        match reply {
                            Ok(value) => this.state.proton = ProtonStatus::from_value(&value),
                            Err(error) => this.state.proton_error = Some(error),
                        }
                        this.state.proton_choice = path;
                    });
                }
                Ok(_) => {}
                Err(error) => this.state.proton_error = Some(error),
            },
        );
    }

    // ---- Диагностика ----------------------------------------------------------------------------

    /// `runDiagnostics`: список очищен, «Идёт проверка…»; итог — проверки ответа.
    pub(super) fn run_diagnostics(&mut self, cx: &mut Context<Self>) {
        if self.state.diag_running {
            return;
        }
        self.state.diag_running = true;
        self.state.diag_error = None;
        self.state.diag_finished = false;
        self.state.diag_checks.clear();
        self.state.details.clear();
        self.ask("diagnostics-run", Vec::new(), cx, |this, reply, _| {
            match reply {
                Ok(result) => {
                    if let Some(checks) = result.get("checks").filter(|c| truthy(c)) {
                        this.state.diag_checks = checks
                            .as_array()
                            .map(|list| list.iter().map(DiagCheck::from_value).collect())
                            .unwrap_or_default();
                    }
                    this.state.diag_finished = true;
                }
                Err(_) => this.state.diag_error = Some(this.state.t("#L_DG_RUN_FAILED")),
            }
            this.state.diag_running = false;
        });
        cx.notify();
    }

    pub(super) fn cancel_diagnostics(&mut self, cx: &mut Context<Self>) {
        self.fire("diagnostics-cancel", Vec::new(), cx);
    }

    pub(super) fn toggle_details(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.state.details.remove(id) {
            self.state.details.insert(id.to_owned());
        }
        cx.notify();
    }

    /// `openExportPreview`: сведения о дампе, панель, прокрутка к ней.
    pub(super) fn open_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.export_opening {
            return;
        }
        if self.state.export_open {
            self.reveal_export(window, cx);
            return;
        }
        self.state.export_opening = true;
        let reply = self.services.invoke("game-crash-info", vec![Value::Null]);
        let generation = self.generation;
        cx.spawn_in(window, async move |this, cx| {
            let info = reply.await;
            this.update_in(cx, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                this.state.export_opening = false;
                if let Ok(info) = info {
                    this.state.crash_dump = info.get("dumpPath").is_some_and(truthy);
                    this.state.export_include_dump = false;
                    this.state.export_open = true;
                    this.reveal_export(window, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `revealExportPanel`: панель — сразу под строкой кнопок (поле 28 + кнопка 48 + зазор
    /// 14), прокрутка к ней минус 8 px; без анимаций — сразу.
    fn reveal_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(scroll) = self.scrolls.get(self.state.section) else {
            return;
        };
        let top = crate::scale::zpx(crate::scale::zoom(window), 28. + 48. + 14. - 8.);
        let smooth = !AnimGate::still(cx).any();
        scroll.scroll_to(top, smooth, window, cx);
        cx.notify();
    }

    pub(super) fn close_export(&mut self, cx: &mut Context<Self>) {
        self.state.export_open = false;
        cx.notify();
    }

    pub(super) fn set_include_dump(&mut self, on: bool, cx: &mut Context<Self>) {
        self.state.export_include_dump = on;
        cx.notify();
    }

    /// `exportZip`: итог — тостом; удачно — панель закрыта.
    pub(super) fn export_zip(&mut self, cx: &mut Context<Self>) {
        if self.state.export_busy {
            return;
        }
        self.state.export_busy = true;
        let include = self.state.export_include_dump;
        self.ask(
            "export-diagnostics-zip",
            vec![json!({ "includeDump": include })],
            cx,
            |this, reply, cx| {
                if let Ok(result) = reply {
                    let ok = result.get("ok").is_some_and(truthy);
                    let key = if ok {
                        "#L_S_EXPORT_DIAGNOSTICS_OK"
                    } else if result.get("cancelled").is_some_and(truthy) {
                        "#L_DG_EXPORT_CANCELLED"
                    } else {
                        "#L_S_EXPORT_DIAGNOSTICS_FAIL"
                    };
                    // `$emit("toast", text)` — значок тоста по умолчанию.
                    if let Some(shell) = ShellContext::get(cx) {
                        shell
                            .shell
                            .notify(&shell.shell.tr(key, &[]), "fas fa-info-circle");
                    }
                    if ok {
                        this.state.export_open = false;
                    }
                }
                this.state.export_busy = false;
            },
        );
        cx.notify();
    }

    // ---- HLSR-CE: «Свои моды» ------------------------------------------------------------------

    fn load_mods(&mut self, cx: &mut Context<Self>) {
        self.ask("custom-mods-list", Vec::new(), cx, |this, reply, _| {
            if let Some(view) = reply.ok().as_ref().and_then(ModsView::from_value) {
                this.state.mods = view;
            }
        });
    }

    /// Действие с модами: ответ — новый снимок и заметка.
    pub(super) fn mods_do(
        &mut self,
        channel: &'static str,
        args: Vec<Value>,
        cx: &mut Context<Self>,
    ) {
        // Выбор папки или exe ждёт диалога; остальные кнопки на это время выключены.
        self.state.mods_busy = true;
        self.state.mods_note = None;
        cx.notify();
        self.ask(channel, args, cx, move |this, reply, _| {
            this.state.mods_busy = false;
            match reply {
                Ok(value) => {
                    if let Some(view) = ModsView::from_value(&value) {
                        this.state.mods = view;
                    }
                    this.state.mods_note = note_for(channel, &value);
                }
                Err(_) => {
                    this.state.mods_note = note_for(channel, &json!({"ok": false}));
                }
            }
        });
    }

    /// Выбор движка: пункт `__pick__` — свой exe через диалог.
    pub(super) fn mods_set_engine(&mut self, id: &str, exe: &str, cx: &mut Context<Self>) {
        if exe == sections::PICK_EXE {
            self.mods_do("custom-mods-pick-exe", vec![json!(id)], cx);
        } else {
            self.mods_do("custom-mods-set-engine", vec![json!(id), json!(exe)], cx);
        }
    }

    /// Поля параметров запуска: новое — для нового мода (с его параметрами), лишние — убрать.
    fn sync_mod_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids: BTreeSet<&str> = self.state.mods.mods.iter().map(|m| m.id.as_str()).collect();
        self.mod_args.retain(|id, _| ids.contains(id.as_str()));
        let placeholder = self.state.t("#L_MODS_ARGS_PLACEHOLDER");
        for entry in &self.state.mods.mods {
            if self.mod_args.contains_key(&entry.id) {
                continue;
            }
            let args = entry.args.clone();
            let placeholder = placeholder.clone();
            let input = cx.new(|cx| {
                TextInput::new(window, cx)
                    .style(sections::args_style)
                    .placeholder(placeholder)
                    .max_length(1024)
                    .with_text(args)
            });
            let id = entry.id.clone();
            let events = cx.subscribe(&input, move |this, input, event, cx| {
                if *event == FieldEvent::Change {
                    let text = input.read(cx).value();
                    this.mods_do("custom-mods-set-args", vec![json!(id), json!(text)], cx);
                }
            });
            self.mod_args.insert(entry.id.clone(), (input, events));
        }
    }

    pub(super) fn open_logs(&mut self, cx: &mut Context<Self>) {
        self.fire("open-logs-folder", Vec::new(), cx);
    }

    pub(super) fn open_link(&mut self, key: &str, cx: &mut Context<Self>) {
        self.fire("open-external-link", vec![json!(key)], cx);
    }
}

/// `setTheme(id, accent)` оболочки: тема из списка (незнакомая — по умолчанию), акцент — новый
/// или прежний; prefs и тема окна. Смена перетекает 0,42 с, без анимаций — сразу.
pub fn set_theme(id: &str, accent: Option<&str>, window: &mut Window, cx: &mut App) {
    let Some(root) = ShellRoot::get(cx) else {
        return;
    };
    let next = cx.global::<Theme>().table().theme_id(id).to_owned();
    let prefs = root.read(cx).prefs();
    let (saved_theme, saved_accent) = {
        let prefs = prefs.borrow();
        let text = |key| prefs.get(key).and_then(Value::as_str).map(str::to_owned);
        (text("launcherTheme"), text("paperAccent"))
    };
    let paper_accent = accent.map(str::to_owned).or(saved_accent.clone());
    if Some(&next) == saved_theme.as_ref() && paper_accent == saved_accent {
        return;
    }
    let mut changes = Map::new();
    changes.insert("launcherTheme".into(), json!(next));
    changes.insert(
        "paperAccent".into(),
        paper_accent.clone().map_or(Value::Null, Value::String),
    );
    root.update(cx, |root, cx| root.apply_prefs(&changes, cx));
    let animate = !AnimGate::still(cx).any();
    cx.global_mut::<Theme>()
        .set(&next, paper_accent.as_deref(), animate, Instant::now());
    window.refresh();
}

/// HLSR-CE: смена языка из Legacy-интерфейса ([`crate::legacy`]).
pub(crate) fn change_language_to(lang: Language, _window: &mut Window, cx: &mut App) {
    let Some(services) = ShellContext::get(cx).map(|shell| shell.services.clone()) else {
        return;
    };
    change_language(&services, lang.code(), cx);
}

/// `setLanguage(id)` оболочки: тот же или незнакомый — ничего; иначе `settings.config.language`,
/// интерфейс заново (`langRev`), `updater-set-language`.
fn change_language(services: &Services, code: &str, cx: &mut App) {
    let lang = match code {
        "en" => Language::En,
        "ru" => Language::Ru,
        _ => return,
    };
    let Some(shell) = ShellContext::get(cx).cloned() else {
        return;
    };
    if lang == shell.shell.language() {
        return;
    }
    let mut data = match services.store_read("settings") {
        StoreRead::Found(data) => data,
        StoreRead::Missing => hlsr_core::defaults::defaults("settings"),
        StoreRead::Invalid => Value::Null,
    };
    if let Value::Object(map) = &mut data {
        let mut config = match map.get("config") {
            Some(Value::Object(config)) => config.clone(),
            _ => Map::new(),
        };
        config.insert("language".into(), json!(lang.index()));
        map.insert("config".into(), Value::Object(config));
        services.store_write("settings", data);
    }
    shell.flows.language_changed(lang.code());
    shell.shell.on_language_change(lang);
    shell.flows.sync_updater_language();
}

impl Render for SettingsModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::live::rendered(cx.entity_id());
        let (open, _) = Self::wanted(cx);
        if open != self.modal.is_open() {
            cx.defer_in(window, Self::sync);
        }
        if !self.state.linux {
            self.sync_mod_inputs(window, cx);
        }
        self.frame(window, cx)
    }
}
