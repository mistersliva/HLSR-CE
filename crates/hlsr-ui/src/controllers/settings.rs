//! Настройки лаунчера (`SettingsModal.vue` 0.4.9) без окна: разделы по ОС, черновик (`draft`)
//! и сброс (тема и акцент в сброс не входят), подписи списков (масштаб, лимит скорости), карточки
//! тем с акцентами (`LauncherThemePicker.vue`), строка состояния Discord, тексты Proton, группы и
//! строки диагностики, текст ошибки пути. Строки «Режим отрисовки» нет (решение владельца 4):
//! `config.renderer` в `settings.json` окно не трогает. Окно — `hlsr-gpui::settings`.

use crate::tr;
use hlsr_core::language::Language;
use hlsr_core::window_size::UI_SCALES;
use serde::Deserialize;
use serde_json::{Map, Value, json};

/// `SPEED_LIMITS` (`shared/download-settings.js`).
pub const SPEED_LIMITS: [u32; 8] = [0, 1, 2, 5, 10, 20, 50, 100];
/// `uiScalePreference.DEFAULT_SCALE`.
pub const DEFAULT_SCALE: f64 = 1.0;

/// Раздел окна (`sections`): id, значок FontAwesome, ключ названия.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub id: &'static str,
    pub icon: &'static str,
    pub title: &'static str,
}

const GENERAL: Section = Section {
    id: "general",
    icon: "fas fa-sliders-h",
    title: "#L_S_GENERAL",
};
const DOWNLOADS: Section = Section {
    id: "downloads",
    icon: "fas fa-download",
    title: "#L_S_DOWNLOADS",
};
const PROTON: Section = Section {
    id: "proton",
    icon: "fas fa-flask",
    title: "#L_S_PROTON",
};
/// HLSR-CE: свои Source-моды (Windows).
const MODS: Section = Section {
    id: "mods",
    icon: "fas fa-puzzle-piece",
    title: "#L_S_MODS",
};
const INTERFACE: Section = Section {
    id: "interface",
    icon: "fas fa-palette",
    title: "#L_S_INTERFACE",
};
const DIAGNOSTICS: Section = Section {
    id: "diagnostics",
    icon: "fas fa-stethoscope",
    title: "#L_S_DIAGNOSTICS",
};
const ABOUT: Section = Section {
    id: "about",
    icon: "fas fa-info-circle",
    title: "#L_S_ABOUT",
};

/// Разделы по порядку; «Proton» — только Linux (`IS_LINUX`).
pub fn sections(linux: bool) -> Vec<Section> {
    let mut list = vec![GENERAL, DOWNLOADS];
    if linux {
        list.push(PROTON);
    } else {
        // HLSR-CE: моды запускаются как exe Windows.
        list.push(MODS);
    }
    list.extend([INTERFACE, DIAGNOSTICS, ABOUT]);
    list
}

/// Раздел при открытии: `initialSection === "data" ? "general" : initialSection`; неизвестный
/// — «О программе» (ветка `v-else` разметки).
pub fn initial_section(linux: bool, requested: &str) -> usize {
    let requested = if requested == "data" {
        "general"
    } else {
        requested
    };
    let list = sections(linux);
    list.iter()
        .position(|section| section.id == requested)
        .unwrap_or(list.len() - 1)
}

/// `tab-ink.js direction(ids, from, to)`: «next», если новая вкладка правее или та же.
pub fn slide_direction(from: usize, to: usize) -> &'static str {
    if to >= from { "next" } else { "prev" }
}

/// Пункт списка: id и подпись.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub label: String,
}

/// `languages`: русский и английский (подписи — на своём языке).
pub fn language_items() -> Vec<Item> {
    vec![
        Item {
            id: "ru".into(),
            label: "Русский".into(),
        },
        Item {
            id: "en".into(),
            label: "English".into(),
        },
    ]
}

/// `languageLabel`: подпись пункта, иначе сам код.
pub fn language_label(code: &str) -> String {
    language_items()
        .into_iter()
        .find(|item| item.id == code)
        .map_or_else(|| code.to_owned(), |item| item.label)
}

/// Id масштаба в списке: `0.75`, `1`, `1.25`, `1.5` (как `String(scale)` у `item.id`).
pub fn scale_id(scale: f64) -> String {
    hlsr_core::js::number::number_to_string(scale)
}

/// `uiScaleItems`: «100 %», у умолчания — « (по умолчанию)».
pub fn ui_scale_items(lang: Language) -> Vec<Item> {
    UI_SCALES
        .iter()
        .map(|&scale| Item {
            id: scale_id(scale),
            label: if scale == DEFAULT_SCALE {
                format!(
                    "{} ({})",
                    ui_scale_label(scale),
                    tr(lang, "#L_S_UI_SCALE_DEFAULT", &[])
                )
            } else {
                ui_scale_label(scale)
            },
        })
        .collect()
}

