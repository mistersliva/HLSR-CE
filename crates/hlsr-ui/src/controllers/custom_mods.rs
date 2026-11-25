//! HLSR-CE: раздел настроек «Свои моды» без окна — снимок каналов `custom-mods-*` (папка мода,
//! движок, параметры запуска, что мешает запуску) и итог действия для заметки под кнопками.

use serde_json::Value;

/// Мод в списке.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModEntry {
    pub id: String,
    pub name: String,
    pub mod_dir: String,
    pub exe: String,
    pub args: String,
    /// Код того, что мешает запуску (`NO_FOLDER`, `NO_GAMEINFO`, `NO_ENGINE`, `ENGINE_MISSING`).
    pub problem: Option<String>,
    /// Движок, который стоит взять вместо нерабочего (Ghosting из лаунчера, если скачан).
    pub suggested: Option<EngineEntry>,
    /// Эпизоды (`ep1`, `ep2`), которые мод подключает, а у движка их файлов нет.
    pub missing_episodes: Vec<String>,
}

/// Ключ жёлтого предупреждения о недостающем эпизоде.
pub fn episode_key(episode: &str) -> &'static str {
    if episode == "ep1" {
        "#L_MODS_NEED_EP1"
    } else {
        "#L_MODS_NEED_EP2"
    }
}

/// Галочки параметров запуска на странице мода: флаг и ключ подписи.
pub const LAUNCH_FLAGS: [(&str, &str); 6] = [
    ("-windowed", "#L_MODS_FLAG_WINDOWED"),
    ("-noborder", "#L_MODS_FLAG_NOBORDER"),
    ("-fullscreen", "#L_MODS_FLAG_FULLSCREEN"),
    ("-novid", "#L_MODS_FLAG_NOVID"),
    ("-console", "#L_MODS_FLAG_CONSOLE"),
    ("-dev", "#L_MODS_FLAG_DEV"),
];

/// Флаг есть в параметрах (отдельным словом, без учёта регистра).
pub fn has_flag(args: &str, flag: &str) -> bool {
    args.split_whitespace()
        .any(|word| word.eq_ignore_ascii_case(flag))
}

/// Параметры с флагом или без него; прочие слова и их порядок не меняются. `-windowed` и
/// `-fullscreen` друг друга исключают.
pub fn set_flag(args: &str, flag: &str, on: bool) -> String {
    let opposite = match flag {
        "-windowed" => Some("-fullscreen"),
        "-fullscreen" => Some("-windowed"),
        _ => None,
    };
    let mut words: Vec<&str> = args
        .split_whitespace()
        .filter(|word| {
            !word.eq_ignore_ascii_case(flag)
                && !(on && opposite.is_some_and(|o| word.eq_ignore_ascii_case(o)))
        })
        .collect();
    if on {
        words.push(flag);
    }
    words.join(" ")
}

/// Движок, который можно выбрать.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineEntry {
    pub exe: String,
    pub label: String,
    /// Свой exe мода (не найден среди Steam и сборок HLSR).
    pub custom: bool,
}

/// Снимок раздела.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModsView {
    pub loaded: bool,
    pub mods: Vec<ModEntry>,
    pub engines: Vec<EngineEntry>,
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

impl ModsView {
    /// Снимок из ответа канала; не снимок — `None` (прежний остаётся).
    pub fn from_value(value: &Value) -> Option<ModsView> {
        let mods = value.get("mods")?.as_array()?;
        let engines = value.get("engines").and_then(Value::as_array);
        Some(ModsView {
            loaded: true,
            mods: mods
                .iter()
                .filter(|m| !text(m, "id").is_empty())
                .map(|m| ModEntry {
                    id: text(m, "id"),
                    name: text(m, "name"),
                    mod_dir: text(m, "modDir"),
                    exe: text(m, "exe"),
                    args: text(m, "args"),
                    problem: m.get("problem").and_then(Value::as_str).map(str::to_owned),
                    suggested: m
                        .get("suggested")
                        .filter(|s| !text(s, "exe").is_empty())
                        .map(|s| EngineEntry {
                            exe: text(s, "exe"),
                            label: text(s, "label"),
                            custom: false,
                        }),
                    missing_episodes: m
                        .get("missingEpisodes")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                })
                .collect(),
            engines: engines
                .into_iter()
                .flatten()
                .filter(|e| !text(e, "exe").is_empty())
                .map(|e| EngineEntry {
                    exe: text(e, "exe"),
                    label: text(e, "label"),
                    custom: e.get("custom").and_then(Value::as_bool).unwrap_or(false),
                })
                .collect(),
        })
    }

    /// Подпись движка мода: из списка движков или имя файла.
    pub fn engine_label<'a>(&'a self, exe: &'a str) -> Option<&'a str> {
        if exe.is_empty() {
            return None;
        }
        self.engines
            .iter()
            .find(|e| e.exe == exe)
            .map(|e| e.label.as_str())
            .or(Some(exe))
    }
}

/// Ключ строки проблемы мода.
pub fn problem_key(code: &str) -> &'static str {
    match code {
        "NO_FOLDER" => "#L_MODS_P_NO_FOLDER",
        "NO_GAMEINFO" => "#L_MODS_P_NO_GAMEINFO",
        "NO_ENGINE" => "#L_MODS_P_NO_ENGINE",
        "ENGINE_MISSING" => "#L_MODS_P_ENGINE_MISSING",
        "ENGINE_IS_LAUNCHER" => "#L_MODS_P_ENGINE_IS_LAUNCHER",
        _ => "#L_MODS_P_UNKNOWN",
    }
}

