//! HLSR-CE: свои Source-моды. Игрок добавляет папку мода (с `gameinfo.txt`) или находит моды в
//! `steamapps/sourcemods` всех библиотек Steam; к моду выбирается движок — `hl2.exe` из Steam
//! (Source SDK Base 2013/2007, Half-Life 2), из сборок HLSR (`Games/<id>/hl2.exe`) или свой exe —
//! и параметры запуска. Запуск — `<exe> -game "<папка мода>" <параметры>` из папки движка.
//!
//! Список хранится в `custom-mods.json` папки данных лаунчера. Каналы окна:
//! `custom-mods-list`, `custom-mods-add-folder`, `custom-mods-scan`, `custom-mods-set-engine`,
//! `custom-mods-pick-exe`, `custom-mods-set-args`, `custom-mods-remove`, `custom-mods-launch`,
//! `custom-mods-open-folder`. Каждый отвечает снимком `{ok, code?, added?, mods, engines}`.

use hlsr_delivery::api::{DeliveryApi, DeliveryEvent};
use hlsr_delivery::ports::{Dialogs, FileFilter, OpenDialog, Shell};
use hlsr_delivery::providers::BoxFuture;
use hlsr_ui::services::{ChannelService, Reply};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

/// Событие окну: список модов изменился (колонка перечитывает его).
pub const CHANGED: &str = "custom-mods-changed";

/// Файл списка в папке данных.
pub const FILE: &str = "custom-mods.json";
/// Параметры нового мода.
pub const DEFAULT_ARGS: &str = "-novid -console";
/// Предел длины параметров запуска.
const MAX_ARGS: usize = 1024;
/// Предел числа модов (и найденных за один поиск).
const MAX_MODS: usize = 200;

/// Движки Steam по порядку предпочтения: AppID и exe в папке приложения.
const STEAM_ENGINES: [(u32, &str); 5] = [
    (243730, "Source SDK Base 2013 Singleplayer"),
    (243750, "Source SDK Base 2013 Multiplayer"),
    (218, "Source SDK Base 2007"),
    (220, "Half-Life 2"),
    (215, "Source SDK Base 2006"),
];

// ---- gameinfo.txt --------------------------------------------------------------------------------

/// Что лаунчеру нужно из `gameinfo.txt`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameInfo {
    /// `game "…"` верхнего уровня.
    pub title: Option<String>,
    /// `SteamAppId` из `FileSystem`.
    pub app_id: Option<u32>,
    /// Эпизоды, чьи файлы мод подключает в `SearchPaths`: `ep1` (папка `episodic`), `ep2`.
    pub episodes: Vec<&'static str>,
}

/// Эпизод по пути из `SearchPaths` (`|all_source_engine_paths|episodic`, `ep2/ep2_pak.vpk`,
/// `|appid_420|ep2`): отрезок пути `episodic` — Episode One, `ep2` — Episode Two.
fn episode_of(path: &str) -> Option<&'static str> {
    path.split(['|', '/', '\\'])
        .find_map(|part| match part.trim().to_ascii_lowercase().as_str() {
            "episodic" => Some("ep1"),
            "ep2" => Some("ep2"),
            _ => None,
        })
}

/// Папка движка, где лежат файлы эпизода.
fn episode_folder(episode: &str) -> &'static str {
    if episode == "ep1" { "episodic" } else { "ep2" }
}

/// Эпизоды, которых мод требует, а у движка `exe` (рядом с ним) их папок нет.
pub fn missing_episodes(episodes: &[&'static str], exe: &str) -> Vec<&'static str> {
    let Some(dir) = Path::new(exe).parent().filter(|_| !exe.is_empty()) else {
        return Vec::new();
    };
    episodes
        .iter()
        .copied()
        .filter(|episode| !dir.join(episode_folder(episode)).is_dir())
        .collect()
}

/// Лексемы KeyValues: строки в кавычках, слова, `{` и `}`; комментарии `//` пропускаются.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '/' {
            chars.next();
            if chars.peek() == Some(&'/') {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            } else {
                out.push("/".to_owned());
            }
        } else if c == '{' || c == '}' {
            chars.next();
            out.push(c.to_string());
        } else if c == '"' {
            chars.next();
            let mut word = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                word.push(c);
            }
            out.push(word);
        } else {
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                    break;
                }
                word.push(c);
                chars.next();
            }
            out.push(word);
        }
    }
    out
}