/// `uiScaleLabel`: `${Math.round(scale * 100)} %`.
pub fn ui_scale_label(scale: f64) -> String {
    format!("{} %", hlsr_core::js::math_round(scale * 100.0))
}

/// `uiScalePreference.normalize`: не из списка → 1.
pub fn normalize_scale(value: &Value) -> f64 {
    hlsr_core::window_size::normalize_ui_scale(value)
}

/// `speedLabel(n)`: 0 — «Без лимита», иначе «%0% МБ/с».
pub fn speed_label(lang: Language, limit: u32) -> String {
    if limit == 0 {
        tr(lang, "#L_S_NO_LIMIT", &[])
    } else {
        tr(lang, "#L_S_MBPS", &[&limit.to_string()])
    }
}

/// `speedItems`.
pub fn speed_items(lang: Language) -> Vec<Item> {
    SPEED_LIMITS
        .iter()
        .map(|&limit| Item {
            id: limit.to_string(),
            label: speed_label(lang, limit),
        })
        .collect()
}

/// `normalizeSpeedLimit`: только число из списка, иначе 0.
pub fn normalize_speed_limit(value: &Value) -> u32 {
    match value.as_f64() {
        Some(limit) if SPEED_LIMITS.iter().any(|&l| f64::from(l) == limit) => limit as u32,
        _ => 0,
    }
}

/// Подпись выбранного пункта (`label`; незнакомый id — как есть).
pub fn item_label(items: &[Item], id: &str) -> String {
    items
        .iter()
        .find(|item| item.id == id)
        .map_or_else(|| id.to_owned(), |item| item.label.clone())
}

// ---- Темы -------------------------------------------------------------------------------------

/// Вариант акцента темы (`PAPER_ACCENTS`): id, ключ названия, цвет.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Accent {
    pub id: String,
    pub title: String,
    pub color: String,
}

/// Карточка темы `LauncherThemePicker` (`data/theme-swatches.json`, THEMES 0.4.9).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ThemeCard {
    pub id: String,
    pub title: String,
    pub hint: String,
    /// Фон окна, поверхность, акцент.
    pub swatch: [String; 3],
    /// Варианты акцента («Бумага»); первый — по умолчанию.
    #[serde(default)]
    pub accents: Vec<Accent>,
}

impl ThemeCard {
    /// `accentId`: выбранный вариант, незнакомый — первый; у темы без вариантов — `None`.
    pub fn accent<'a>(&'a self, current: &str) -> Option<&'a Accent> {
        self.accents
            .iter()
            .find(|accent| accent.id == current)
            .or(self.accents.first())
    }

    /// `swatchAccent(theme)`: выбранная тема с вариантами показывает выбранный акцент.
    pub fn swatch_accent<'a>(&'a self, current_theme: &str, current_accent: &str) -> &'a str {
        match self.accent(current_accent) {
            Some(accent) if self.id == current_theme => &accent.color,
            _ => &self.swatch[2],
        }
    }
}

/// Темы в порядке выбора (все 9 тем 0.4.9; какие показывать — решает вид по таблице тем).
pub fn theme_cards() -> Vec<ThemeCard> {
    #[derive(Deserialize)]
    struct Table {
        themes: Vec<ThemeCard>,
    }
    let table: Table = serde_json::from_str(include_str!("../../data/theme-swatches.json"))
        .expect("data/theme-swatches.json");
    table.themes
}

// ---- Discord ----------------------------------------------------------------------------------

/// `discordRpcStatusText`: ключ строки по `{enabled, state}`.
pub fn discord_status_key(enabled: bool, state: &str) -> &'static str {
    if !enabled {
        return "#L_S_DISCORD_RPC_OFF";
    }
    match state {
        "connected" => "#L_S_DISCORD_RPC_CONNECTED",
        "connecting" => "#L_S_DISCORD_RPC_CONNECTING",
        "unavailable" => "#L_S_DISCORD_RPC_UNAVAILABLE",
        "retry_wait" | "disconnected" => "#L_S_DISCORD_RPC_ERROR",
        _ => "#L_S_DISCORD_RPC_CONNECTING",
    }
}

/// Состояние Discord RPC (`discord-rpc-get/set`, `discord-rpc-state`).
#[derive(Debug, Clone, PartialEq)]
pub struct DiscordRpc {
    pub enabled: bool,
    pub state: String,
}