/// Заметка после действия: ключ строки, её аргумент и ошибка ли это. `None` — заметка не нужна.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub key: &'static str,
    pub arg: Option<String>,
    pub error: bool,
}

/// Итог ответа канала `channel` для заметки.
pub fn note_for(channel: &str, reply: &Value) -> Option<Note> {
    let ok = reply.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let code = reply.get("code").and_then(Value::as_str);
    let added = reply.get("added").and_then(Value::as_u64);
    let note = |key, arg: Option<String>, error| Some(Note { key, arg, error });
    match (ok, code) {
        (_, Some("CANCELED")) => None,
        (true, Some("ALREADY_ADDED")) => note("#L_MODS_ALREADY", None, false),
        (true, _) if added.is_some() => match added.unwrap_or(0) {
            0 => note("#L_MODS_FOUND_NONE", None, false),
            n => note("#L_MODS_ADDED", Some(n.to_string()), false),
        },
        (true, _) if channel == "custom-mods-launch" => note("#L_MODS_LAUNCHED", None, false),
        (true, _) => None,
        (false, Some("NOT_A_MOD")) => note("#L_MODS_E_NOT_A_MOD", None, true),
        (false, Some("NO_STEAM")) => note("#L_MODS_E_NO_STEAM", None, true),
        (false, Some("SPAWN_FAILED")) => note("#L_MODS_E_SPAWN", None, true),
        (false, Some("UNPACK_FAILED")) => note("#L_MODS_E_UNPACK", None, true),
        (false, Some(code)) if problem_key(code) != "#L_MODS_P_UNKNOWN" => {
            note(problem_key(code), None, true)
        }
        (false, _) => note("#L_MODS_E_FAILED", None, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn view_from_reply() {
        let reply = json!({
            "ok": true,
            "mods": [
                {"id": "m1", "name": "EZ2", "modDir": "C:/sm/EZ2", "exe": "C:/sdk/hl2.exe",
                 "args": "-novid", "problem": null},
                {"id": "", "name": "broken"},
                {"id": "m2", "name": "X", "modDir": "C:/sm/X", "exe": "", "args": "",
                 "problem": "NO_ENGINE"}
            ],
            "engines": [
                {"exe": "C:/sdk/hl2.exe", "label": "Source SDK Base 2013 Singleplayer"},
                {"exe": "D:/own/hl2.exe", "label": "own", "custom": true}
            ]
        });
        let view = ModsView::from_value(&reply).unwrap();
        assert!(view.loaded);
        assert_eq!(view.mods.len(), 2);
        assert_eq!(view.mods[1].problem.as_deref(), Some("NO_ENGINE"));
        assert!(view.engines[1].custom);
        assert_eq!(
            view.engine_label("C:/sdk/hl2.exe"),
            Some("Source SDK Base 2013 Singleplayer")
        );
        assert_eq!(view.engine_label("E:/x.exe"), Some("E:/x.exe"));
        assert_eq!(view.engine_label(""), None);
        assert!(ModsView::from_value(&json!({"ok": false})).is_none());
    }

    #[test]
    fn launch_flags_toggle_as_words() {
        assert!(has_flag("-novid -Windowed", "-windowed"));
        assert!(!has_flag("-windowed2", "-windowed"));
        assert_eq!(set_flag("-novid", "-windowed", true), "-novid -windowed");
        assert_eq!(
            set_flag("-novid -windowed", "-windowed", true),
            "-novid -windowed"
        );
        assert_eq!(set_flag("-windowed  +map x", "-windowed", false), "+map x");
        // Окно и полный экран исключают друг друга.
        assert_eq!(
            set_flag("-fullscreen -novid", "-windowed", true),
            "-novid -windowed"
        );
        assert_eq!(set_flag("", "-noborder", true), "-noborder");
    }

    #[test]
    fn notes_for_replies() {
        let added = note_for("custom-mods-scan", &json!({"ok": true, "added": 3})).unwrap();
        assert_eq!(added.key, "#L_MODS_ADDED");
        assert_eq!(added.arg.as_deref(), Some("3"));
        assert_eq!(
            note_for("custom-mods-scan", &json!({"ok": true, "added": 0})).map(|n| n.key),
            Some("#L_MODS_FOUND_NONE")
        );
        assert_eq!(
            note_for(
                "custom-mods-add-folder",
                &json!({"ok": true, "code": "CANCELED"})
            ),
            None
        );
        assert_eq!(note_for("custom-mods-set-args", &json!({"ok": true})), None);
        let launched = note_for("custom-mods-launch", &json!({"ok": true})).unwrap();
        assert!(!launched.error);
        let missing = note_for(
            "custom-mods-launch",
            &json!({"ok": false, "code": "ENGINE_MISSING"}),
        )
        .unwrap();
        assert_eq!(missing.key, "#L_MODS_P_ENGINE_MISSING");
        assert!(missing.error);
        assert_eq!(
            note_for("custom-mods-remove", &json!({"ok": false})).map(|n| n.key),
            Some("#L_MODS_E_FAILED")
        );
    }
}