/// Разбор `gameinfo.txt` без строгости: незнакомое пропускается.
pub fn parse_gameinfo(text: &str) -> GameInfo {
    let text = text.trim_start_matches('\u{feff}');
    let mut info = GameInfo::default();
    let mut depth = 0usize;
    let list = tokens(text);
    let mut i = 0;
    while i < list.len() {
        let token = list[i].as_str();
        match token {
            "{" => depth += 1,
            "}" => depth = depth.saturating_sub(1),
            _ => {
                let value = list.get(i + 1).map(String::as_str);
                let is_value = value.is_some_and(|v| v != "{" && v != "}");
                if is_value {
                    let value = value.unwrap_or_default();
                    if depth == 1 && token.eq_ignore_ascii_case("game") && info.title.is_none() {
                        let title = value.trim();
                        if !title.is_empty() {
                            info.title = Some(title.to_owned());
                        }
                    } else if token.eq_ignore_ascii_case("SteamAppId") && info.app_id.is_none() {
                        info.app_id = value.trim().parse().ok();
                    } else if depth >= 3
                        && let Some(episode) = episode_of(value)
                        && !info.episodes.contains(&episode)
                    {
                        // Пути поиска (`FileSystem` → `SearchPaths`).
                        info.episodes.push(episode);
                    }
                    i += 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    info
}

/// `gameinfo.txt` в папке (имя без учёта регистра: в Linux бывает `GameInfo.txt`).
pub fn gameinfo_path(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join("gameinfo.txt");
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        let name = entry.file_name();
        (name.to_string_lossy().eq_ignore_ascii_case("gameinfo.txt") && entry.path().is_file())
            .then(|| entry.path())
    })
}

fn read_gameinfo(dir: &Path) -> Option<GameInfo> {
    let file = gameinfo_path(dir)?;
    let bytes = std::fs::read(file).ok()?;
    Some(parse_gameinfo(&String::from_utf8_lossy(&bytes)))
}

// ---- Параметры запуска ---------------------------------------------------------------------------

/// Параметры строкой → аргументы: пробелы делят, кавычки `"…"` держат пробелы внутри.
pub fn split_args(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

/// Аргументы запуска мода: `-game <папка>` и параметры игрока (свой `-game` игрока не дублируется
/// — он заменяет папку мода).
pub fn launch_args(mod_dir: &str, extra: &str) -> Vec<String> {
    let extra = split_args(extra);
    let own_game = extra.iter().any(|arg| arg.eq_ignore_ascii_case("-game"));
    let mut args = Vec::with_capacity(extra.len() + 2);
    if !own_game {
        args.push("-game".to_owned());
        args.push(mod_dir.to_owned());
    }
    args.extend(extra);
    args
}

/// Ключ сравнения путей: разделители и регистр (Windows) не важны.
fn path_key(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let trimmed = unified.trim_end_matches('/');
    if cfg!(windows) {
        trimmed.to_lowercase()
    } else {
        trimmed.to_owned()
    }
}

// ---- Список --------------------------------------------------------------------------------------

/// Мод в списке.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomMod {
    pub id: String,
    pub name: String,
    pub mod_dir: String,
    pub exe: String,
    pub args: String,
    pub app_id: Option<u32>,
}

impl CustomMod {
    fn from_value(value: &Value) -> Option<CustomMod> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Some(CustomMod {
            id: text("id").filter(|id| !id.is_empty())?,
            name: text("name").unwrap_or_default(),
            mod_dir: text("modDir").filter(|dir| !dir.is_empty())?,
            exe: text("exe").unwrap_or_default(),
            args: text("args").unwrap_or_default(),
            app_id: value
                .get("appId")
                .and_then(Value::as_u64)
                .and_then(|id| u32::try_from(id).ok()),
        })
    }

    fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "modDir": self.mod_dir,
            "exe": self.exe,
            "args": self.args,
            "appId": self.app_id,
        })
    }

    /// Что мешает запуску: нет папки или `gameinfo.txt`, не выбран или пропал движок.
    pub fn problem(&self) -> Option<&'static str> {
        let dir = Path::new(&self.mod_dir);
        if !dir.is_dir() {
            Some("NO_FOLDER")
        } else if gameinfo_path(dir).is_none() {
            Some("NO_GAMEINFO")
        } else if self.exe.is_empty() {
            Some("NO_ENGINE")
        } else if !Path::new(&self.exe).is_file() {
            Some("ENGINE_MISSING")
        } else if is_launcher_exe(&self.exe) {
            // Сам лаунчер движком не бывает: запуск открыл бы ещё один HLSR.
            Some("ENGINE_IS_LAUNCHER")
        } else {
            None
        }
    }
}

/// Это exe самого лаунчера.
fn is_launcher_exe(exe: &str) -> bool {
    let canon = |path: &Path| std::fs::canonicalize(path).ok();
    match (std::env::current_exe().ok(), canon(Path::new(exe))) {
        (Some(own), Some(exe)) => canon(&own).is_some_and(|own| own == exe),
        _ => false,
    }
}

/// Список из JSON файла; испорченные записи пропускаются.
pub fn parse_list(value: &Value) -> Vec<CustomMod> {
    value
        .get("mods")
        .and_then(Value::as_array)
        .map(|mods| mods.iter().filter_map(CustomMod::from_value).collect())
        .unwrap_or_default()
}

fn list_value(mods: &[CustomMod]) -> Value {
    json!({
        "schemaVersion": 1,
        "mods": mods.iter().map(CustomMod::to_value).collect::<Vec<_>>(),
    })
}