impl Default for DiscordRpc {
    fn default() -> DiscordRpc {
        DiscordRpc {
            enabled: false,
            state: "disabled".into(),
        }
    }
}

impl DiscordRpc {
    /// Ответ канала: только объект (`state && typeof state === "object"`), иначе `None`.
    pub fn from_value(value: &Value) -> Option<DiscordRpc> {
        let object = value.as_object()?;
        Some(DiscordRpc {
            enabled: object.get("enabled") == Some(&Value::Bool(true)),
            state: object
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        })
    }
}

// ---- Proton -----------------------------------------------------------------------------------

/// Установленная сборка (`proton-status.installed[]`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProtonBuild {
    pub path: String,
    pub name: String,
    pub recommended: bool,
}

/// `proton-status.update`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProtonUpdate {
    pub available: bool,
    pub latest: Option<String>,
    pub updated_from: Option<String>,
    pub updated_to: Option<String>,
    pub error: bool,
    pub checked: bool,
}

/// `proton-status` / `proton-state`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProtonStatus {
    pub installed: Vec<ProtonBuild>,
    pub selected: Option<String>,
    pub phase: String,
    pub percent: f64,
    pub error: Option<String>,
    pub update: ProtonUpdate,
}

impl ProtonStatus {
    /// Состояние «ничего не знаем» (`data()`): `phase: "idle"`.
    pub fn idle() -> ProtonStatus {
        ProtonStatus {
            phase: "idle".into(),
            ..ProtonStatus::default()
        }
    }

    pub fn from_value(value: &Value) -> ProtonStatus {
        let text = |object: &Value, key: &str| {
            object
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .filter(|s| !s.is_empty())
        };
        let update = value.get("update").cloned().unwrap_or(Value::Null);
        ProtonStatus {
            installed: value
                .get("installed")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .map(|item| ProtonBuild {
                            path: text(item, "path").unwrap_or_default(),
                            name: text(item, "name").unwrap_or_default(),
                            recommended: item.get("recommended") == Some(&Value::Bool(true)),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            selected: text(value, "selected"),
            phase: text(value, "phase").unwrap_or_else(|| "idle".into()),
            percent: value.get("percent").and_then(Value::as_f64).unwrap_or(0.0),
            error: text(value, "error"),
            update: ProtonUpdate {
                available: update.get("available") == Some(&Value::Bool(true)),
                latest: text(&update, "latest"),
                updated_from: text(&update, "updatedFrom"),
                updated_to: text(&update, "updatedTo"),
                error: update.get("error").is_some_and(hlsr_core::js::truthy),
                checked: update.get("checked").is_some_and(hlsr_core::js::truthy),
            },
        }
    }

    /// `protonBusy`: checking, downloading или extracting.
    pub fn busy(&self) -> bool {
        matches!(
            self.phase.as_str(),
            "checking" | "downloading" | "extracting"
        )
    }

    /// `protonPhaseText`.
    pub fn phase_key(&self) -> &'static str {
        match self.phase.as_str() {
            "extracting" => "#L_S_PROTON_EXTRACTING",
            "checking" => "#L_S_PROTON_CHECKING",
            _ => "#L_S_PROTON_DOWNLOADING",
        }
    }

    /// `protonUpdateTone`: fresh | warn | ok.
    pub fn update_tone(&self) -> &'static str {
        let update = &self.update;
        if update.available || update.updated_to.is_some() {
            "fresh"
        } else if update.error {
            "warn"
        } else {
            "ok"
        }
    }

    /// `protonUpdateText`: пусто — ничего не показывать.
    pub fn update_text(&self, lang: Language) -> String {
        let update = &self.update;
        let latest = short_tag(update.latest.as_deref());
        if update.available {
            return tr(lang, "#L_S_PROTON_UPDATE_AVAILABLE", &[&latest]);
        }
        if let Some(to) = &update.updated_to {
            return tr(
                lang,
                "#L_S_PROTON_UPDATED",
                &[
                    &short_tag(update.updated_from.as_deref()),
                    &short_tag(Some(to)),
                ],
            );
        }
        if update.error {
            return tr(lang, "#L_S_PROTON_UPDATE_FAILED", &[]);
        }
        if update.checked && update.latest.is_some() {
            return tr(lang, "#L_S_PROTON_UP_TO_DATE", &[&latest]);
        }
        String::new()
    }

    /// `isFreshProton(item)`: сборка только что обновлена (`proton-<updatedTo>-x86_64`).
    pub fn is_fresh(&self, name: &str) -> bool {
        self.update
            .updated_to
            .as_deref()
            .is_some_and(|to| name == format!("proton-{to}-x86_64"))
    }

    pub fn has(&self, path: &str) -> bool {
        self.installed.iter().any(|item| item.path == path)
    }
}