/// Движок, который можно выбрать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    pub exe: String,
    pub label: String,
    pub app_id: Option<u32>,
}

/// Сборка HLSR, которая годится движком модов: id игры в библиотеке и название.
pub const HLSR_ENGINES: [(&str, &str); 3] = [
    // Первой — Ghosting: движок Source 2013, на нём идут моды SDK 2013.
    ("ghosting-renewed", "Half-Life 2 Ghosting (HLSR)"),
    ("hl2-4260", "Half-Life 2 build 4260 (HLSR)"),
    ("hl2-2707", "Half-Life 2 build 2707 (HLSR)"),
];

/// Движок для мода: тот же AppID, что в `gameinfo.txt`; иначе HL2 Ghosting из лаунчера, если он
/// скачан; иначе первый (по порядку [`STEAM_ENGINES`], затем сборки HLSR).
pub fn guess_engine(engines: &[Engine], app_id: Option<u32>) -> Option<&Engine> {
    let ghosting = HLSR_ENGINES[0].1;
    app_id
        .and_then(|id| engines.iter().find(|engine| engine.app_id == Some(id)))
        .or_else(|| engines.iter().find(|engine| engine.label == ghosting))
        .or_else(|| engines.first())
}

/// Новый id: время в мс (шестнадцатеричное) и номер при совпадении.
fn new_id(mods: &[CustomMod]) -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let base = format!("m{ms:x}");
    let mut id = base.clone();
    let mut n = 1;
    while mods.iter().any(|m| m.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

/// Добавить папку мода, если её ещё нет: `Some(true)` — добавлен, `Some(false)` — уже есть,
/// `None` — это не мод (нет `gameinfo.txt`).
pub fn add_mod(mods: &mut Vec<CustomMod>, dir: &Path, engines: &[Engine]) -> Option<bool> {
    let info = read_gameinfo(dir)?;
    let mod_dir = dir.to_string_lossy().into_owned();
    let key = path_key(&mod_dir);
    if mods.iter().any(|m| path_key(&m.mod_dir) == key) {
        return Some(false);
    }
    if mods.len() >= MAX_MODS {
        return Some(false);
    }
    let name = info.title.clone().unwrap_or_else(|| {
        dir.file_name()
            .map_or_else(|| mod_dir.clone(), |n| n.to_string_lossy().into_owned())
    });
    let exe = guess_engine(engines, info.app_id).map_or_else(String::new, |e| e.exe.clone());
    let id = new_id(mods);
    mods.push(CustomMod {
        id,
        name,
        mod_dir,
        exe,
        args: DEFAULT_ARGS.to_owned(),
        app_id: info.app_id,
    });
    Some(true)
}

/// Папки модов внутри `root` (на один уровень) или сама `root`, если в ней `gameinfo.txt`.
pub fn mod_dirs_in(root: &Path) -> Vec<PathBuf> {
    if gameinfo_path(root).is_some() {
        return vec![root.to_path_buf()];
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && gameinfo_path(path).is_some())
        .take(MAX_MODS)
        .collect();
    dirs.sort();
    dirs
}

/// Папки модов в распакованном архиве: сама папка или вложенные (до `depth` уровней:
/// `Мод/gameinfo.txt`, `sourcemods/Мод/gameinfo.txt`). Внутрь найденного мода не спускается.
pub fn find_mod_dirs(root: &Path, depth: usize) -> Vec<PathBuf> {
    if gameinfo_path(root).is_some() {
        return vec![root.to_path_buf()];
    }
    if depth == 0 {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs.iter()
        .flat_map(|dir| find_mod_dirs(dir, depth - 1))
        .take(MAX_MODS)
        .collect()
}

/// Перенести папку мода в `root` под её именем (занято — `имя-2`, `имя-3`…). Архив без папки
/// (`gameinfo.txt` в корне) получает имя папки распаковки — такой мод переименовывается в `mod`.
fn move_into(dir: &Path, root: &Path) -> Option<PathBuf> {
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.starts_with(".hlsr-unpack-"))
        .unwrap_or_else(|| "mod".to_owned());
    let mut target = root.join(&name);
    let mut n = 2;
    while target.exists() {
        target = root.join(format!("{name}-{n}"));
        n += 1;
    }
    match std::fs::rename(dir, &target) {
        Ok(()) => Some(target),
        Err(error) => {
            tracing::warn!("[custom-mods] {} не перенесён: {error}", dir.display());
            None
        }
    }
}

// ---- Каналы --------------------------------------------------------------------------------------

/// Сервис каналов «Своих модов».
pub struct CustomMods {
    file: PathBuf,
    /// Куда ставятся моды из архива без Steam (`<данные>/mods`).
    own_root: PathBuf,
    delivery: Arc<DeliveryApi>,
    shell: Arc<dyn Shell>,
    dialogs: Arc<dyn Dialogs>,
    /// Чтение-изменение-запись списка — по одному.
    lock: Mutex<()>,
    /// [`CHANGED`] после каждого сохранения списка.
    events: broadcast::Sender<DeliveryEvent>,
}

impl CustomMods {
    pub fn new(
        data_dir: &Path,
        delivery: Arc<DeliveryApi>,
        shell: Arc<dyn Shell>,
        dialogs: Arc<dyn Dialogs>,
    ) -> CustomMods {
        CustomMods {
            file: data_dir.join(FILE),
            own_root: data_dir.join("mods"),
            delivery,
            shell,
            dialogs,
            lock: Mutex::new(()),
            events: broadcast::channel(16).0,
        }
    }

    /// События окну ([`CHANGED`]).
    pub fn subscribe(&self) -> broadcast::Receiver<DeliveryEvent> {
        self.events.subscribe()
    }

    fn load(&self) -> Vec<CustomMod> {
        std::fs::read(&self.file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .map(|value| parse_list(&value))
            .unwrap_or_default()
    }

    fn save(&self, mods: &[CustomMod]) -> bool {
        let text = match serde_json::to_vec_pretty(&list_value(mods)) {
            Ok(text) => text,
            Err(_) => return false,
        };
        let tmp = self.file.with_extension("json.tmp");
        let written = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &self.file));
        match &written {
            Ok(()) => {
                let _ = self.events.send(DeliveryEvent {
                    channel: CHANGED,
                    payload: Value::Null,
                });
            }
            Err(error) => tracing::warn!("[custom-mods] список не сохранён: {error}"),
        }
        written.is_ok()
    }

    /// Изменить список под замком; `true` из `change` — сохранить.
    fn update<T>(&self, change: impl FnOnce(&mut Vec<CustomMod>) -> (bool, T)) -> (bool, T) {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut mods = self.load();
        let (dirty, out) = change(&mut mods);
        let saved = !dirty || self.save(&mods);
        (saved, out)
    }

    /// Библиотеки Steam (Windows); в Linux Steam здесь не ищется.
    fn steam_libraries() -> Vec<PathBuf> {
        #[cfg(windows)]
        {
            use hlsr_platform::steam_locator::{LocatorOptions, SteamLocator};
            let locator = SteamLocator::new(LocatorOptions::default());
            match locator.locate_steam() {
                Ok(steam) => locator
                    .steam_libraries(&steam)
                    .into_iter()
                    .map(PathBuf::from)
                    .collect(),
                Err(_) => Vec::new(),
            }
        }
        #[cfg(not(windows))]
        {
            Vec::new()
        }
    }

    /// Движки: приложения Steam из [`STEAM_ENGINES`] и сборки HLSR с `hl2.exe`.
    fn engines(&self) -> Vec<Engine> {
        let mut engines = Vec::new();
        #[cfg(windows)]
        {
            use hlsr_platform::steam_locator::{LocatorOptions, SteamLocator};
            let locator = SteamLocator::new(LocatorOptions::default());
            if let Ok(steam) = locator.locate_steam() {
                for (app_id, fallback) in STEAM_ENGINES {
                    let Ok(app) = locator.find_app(&steam, app_id) else {
                        continue;
                    };
                    let exe = Path::new(&app.app_path).join("hl2.exe");
                    if exe.is_file() {
                        engines.push(Engine {
                            exe: exe.to_string_lossy().into_owned(),
                            label: app.name.clone().unwrap_or_else(|| fallback.to_owned()),
                            app_id: Some(app_id),
                        });
                    }
                }
            }
        }
        // Сборки, скачанные лаунчером: `Games/<id>/hl2.exe` библиотеки (Ghosting — первым).
        let library = self.delivery.current_library_path();
        if !library.is_empty() {
            let games = Path::new(&library).join("Games");
            for (id, label) in HLSR_ENGINES {
                let exe = games.join(id).join("hl2.exe");
                if exe.is_file() {
                    engines.push(Engine {
                        exe: exe.to_string_lossy().into_owned(),
                        label: label.to_owned(),
                        app_id: None,
                    });
                }
            }
        }
        engines
    }

    /// Снимок для окна: моды с проблемами и движки (свой exe мода — тоже в списке).
    fn snapshot(&self, extra: Map<String, Value>) -> Value {
        let mods = {
            let _guard = self
                .lock
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            self.load()
        };
        let mut engines = self.engines();
        // Свои exe модов: окно подписывает их отдельно.
        let mut custom = Vec::new();
        // Найденные движки — до своих exe: подсказка берётся только из них.
        let found = engines.clone();
        for m in &mods {
            // Сам лаунчер в список движков не попадает (мод с ним — `ENGINE_IS_LAUNCHER`).
            if !m.exe.is_empty()
                && !is_launcher_exe(&m.exe)
                && !engines.iter().any(|e| path_key(&e.exe) == path_key(&m.exe))
            {
                let label = Path::new(&m.exe)
                    .parent()
                    .and_then(Path::file_name)
                    .map_or_else(|| m.exe.clone(), |n| n.to_string_lossy().into_owned());
                engines.push(Engine {
                    exe: m.exe.clone(),
                    label,
                    app_id: None,
                });
                custom.push(m.exe.clone());
            }
        }
        let mut out = extra;
        out.entry("ok").or_insert(json!(true));
        out.insert(
            "mods".into(),
            Value::Array(
                mods.iter()
                    .map(|m| {
                        let mut value = m.to_value();
                        if let Value::Object(map) = &mut value {
                            let problem = m.problem();
                            map.insert("problem".into(), json!(problem));
                            // Движка нет или он не годится — предложить найденный (Ghosting из
                            // лаунчера, если скачан).
                            let suggested = matches!(
                                problem,
                                Some("NO_ENGINE" | "ENGINE_MISSING" | "ENGINE_IS_LAUNCHER")
                            )
                            .then(|| guess_engine(&found, m.app_id))
                            .flatten()
                            .map(|e| json!({"exe": e.exe, "label": e.label}));
                            map.insert("suggested".into(), suggested.unwrap_or(Value::Null));
                            // Эпизоды, которые мод подключает, а у движка их файлов нет.
                            let missing = read_gameinfo(Path::new(&m.mod_dir))
                                .map(|info| missing_episodes(&info.episodes, &m.exe))
                                .unwrap_or_default();
                            map.insert("missingEpisodes".into(), json!(missing));
                        }
                        value
                    })
                    .collect(),
            ),
        );
        out.insert(
            "engines".into(),
            Value::Array(
                engines
                    .iter()
                    .map(|e| {
                        json!({
                            "exe": e.exe,
                            "label": e.label,
                            "appId": e.app_id,
                            "custom": custom.contains(&e.exe),
                        })
                    })
                    .collect(),
            ),
        );
        Value::Object(out)
    }

    fn reply(&self, ok: bool, code: Option<&str>, added: Option<usize>) -> Value {
        let mut extra = Map::new();
        extra.insert("ok".into(), json!(ok));
        if let Some(code) = code {
            extra.insert("code".into(), json!(code));
        }
        if let Some(added) = added {
            extra.insert("added".into(), json!(added));
        }
        self.snapshot(extra)
    }

    fn add_dirs(&self, dirs: &[PathBuf]) -> (bool, usize) {
        let engines = self.engines();
        let (saved, added) = self.update(|mods| {
            let added = dirs
                .iter()
                .filter(|dir| add_mod(mods, dir, &engines) == Some(true))
                .count();
            (added > 0, added)
        });
        (saved, added)
    }

    /// `custom-mods-add-folder`: папка мода или папка с модами (например `sourcemods`).
    async fn add_folder(&self) -> Value {
        let picked = self
            .dialogs
            .show_open_dialog(OpenDialog {
                properties: vec!["openDirectory"],
                ..OpenDialog::default()
            })
            .await;
        let Some(folder) = picked.into_iter().next() else {
            return self.reply(true, Some("CANCELED"), Some(0));
        };
        let dirs = mod_dirs_in(&folder);
        if dirs.is_empty() {
            return self.reply(false, Some("NOT_A_MOD"), Some(0));
        }
        let (saved, added) = self.add_dirs(&dirs);
        if !saved {
            return self.reply(false, Some("SAVE_FAILED"), Some(0));
        }
        self.reply(true, (added == 0).then_some("ALREADY_ADDED"), Some(added))
    }

    /// `custom-mods-scan`: `steamapps/sourcemods` всех библиотек Steam.
    fn scan(&self) -> Value {
        let libraries = Self::steam_libraries();
        if libraries.is_empty() {
            return self.reply(false, Some("NO_STEAM"), Some(0));
        }
        let dirs: Vec<PathBuf> = libraries
            .iter()
            .flat_map(|library| {
                let root = library.join("steamapps").join("sourcemods");
                if root.is_dir() {
                    mod_dirs_in(&root)
                } else {
                    Vec::new()
                }
            })
            .collect();
        let (saved, added) = self.add_dirs(&dirs);
        if !saved {
            return self.reply(false, Some("SAVE_FAILED"), Some(0));
        }
        self.reply(true, None, Some(added))
    }

    /// Изменить одно поле мода.
    fn edit(&self, id: &str, change: impl FnOnce(&mut CustomMod)) -> Value {
        let (saved, found) = self.update(|mods| match mods.iter_mut().find(|m| m.id == id) {
            Some(m) => {
                change(m);
                (true, true)
            }
            None => (false, false),
        });
        match (found, saved) {
            (false, _) => self.reply(false, Some("NOT_FOUND"), None),
            (true, false) => self.reply(false, Some("SAVE_FAILED"), None),
            (true, true) => self.reply(true, None, None),
        }
    }

    fn find(&self, id: &str) -> Option<CustomMod> {
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.load().into_iter().find(|m| m.id == id)
    }

    /// Движок мода; сам лаунчер — отказ.
    fn set_engine(&self, id: &str, exe: String) -> Value {
        if is_launcher_exe(&exe) {
            return self.reply(false, Some("ENGINE_IS_LAUNCHER"), None);
        }
        self.edit(id, move |m| m.exe = exe)
    }

    /// `custom-mods-pick-exe`: свой exe движка.
    async fn pick_exe(&self, id: &str) -> Value {
        let current = self.find(id).map(|m| m.exe).filter(|exe| !exe.is_empty());
        let picked = self
            .dialogs
            .show_open_dialog(OpenDialog {
                title: None,
                default_path: current,
                properties: vec!["openFile"],
                filters: vec![FileFilter::new("exe", &["exe"])],
            })
            .await;
        match picked.into_iter().next() {
            Some(exe) => self.set_engine(id, exe.to_string_lossy().into_owned()),
            None => self.reply(true, Some("CANCELED"), None),
        }
    }

    /// `custom-mods-launch`.
    fn launch(&self, id: &str) -> Value {
        let Some(m) = self.find(id) else {
            return self.reply(false, Some("NOT_FOUND"), None);
        };
        if let Some(problem) = m.problem() {
            return self.reply(false, Some(problem), None);
        }
        let exe = Path::new(&m.exe);
        let args = launch_args(&m.mod_dir, &m.args);
        let mut command = std::process::Command::new(exe);
        command
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(dir) = exe.parent() {
            command.current_dir(dir);
        }
        match command.spawn() {
            Ok(child) => {
                tracing::info!(
                    "[custom-mods] запущен «{}» (pid {}): {} {:?}",
                    m.name,
                    child.id(),
                    m.exe,
                    args
                );
                self.reply(true, None, None)
            }
            Err(error) => {
                tracing::warn!("[custom-mods] «{}» не запустился: {error}", m.name);
                self.reply(false, Some("SPAWN_FAILED"), None)
            }
        }
    }

    /// `custom-mods-open-folder`.
    async fn open_folder(&self, id: &str) -> Value {
        let Some(m) = self.find(id) else {
            return self.reply(false, Some("NOT_FOUND"), None);
        };
        let error = self.shell.open_path(Path::new(&m.mod_dir)).await;
        if error.is_empty() {
            self.reply(true, None, None)
        } else {
            self.reply(false, Some("OPEN_FAILED"), None)
        }
    }

    /// Папка, куда ставятся моды из архива: `sourcemods` Steam (Steam сам покажет их в
    /// библиотеке после перезапуска), без Steam — своя папка в данных лаунчера.
    fn install_root(&self) -> PathBuf {
        Self::steam_libraries()
            .first()
            .map(|steam| steam.join("steamapps").join("sourcemods"))
            .unwrap_or_else(|| self.own_root.clone())
    }

    /// `custom-mods-install-archive`: архив мода (zip или 7z) → распаковка 7-Zip лаунчера
    /// во временную папку рядом с модами → папки с `gameinfo.txt` переносятся в папку модов и
    /// добавляются в список.
    async fn install_archive(&self) -> Value {
        let picked = self
            .dialogs
            .show_open_dialog(OpenDialog {
                title: None,
                default_path: None,
                properties: vec!["openFile"],
                filters: vec![FileFilter::new("zip, 7z", &["zip", "7z"])],
            })
            .await;
        let Some(archive) = picked.into_iter().next() else {
            return self.reply(true, Some("CANCELED"), Some(0));
        };
        let root = self.install_root();
        let staging = root.join(format!(".hlsr-unpack-{}", new_id(&[])));
        if let Err(error) = std::fs::create_dir_all(&staging) {
            tracing::warn!("[custom-mods] папка распаковки не создана: {error}");
            return self.reply(false, Some("UNPACK_FAILED"), Some(0));
        }
        let logger: Arc<dyn hlsr_delivery::ports::Logger> =
            Arc::new(hlsr_delivery::ports::TracingLogger);
        let unpacked = hlsr_delivery::archive_service::ArchiveService::with_default_bin(logger)
            .extract(&archive, &staging, Default::default())
            .await;
        let reply = match unpacked {
            Err(error) => {
                tracing::warn!(
                    "[custom-mods] архив {} не распакован: {}",
                    archive.display(),
                    error.message
                );
                self.reply(false, Some("UNPACK_FAILED"), Some(0))
            }
            Ok(_) => {
                let found = find_mod_dirs(&staging, 3);
                if found.is_empty() {
                    self.reply(false, Some("NOT_A_MOD"), Some(0))
                } else {
                    let moved: Vec<PathBuf> = found
                        .iter()
                        .filter_map(|dir| move_into(dir, &root))
                        .collect();
                    tracing::info!(
                        "[custom-mods] из {} поставлено модов: {}",
                        archive.display(),
                        moved.len()
                    );
                    let (saved, added) = self.add_dirs(&moved);
                    if saved {
                        self.reply(true, None, Some(added))
                    } else {
                        self.reply(false, Some("SAVE_FAILED"), Some(0))
                    }
                }
            }
        };
        let _ = std::fs::remove_dir_all(&staging);
        reply
    }

    fn remove(&self, id: &str) -> Value {
        let (saved, found) = self.update(|mods| {
            let before = mods.len();
            mods.retain(|m| m.id != id);
            let found = mods.len() != before;
            (found, found)
        });
        match (found, saved) {
            (false, _) => self.reply(false, Some("NOT_FOUND"), None),
            (true, false) => self.reply(false, Some("SAVE_FAILED"), None),
            (true, true) => self.reply(true, None, None),
        }
    }
}

fn text_arg(args: &[Value], index: usize) -> String {
    args.get(index)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

impl ChannelService for CustomMods {
    fn call<'a>(&'a self, channel: &'a str, args: &'a [Value]) -> BoxFuture<'a, Option<Reply>> {
        Box::pin(async move {
            if !channel.starts_with("custom-mods-") {
                return None;
            }
            let id = text_arg(args, 0);
            let reply = match channel {
                "custom-mods-list" => self.reply(true, None, None),
                "custom-mods-add-folder" => self.add_folder().await,
                "custom-mods-scan" => self.scan(),
                "custom-mods-set-engine" => self.set_engine(&id, text_arg(args, 1)),
                "custom-mods-pick-exe" => self.pick_exe(&id).await,
                "custom-mods-set-args" => {
                    let text: String = text_arg(args, 1)
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(MAX_ARGS)
                        .collect();
                    self.edit(&id, move |m| m.args = text)
                }
                "custom-mods-remove" => self.remove(&id),
                "custom-mods-launch" => self.launch(&id),
                "custom-mods-open-folder" => self.open_folder(&id).await,
                "custom-mods-install-archive" => self.install_archive().await,
                _ => return None,
            };
            Some(Ok(reply))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAMEINFO: &str = r#"
"GameInfo"
{
	game	"Entropy : Zero 2"
	title	"ENTROPY : ZERO 2" // заголовок меню
	type	singleplayer_only

	FileSystem
	{
		SteamAppId		243730		// Source SDK Base 2013 Singleplayer
		SearchPaths
		{
			game+mod			|gameinfo_path|.
			game				|all_source_engine_paths|hl2
		}
	}
}
"#;

    #[test]
    fn gameinfo_title_and_app_id() {
        assert_eq!(
            parse_gameinfo(GAMEINFO),
            GameInfo {
                title: Some("Entropy : Zero 2".into()),
                app_id: Some(243730),
                episodes: Vec::new(),
            }
        );
        assert_eq!(parse_gameinfo(""), GameInfo::default());
        // Вложенный `game` (в SearchPaths) — не название.
        assert_eq!(
            parse_gameinfo("GameInfo { FileSystem { SearchPaths { game hl2 } } }").title,
            None
        );
        // BOM и ключи без учёта регистра.
        assert_eq!(
            parse_gameinfo("\u{feff}\"GameInfo\" { \"GAME\" \"Mod\" steamappid \"220\" }"),
            GameInfo {
                title: Some("Mod".into()),
                app_id: Some(220),
                episodes: Vec::new(),
            }
        );
    }

    #[test]
    fn archive_layouts() {
        let dir = tempfile::tempdir().unwrap();
        let unpack = dir.path().join(".hlsr-unpack-x");
        let nested = unpack.join("sourcemods").join("Awakening");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("gameinfo.txt"), GAMEINFO).unwrap();
        assert_eq!(find_mod_dirs(&unpack, 3), vec![nested.clone()]);
        assert!(find_mod_dirs(&unpack, 1).is_empty());

        // Перенос в папку модов: занятое имя — с номером.
        let root = dir.path().join("sourcemods");
        std::fs::create_dir_all(root.join("Awakening")).unwrap();
        assert_eq!(move_into(&nested, &root), Some(root.join("Awakening-2")));
        assert!(root.join("Awakening-2").join("gameinfo.txt").is_file());

        // gameinfo.txt в корне архива — папка «mod».
        let flat = dir.path().join(".hlsr-unpack-y");
        std::fs::create_dir_all(&flat).unwrap();
        std::fs::write(flat.join("gameinfo.txt"), GAMEINFO).unwrap();
        assert_eq!(move_into(&flat, &root), Some(root.join("mod")));
    }

    #[test]
    fn episodes_from_search_paths_and_engine_folders() {
        let text = r#""GameInfo" { game "EP2 mod" FileSystem { SteamAppId 243730
            SearchPaths {
                game+mod |gameinfo_path|.
                game |all_source_engine_paths|ep2/ep2_pak.vpk
                game |all_source_engine_paths|episodic
                game |all_source_engine_paths|ep2
                game |all_source_engine_paths|hl2
            } } }"#;
        assert_eq!(parse_gameinfo(text).episodes, ["ep2", "ep1"]);
        // Название мода — не путь поиска.
        assert!(
            parse_gameinfo(r#""GameInfo" { game "episodic" }"#)
                .episodes
                .is_empty()
        );

        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("hl2.exe");
        let exe = exe.to_string_lossy();
        assert_eq!(missing_episodes(&["ep1", "ep2"], &exe), ["ep1", "ep2"]);
        std::fs::create_dir(dir.path().join("episodic")).unwrap();
        assert_eq!(missing_episodes(&["ep1", "ep2"], &exe), ["ep2"]);
        // Движок не выбран — предупреждать не о чем (есть своя проблема).
        assert!(missing_episodes(&["ep1"], "").is_empty());
    }

    #[test]
    fn args_split_and_quotes() {
        assert_eq!(split_args("-novid  -console"), ["-novid", "-console"]);
        assert_eq!(
            split_args(r#"+map "my map" -w 1920"#),
            ["+map", "my map", "-w", "1920"]
        );
        assert_eq!(split_args(r#""""#), [""]);
        assert!(split_args("   ").is_empty());
        assert_eq!(
            launch_args(r"C:\mods\ez2", "-novid"),
            ["-game", r"C:\mods\ez2", "-novid"]
        );
        // Свой -game игрока заменяет папку мода.
        assert_eq!(
            launch_args(r"C:\mods\ez2", "-game ep2 -dev"),
            ["-game", "ep2", "-dev"]
        );
    }

    #[test]
    fn engine_guess_prefers_same_app_id() {
        let engines = vec![
            Engine {
                exe: "a".into(),
                label: "SDK".into(),
                app_id: Some(243730),
            },
            Engine {
                exe: "b".into(),
                label: "HL2".into(),
                app_id: Some(220),
            },
        ];
        assert_eq!(
            guess_engine(&engines, Some(220)).map(|e| &*e.exe),
            Some("b")
        );
        assert_eq!(guess_engine(&engines, Some(1)).map(|e| &*e.exe), Some("a"));
        assert_eq!(guess_engine(&engines, None).map(|e| &*e.exe), Some("a"));
        assert!(guess_engine(&[], Some(220)).is_none());
    }

    #[test]
    fn ghosting_from_the_launcher_is_suggested_without_app_id_match() {
        let engines = vec![
            Engine {
                exe: "steam/hl2.exe".into(),
                label: "Half-Life 2".into(),
                app_id: Some(220),
            },
            Engine {
                exe: "lib/Games/ghosting-renewed/hl2.exe".into(),
                label: HLSR_ENGINES[0].1.into(),
                app_id: None,
            },
        ];
        // AppID совпал — его движок.
        assert_eq!(
            guess_engine(&engines, Some(220)).map(|e| &*e.exe),
            Some("steam/hl2.exe")
        );
        // SDK 2013 не установлен — Ghosting из лаунчера.
        assert_eq!(
            guess_engine(&engines, Some(243730)).map(|e| &*e.exe),
            Some("lib/Games/ghosting-renewed/hl2.exe")
        );
        assert_eq!(
            guess_engine(&engines, None).map(|e| &*e.exe),
            Some("lib/Games/ghosting-renewed/hl2.exe")
        );
    }

    #[test]
    fn launcher_is_never_an_engine() {
        let own = std::env::current_exe().unwrap();
        assert!(is_launcher_exe(&own.to_string_lossy()));
        assert!(!is_launcher_exe("C:/nope/hl2.exe"));
    }

    #[test]
    fn add_mod_reads_gameinfo_and_skips_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let sourcemods = dir.path().join("sourcemods");
        let ez2 = sourcemods.join("EZ2");
        let other = sourcemods.join("notamod");
        std::fs::create_dir_all(&ez2).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(ez2.join("GameInfo.txt"), GAMEINFO).unwrap();

        assert_eq!(mod_dirs_in(&sourcemods), vec![ez2.clone()]);
        assert_eq!(mod_dirs_in(&ez2), vec![ez2.clone()]);
        assert!(mod_dirs_in(&other).is_empty());

        let engines = vec![Engine {
            exe: "hl2.exe".into(),
            label: "SDK".into(),
            app_id: Some(243730),
        }];
        let mut mods = Vec::new();
        assert_eq!(add_mod(&mut mods, &ez2, &engines), Some(true));
        assert_eq!(add_mod(&mut mods, &ez2, &engines), Some(false));
        assert_eq!(add_mod(&mut mods, &other, &engines), None);
        assert_eq!(mods.len(), 1);
        let m = &mods[0];
        assert_eq!(m.name, "Entropy : Zero 2");
        assert_eq!(m.exe, "hl2.exe");
        assert_eq!(m.args, DEFAULT_ARGS);
        assert_eq!(m.app_id, Some(243730));
        // Движок не найден на диске.
        assert_eq!(m.problem(), Some("ENGINE_MISSING"));

        // Круг через JSON.
        let back = parse_list(&list_value(&mods));
        assert_eq!(back, mods);
        // Испорченная запись пропускается.
        let broken = json!({"mods": [{"id": "", "modDir": "x"}, {"id": "a"}, m.to_value()]});
        assert_eq!(parse_list(&broken), mods);
    }
}