/// `short(tag)`: без префикса `wineland-`; нет тега — пусто.
pub fn short_tag(tag: Option<&str>) -> String {
    let tag = tag.unwrap_or_default();
    tag.strip_prefix("wineland-").unwrap_or(tag).to_owned()
}

/// `refreshProtons`: выбор остаётся, если он есть среди установленных, иначе — выбранная main.
pub fn choice_after_refresh(choice: Option<String>, status: &ProtonStatus) -> Option<String> {
    match choice {
        Some(path) if status.has(&path) => Some(path),
        _ => status.selected.clone(),
    }
}

/// `onProtonState`: выбор следует за main, если его не было, если он совпадал с прежним выбором
/// main или его больше нет среди установленных.
pub fn choice_after_state(
    choice: Option<String>,
    previous_selected: Option<&str>,
    status: &ProtonStatus,
) -> Option<String> {
    match choice {
        Some(path) if Some(path.as_str()) != previous_selected && status.has(&path) => Some(path),
        _ => status.selected.clone(),
    }
}

// ---- Диагностика ------------------------------------------------------------------------------

/// Проверка (`diagnostics-run.checks[]`, `diagnostics-progress`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DiagCheck {
    pub id: String,
    pub group: String,
    pub status: String,
    pub detail: Option<String>,
    pub detail_key: Option<String>,
    pub detail_args: Vec<String>,
    pub code: Option<String>,
}

impl DiagCheck {
    pub fn from_value(value: &Value) -> DiagCheck {
        let text = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .filter(|s| !s.is_empty())
        };
        DiagCheck {
            id: text("id").unwrap_or_default(),
            group: text("group").unwrap_or_default(),
            status: text("status").unwrap_or_default(),
            detail: text("detail"),
            detail_key: text("detailKey"),
            detail_args: value
                .get("detailArgs")
                .and_then(Value::as_array)
                .map(|args| args.iter().map(hlsr_core::js::to_string).collect())
                .unwrap_or_default(),
            code: text("code"),
        }
    }
}

/// `onDiagProgress`: проверка с тем же id заменяется, новая встаёт в конец.
pub fn upsert_check(checks: &mut Vec<DiagCheck>, check: DiagCheck) {
    match checks.iter().position(|c| c.id == check.id) {
        Some(index) => checks[index] = check,
        None => checks.push(check),
    }
}

/// Группы по порядку (`diagGroups`): id и ключ заголовка; «Linux и Proton» или «Windows» — по ОС.
pub fn diag_group_order(linux: bool) -> Vec<(&'static str, &'static str)> {
    vec![
        ("hlsr", "#L_DG_G_HLSR"),
        ("graphics", "#L_DG_G_GRAPHICS"),
        ("downloads", "#L_DG_G_DOWNLOADS"),
        if linux {
            ("linux", "#L_DG_G_LINUX")
        } else {
            ("windows", "#L_DG_G_WINDOWS")
        },
        ("library", "#L_DG_G_LIBRARY"),
        ("build", "#L_DG_G_BUILD"),
        ("profile", "#L_DG_G_PROFILE"),
        ("steam", "#L_DG_G_STEAM"),
        ("network", "#L_DG_G_NETWORK"),
    ]
}

/// Непустые группы с их проверками.
pub fn diag_groups(
    checks: &[DiagCheck],
    linux: bool,
) -> Vec<(&'static str, &'static str, Vec<&DiagCheck>)> {
    diag_group_order(linux)
        .into_iter()
        .filter_map(|(id, title)| {
            let items: Vec<&DiagCheck> = checks.iter().filter(|c| c.group == id).collect();
            (!items.is_empty()).then_some((id, title, items))
        })
        .collect()
}

/// `statusIcon(status)`.
pub fn status_icon(status: &str) -> &'static str {
    match status {
        "ok" => "fas fa-check-circle",
        "warn" => "fas fa-exclamation-triangle",
        "fail" => "fas fa-times-circle",
        _ => "fas fa-minus-circle",
    }
}

/// `statusText(status)`.
pub fn status_key(status: &str) -> &'static str {
    match status {
        "ok" => "#L_DG_OK",
        "warn" => "#L_DG_WARN",
        "fail" => "#L_DG_FAIL",
        _ => "#L_DG_SKIP",
    }
}

/// `diagName(id)`: `#L_DG_<ID>` (дефисы → подчёркивания); нет строки — `steam-owns-*` →
/// «Лицензия Steam», иначе id с пробелами вместо дефисов.
pub fn diag_name(lang: Language, id: &str) -> String {
    let key = format!("#L_DG_{}", id.to_uppercase().replace('-', "_"));
    let text = tr(lang, &key, &[]);
    if text != key {
        return text;
    }
    if id.starts_with("steam-owns-") {
        return tr(lang, "#L_DG_STEAM_LICENSE", &[]);
    }
    id.replace('-', " ")
}

/// `diagDetail(item)`: `detailKey` с аргументами, иначе `detail`, иначе текст состояния.
pub fn diag_detail(lang: Language, check: &DiagCheck) -> String {
    if let Some(key) = &check.detail_key {
        let args: Vec<&str> = check.detail_args.iter().map(String::as_str).collect();
        return tr(lang, key, &args);
    }
    match &check.detail {
        Some(detail) => detail.clone(),
        None => tr(lang, status_key(&check.status), &[]),
    }
}

/// `diagHint(code)`: `#L_DGH_<code>`, нет строки — пусто.
pub fn diag_hint(lang: Language, code: &str) -> String {
    let key = format!("#L_DGH_{code}");
    let text = tr(lang, &key, &[]);
    if text == key { String::new() } else { text }
}

// ---- Сохранение -------------------------------------------------------------------------------

/// `pathErrorText(code)`.
pub fn path_error_key(code: Option<&str>) -> &'static str {
    match code {
        Some("no-access") => "#L_S_PATH_NO_ACCESS",
        _ => "#L_S_PATH_INVALID",
    }
}

/// Черновик окна: `draft` (prefs плюс `installPath`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Draft {
    pub prefs: Map<String, Value>,
    pub install_path: String,
}

impl Draft {
    /// `Object.assign({}, this.prefs, {installPath})`.
    pub fn new(prefs: &Map<String, Value>, install_path: &str) -> Draft {
        Draft {
            prefs: prefs.clone(),
            install_path: install_path.to_owned(),
        }
    }

    /// `resetDraft`: умолчания `launcher-prefs.js` плюс нынешние `toolSplits`, `liveSplitGame`,
    /// тема и акцент (применяются сразу и в сброс не входят), путь — по умолчанию.
    pub fn reset(prefs: &Map<String, Value>, default_install_path: &str) -> Draft {
        let Value::Object(mut defaults) = prefs_store_defaults() else {
            unreachable!("умолчания — объект")
        };
        let splits = match prefs.get("toolSplits") {
            Some(Value::Object(map)) => Value::Object(map.clone()),
            _ => json!({}),
        };
        defaults.insert("toolSplits".into(), splits);
        defaults.insert(
            "liveSplitGame".into(),
            prefs.get("liveSplitGame").cloned().unwrap_or(Value::Null),
        );
        for key in ["launcherTheme", "paperAccent"] {
            match prefs.get(key) {
                Some(value) => defaults.insert(key.into(), value.clone()),
                None => defaults.shift_remove(key),
            };
        }
        Draft {
            prefs: defaults,
            install_path: default_install_path.to_owned(),
        }
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        match self.prefs.get(key) {
            Some(Value::Bool(value)) => *value,
            Some(other) => hlsr_core::js::truthy(other),
            None => default,
        }
    }

    pub fn set(&mut self, key: &str, value: Value) {
        self.prefs.insert(key.to_owned(), value);
    }

    /// `speedLimit` черновика (число из списка).
    pub fn speed_limit(&self) -> u32 {
        normalize_speed_limit(self.prefs.get("speedLimit").unwrap_or(&Value::Null))
    }
}

/// `prefsStore.defaults()`: `DEFAULTS` из `launcher-prefs.js` (без ключей, добавленных в
/// `launcher-prefs.json` переносом: `keyboardRunMode`, `wasLaunched`).
pub fn prefs_store_defaults() -> Value {
    let Value::Object(mut defaults) = hlsr_core::launcher_prefs::prefs_defaults() else {
        unreachable!("умолчания — объект")
    };
    defaults.remove("keyboardRunMode");
    defaults.remove("wasLaunched");
    Value::Object(defaults)
}

/// `liveSplitChoicesOf(prefs)`: `{gameId: набор}`; старый вид `{livesplit: набор}` относился к
/// Ghosting.
pub fn live_split_choices(prefs: &Map<String, Value>) -> Value {
    let mut saved = match prefs.get("toolSplits") {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    if let Some(Value::String(set)) = saved.get("livesplit").cloned() {
        if !saved
            .get("ghosting-renewed")
            .is_some_and(hlsr_core::js::truthy)
        {
            saved.insert("ghosting-renewed".into(), Value::String(set));
        }
        saved.shift_remove("livesplit");
    }
    Value::Object(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_follow_the_platform() {
        let win: Vec<&str> = sections(false).iter().map(|s| s.id).collect();
        assert_eq!(
            win,
            [
                "general",
                "downloads",
                "mods",
                "interface",
                "diagnostics",
                "about"
            ]
        );
        let linux: Vec<&str> = sections(true).iter().map(|s| s.id).collect();
        assert_eq!(
            linux,
            [
                "general",
                "downloads",
                "proton",
                "interface",
                "diagnostics",
                "about"
            ]
        );
        assert_eq!(initial_section(false, "data"), 0);
        assert_eq!(initial_section(false, "diagnostics"), 4);
        assert_eq!(initial_section(false, "mods"), 2);
        assert_eq!(initial_section(true, "diagnostics"), 4);
        assert_eq!(initial_section(true, "proton"), 2);
        assert_eq!(initial_section(false, "proton"), 5);
        assert_eq!(slide_direction(0, 2), "next");
        assert_eq!(slide_direction(2, 2), "next");
        assert_eq!(slide_direction(3, 1), "prev");
    }

    #[test]
    fn list_items_match_the_vue_computeds() {
        let scales: Vec<(String, String)> = ui_scale_items(Language::Ru)
            .into_iter()
            .map(|i| (i.id, i.label))
            .collect();
        assert_eq!(
            scales,
            [
                ("0.75".to_owned(), "75 %".to_owned()),
                ("1".to_owned(), "100 % (по умолчанию)".to_owned()),
                ("1.25".to_owned(), "125 %".to_owned()),
                ("1.5".to_owned(), "150 %".to_owned()),
            ]
        );
        assert_eq!(ui_scale_label(1.25), "125 %");
        assert_eq!(normalize_scale(&json!("1.25")), 1.25);
        assert_eq!(normalize_scale(&json!(2)), 1.0);
        let speeds: Vec<(String, String)> = speed_items(Language::En)
            .into_iter()
            .map(|i| (i.id, i.label))
            .collect();
        assert_eq!(speeds[0], ("0".to_owned(), "No limit".to_owned()));
        assert_eq!(speeds[3], ("5".to_owned(), "5 MB/s".to_owned()));
        assert_eq!(speed_label(Language::Ru, 20), "20 МБ/с");
        assert_eq!(normalize_speed_limit(&json!(7)), 0);
        assert_eq!(normalize_speed_limit(&json!(50)), 50);
        assert_eq!(normalize_speed_limit(&json!("5")), 0);
        assert_eq!(language_label("ru"), "Русский");
        assert_eq!(language_label("de"), "de");
    }

    #[test]
    fn theme_cards_carry_paper_accents() {
        let cards = theme_cards();
        let ids: Vec<&str> = cards.iter().map(|card| card.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "hlsr",
                "lambda",
                "resonance",
                "combine",
                "xen",
                "steam2003",
                "midnight",
                "liquid-glass",
                "paper"
            ]
        );
        // Поверхность Liquid Glass — полупрозрачный белый, как в launcher-themes.js.
        assert_eq!(cards[7].swatch[1], "rgba(255, 255, 255, 0.18)");
        let paper = &cards[8];
        assert_eq!(paper.accent("rose").map(|a| a.id.as_str()), Some("rose"));
        assert_eq!(paper.accent("nope").map(|a| a.id.as_str()), Some("clay"));
        assert_eq!(paper.swatch_accent("paper", "sea"), "#80B2BA");
        // Не выбранная тема — свой цвет образца.
        assert_eq!(paper.swatch_accent("hlsr", "sea"), "#D5876D");
        assert!(cards[0].accent("clay").is_none());
        assert_eq!(cards[0].swatch_accent("hlsr", "clay"), "#00ABFF");
    }

    #[test]
    fn discord_status_text_follows_enabled_and_state() {
        assert_eq!(
            discord_status_key(false, "connected"),
            "#L_S_DISCORD_RPC_OFF"
        );
        assert_eq!(
            discord_status_key(true, "connected"),
            "#L_S_DISCORD_RPC_CONNECTED"
        );
        assert_eq!(
            discord_status_key(true, "unavailable"),
            "#L_S_DISCORD_RPC_UNAVAILABLE"
        );
        assert_eq!(
            discord_status_key(true, "retry_wait"),
            "#L_S_DISCORD_RPC_ERROR"
        );
        assert_eq!(
            discord_status_key(true, "disconnected"),
            "#L_S_DISCORD_RPC_ERROR"
        );
        assert_eq!(
            discord_status_key(true, "weird"),
            "#L_S_DISCORD_RPC_CONNECTING"
        );
        assert_eq!(
            DiscordRpc::from_value(&json!({"enabled": true, "state": "connected", "code": null})),
            Some(DiscordRpc {
                enabled: true,
                state: "connected".into()
            })
        );
        assert_eq!(DiscordRpc::from_value(&json!(null)), None);
        assert_eq!(DiscordRpc::from_value(&json!("x")), None);
    }

    fn status(update: Value) -> ProtonStatus {
        ProtonStatus::from_value(&json!({
            "installed": [
                {"path": "/a/GE-Proton10-25", "name": "GE-Proton10-25", "recommended": true},
                {"path": "/b/proton-10.2-x86_64", "name": "proton-10.2-x86_64", "recommended": false}
            ],
            "selected": "/a/GE-Proton10-25",
            "phase": "downloading",
            "percent": 45,
            "error": null,
            "update": update
        }))
    }

    #[test]
    fn proton_texts_and_tones() {
        let s = status(json!({"available": true, "latest": "wineland-10.3"}));
        assert!(s.busy());
        assert_eq!(s.phase_key(), "#L_S_PROTON_DOWNLOADING");
        assert_eq!(s.update_tone(), "fresh");
        assert_eq!(
            s.update_text(Language::En),
            tr(Language::En, "#L_S_PROTON_UPDATE_AVAILABLE", &["10.3"])
        );
        let s = status(json!({"updatedFrom": "wineland-10.1", "updatedTo": "10.2"}));
        assert_eq!(s.update_tone(), "fresh");
        assert_eq!(
            s.update_text(Language::Ru),
            "Proton Wineland обновлён: 10.1 → 10.2"
        );
        assert!(s.is_fresh("proton-10.2-x86_64"));
        assert!(!s.is_fresh("GE-Proton10-25"));
        let s = status(json!({"error": "ETIMEDOUT"}));
        assert_eq!(s.update_tone(), "warn");
        assert_eq!(
            s.update_text(Language::En),
            tr(Language::En, "#L_S_PROTON_UPDATE_FAILED", &[])
        );
        let s = status(json!({"checked": true, "latest": "wineland-10.2"}));
        assert_eq!(s.update_tone(), "ok");
        assert_eq!(
            s.update_text(Language::En),
            "The latest Proton Wineland build is installed (10.2)"
        );
        let s = status(json!({"checked": false}));
        assert_eq!(s.update_text(Language::En), "");
        let idle =
            ProtonStatus::from_value(&json!({"installed": [], "selected": null, "phase": "idle"}));
        assert!(!idle.busy());
        assert_eq!(idle.phase_key(), "#L_S_PROTON_DOWNLOADING");
        assert_eq!(
            ProtonStatus::from_value(&json!({"phase": "checking"})).phase_key(),
            "#L_S_PROTON_CHECKING"
        );
        assert_eq!(
            ProtonStatus::from_value(&json!({"phase": "extracting"})).phase_key(),
            "#L_S_PROTON_EXTRACTING"
        );
    }

    #[test]
    fn proton_choice_follows_main_rules() {
        let s = status(json!({}));
        assert_eq!(
            choice_after_refresh(None, &s).as_deref(),
            Some("/a/GE-Proton10-25")
        );
        assert_eq!(
            choice_after_refresh(Some("/b/proton-10.2-x86_64".into()), &s).as_deref(),
            Some("/b/proton-10.2-x86_64")
        );
        assert_eq!(
            choice_after_refresh(Some("/gone".into()), &s).as_deref(),
            Some("/a/GE-Proton10-25")
        );
        // Выбор совпадал с прежним выбором main — идёт за новым.
        assert_eq!(
            choice_after_state(Some("/old".into()), Some("/old"), &s).as_deref(),
            Some("/a/GE-Proton10-25")
        );
        assert_eq!(
            choice_after_state(Some("/b/proton-10.2-x86_64".into()), Some("/old"), &s).as_deref(),
            Some("/b/proton-10.2-x86_64")
        );
        assert_eq!(
            choice_after_state(None, None, &s).as_deref(),
            Some("/a/GE-Proton10-25")
        );
    }

    fn check(id: &str, group: &str, status: &str) -> DiagCheck {
        DiagCheck {
            id: id.into(),
            group: group.into(),
            status: status.into(),
            ..DiagCheck::default()
        }
    }

    #[test]
    fn diagnostics_groups_names_details_and_hints() {
        let checks = vec![
            check("net-dns", "network", "ok"),
            check("hlsr-version", "hlsr", "ok"),
            check("win-clock", "windows", "warn"),
            check("steam-owns-220", "steam", "fail"),
            check("steam-owns-320", "steam", "skip"),
        ];
        let groups: Vec<(&str, usize)> = diag_groups(&checks, false)
            .iter()
            .map(|(id, _, items)| (*id, items.len()))
            .collect();
        assert_eq!(
            groups,
            [("hlsr", 1), ("windows", 1), ("steam", 2), ("network", 1)]
        );
        assert!(
            diag_groups(&checks, true)
                .iter()
                .all(|(id, _, _)| *id != "windows")
        );
        assert_eq!(diag_name(Language::En, "net-dns"), "DNS github.com");
        assert_eq!(
            diag_name(Language::Ru, "steam-owns-220"),
            "Лицензия Half-Life 2"
        );
        assert_eq!(diag_name(Language::Ru, "steam-owns-320"), "Лицензия Steam");
        assert_eq!(diag_name(Language::En, "some-new-check"), "some new check");
        assert_eq!(status_icon("warn"), "fas fa-exclamation-triangle");
        assert_eq!(status_icon("skip"), "fas fa-minus-circle");
        let mut c = check("x", "hlsr", "skip");
        assert_eq!(diag_detail(Language::En, &c), "Skipped");
        c.detail = Some("1.2.3".into());
        assert_eq!(diag_detail(Language::En, &c), "1.2.3");
        c.detail_key = Some("#L_DGV_PLAYTIME".into());
        c.detail_args = vec!["1".into(), "2".into(), "3".into(), "4".into(), "5".into()];
        assert_eq!(
            diag_detail(Language::En, &c),
            "1 h 2 m 3 s · 4 launches, 5 games"
        );
        assert_eq!(diag_hint(Language::En, "NOPE"), "");
        assert_eq!(
            diag_hint(Language::Ru, "GPU_RESTART_REQUIRED"),
            tr(Language::Ru, "#L_DGH_GPU_RESTART_REQUIRED", &[])
        );
        let parsed = DiagCheck::from_value(&json!({
            "id": "profile-playtime", "group": "profile", "status": "ok",
            "detailKey": "#L_DGV_PLAYTIME", "detailArgs": [36, 5, 0, 12, 3], "code": null
        }));
        assert_eq!(parsed.detail_args, ["36", "5", "0", "12", "3"]);
        assert_eq!(parsed.code, None);
        let mut list = vec![check("a", "hlsr", "ok")];
        upsert_check(&mut list, check("b", "hlsr", "ok"));
        upsert_check(&mut list, check("a", "hlsr", "fail"));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].status, "fail");
        assert_eq!(list[1].id, "b");
    }

    #[test]
    fn draft_reset_keeps_splits_and_game() {
        let mut prefs = Map::new();
        prefs.insert("speedLimit".into(), json!(20));
        prefs.insert("animations".into(), json!(false));
        prefs.insert("toolSplits".into(), json!({"ghosting-renewed": "maps"}));
        prefs.insert("liveSplitGame".into(), json!("218"));
        prefs.insert("keyboardRunMode".into(), json!("scripted"));
        prefs.insert("launcherTheme".into(), json!("paper"));
        prefs.insert("paperAccent".into(), json!("sea"));
        let draft = Draft::new(&prefs, "D:\\Games");
        assert_eq!(draft.install_path, "D:\\Games");
        assert_eq!(draft.speed_limit(), 20);
        assert!(!draft.get_bool("animations", true));
        let reset = Draft::reset(&prefs, "C:\\HLSR");
        assert_eq!(reset.install_path, "C:\\HLSR");
        assert_eq!(reset.prefs["speedLimit"], json!(0));
        assert_eq!(reset.prefs["animations"], json!(true));
        assert_eq!(reset.prefs["hideWhilePlaying"], json!(true));
        assert_eq!(
            reset.prefs["toolSplits"],
            json!({"ghosting-renewed": "maps"})
        );
        assert_eq!(reset.prefs["liveSplitGame"], json!("218"));
        assert_eq!(reset.prefs["launcherTheme"], json!("paper"));
        assert_eq!(reset.prefs["paperAccent"], json!("sea"));
        assert_eq!(reset.prefs["fullscreenButton"], json!(true));
        assert!(!reset.prefs.contains_key("keyboardRunMode"));
        assert!(!reset.prefs.contains_key("wasLaunched"));
        assert_eq!(
            live_split_choices(&prefs),
            json!({"ghosting-renewed": "maps"})
        );
        let mut old = Map::new();
        old.insert("toolSplits".into(), json!({"livesplit": "chapters"}));
        assert_eq!(
            live_split_choices(&old),
            json!({"ghosting-renewed": "chapters"})
        );
        assert_eq!(path_error_key(Some("no-access")), "#L_S_PATH_NO_ACCESS");
        assert_eq!(path_error_key(Some("invalid")), "#L_S_PATH_INVALID");
        assert_eq!(path_error_key(None), "#L_S_PATH_INVALID");
    }
}
