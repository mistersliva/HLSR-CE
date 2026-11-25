//! Настройки лаунчера в окне GPUI на фикстурах: окно говорит с главным процессом как 0.4.9.

use crate::app::LauncherRoot;
use crate::layer::Layers;
use crate::shell::root::{ShellContext, ShellRoot};
use crate::theme::Theme;
use gpui::{
    Modifiers, PlatformInput, ScrollDelta, ScrollEvent, ScrollSource, TestAppContext, TouchPhase,
    VisualTestContext, point,
};
use hlsr_ui::fixtures::{FixtureServices, Scene, Variant};
use hlsr_ui::services::{BoxFuture, MainProcess, Reply, ServiceEvent, Services, StoreRead};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, broadcast};

fn fixtures(invoke: Value) -> Arc<FixtureServices> {
    let scene: Scene = serde_json::from_value(json!({
        "id": "t", "route": "/launcher", "platform": "win32",
        "window": {"width": 1600, "height": 900},
        "themes": ["hlsr"], "languages": ["ru"], "scales": [1],
        "now": "2026-11-17T12:00:00.000Z", "timezone": "UTC",
        "store": {"settings": {"config": {"language": 1, "uiScale": 1}}},
        "sendSync": {"app-version": "0.4.9", "app-default-library-path": "C:\\HLSR"},
        "invoke": invoke
    }))
    .unwrap();
    let variant = Variant {
        theme: "hlsr".into(),
        lang: "ru".into(),
        scale: 1.0,
        zoom: None,
        rail: None,
    };
    Arc::new(FixtureServices::new(&scene, &variant).unwrap())
}

fn base(extra: Value) -> Value {
    let mut invoke = json!({
        "get-library-path": {"path": "D:\\Games", "confirmed": true},
        // Вошёл через Steam: приветствие не открывается поверх.
        "runner-state": {"account": {"signedIn": true}},
        "validate-install-path": {"ok": true, "path": "D:\\Games", "hasCyrillic": false},
        "download-settings-set": {"ok": true},
        "discord-rpc-get": {"enabled": true, "state": "connected"}
    });
    if let (Value::Object(map), Value::Object(extra)) = (&mut invoke, extra) {
        map.extend(extra);
    }
    invoke
}

fn calls(fx: &FixtureServices, channel: &str) -> Vec<Value> {
    fx.log()
        .into_iter()
        .filter(|entry| entry["kind"] == "invoke" && entry["channel"] == channel)
        .map(|entry| entry["args"].clone())
        .collect()
}

fn settle(cx: &mut VisualTestContext, frames: usize) {
    for _ in 0..frames {
        cx.executor().advance_clock(Duration::from_millis(16));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }
}

fn click(cx: &mut VisualTestContext, name: &str) {
    settle(cx, 2);
    let at = crate::snap::bounds_of(name).unwrap_or_else(|| panic!("нет цели {name}"));
    cx.simulate_click(at.center(), Modifiers::none());
    settle(cx, 30);
}

/// Колесо над целью `name`: `lines` строк вниз.
fn wheel(cx: &mut VisualTestContext, name: &str, lines: f32) {
    let at = crate::snap::bounds_of(name).unwrap_or_else(|| panic!("нет цели {name}"));
    cx.update(|window, cx| {
        window.dispatch_event(
            PlatformInput::Scroll(ScrollEvent {
                position: at.center(),
                delta: ScrollDelta::Lines(point(0., -lines)),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
                source: ScrollSource::Pointer,
            }),
            cx,
        );
    });
    settle(cx, 40);
}

fn pref(cx: &mut VisualTestContext, key: &str) -> Value {
    cx.update(|_, cx| {
        let root = ShellRoot::get(cx).expect("оболочка");
        let prefs = root.read(cx).prefs();
        let value = prefs.borrow().get(key).cloned();
        value.unwrap_or(Value::Null)
    })
}

fn open(cx: &mut VisualTestContext, section: Option<&str>) {
    cx.update(|_, cx| {
        let flows = ShellContext::get(cx).expect("оболочка").flows.clone();
        match section {
            Some("diagnostics") => flows.open_diagnostics(),
            _ => flows.on_tool("settings"),
        }
    });
    settle(cx, 30);
}

fn settings_open(cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| {
        ShellContext::get(cx)
            .expect("оболочка")
            .flows
            .with_state(|s| s.settings_open)
    })
}

#[gpui::test]
fn save_keeps_the_window_open_and_theme_applies_at_once(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let fx = fixtures(base(json!({})));
    let services: Services = fx.clone();
    let (_, cx) = cx.add_window_view(|_, cx| LauncherRoot::launcher(services, cx));
    cx.run_until_parked();
    open(cx, None);
    assert_eq!(cx.update(|_, cx| Layers::open_count(cx)), 1);
    assert_eq!(calls(&fx, "discord-rpc-get"), [json!([])]);
    // «Интерфейс» (Windows: Общие, Загрузки, Свои моды, Интерфейс…): тема — сразу, без «Сохранить».
    click(cx, "settings-tabs-3");
    // «Бумага» — в третьем ряду сетки, ниже края раздела.
    wheel(cx, "settings-body", 6.);
    click(cx, "theme-paper");
    assert_eq!(pref(cx, "launcherTheme"), json!("paper"));
    assert_eq!(
        cx.update(|_, cx| cx.global::<Theme>().id().to_owned()),
        "paper"
    );
    wheel(cx, "settings-body", 6.);
    click(cx, "accent-sea");
    assert_eq!(pref(cx, "paperAccent"), json!("sea"));
    assert_eq!(
        cx.update(|_, cx| cx.global::<Theme>().accent().map(str::to_owned)),
        Some("sea".into())
    );
    wheel(cx, "settings-body", 20.);
    click(cx, "settings-fullscreenButton");
    assert!(settings_open(cx));
    // Оболочка при запуске уже отправила лимит (`initializeDownloadSettings`).
    let speed = calls(&fx, "download-settings-set").len();
    click(cx, "settings-save");
    assert_eq!(calls(&fx, "validate-install-path"), [json!(["D:\\Games"])]);
    let sent = calls(&fx, "download-settings-set");
    assert_eq!(sent.len(), speed + 1);
    assert_eq!(sent.last(), Some(&json!([0])));
    assert_eq!(
        calls(&fx, "tool-splits-set").last(),
        Some(&json!(["livesplit", {}]))
    );
    assert_eq!(
        calls(&fx, "tray-hide-while-playing-set").last(),
        Some(&json!([true]))
    );
    // Путь тот же — библиотеку не трогают; масштаб тот же — окно не меняют.
    assert!(calls(&fx, "set-library-path").is_empty());
    assert!(calls(&fx, "ui-scale-apply").is_empty());
    assert_eq!(pref(cx, "fullscreenButton"), json!(false));
    // «Сохранить» окно не закрывает (0.4.9).
    assert!(settings_open(cx));
    assert_eq!(cx.update(|_, cx| Layers::open_count(cx)), 1);
    // «Сбросить»: умолчания, но тема и акцент остаются.
    click(cx, "settings-reset");
    click(cx, "settings-save");
    assert_eq!(pref(cx, "fullscreenButton"), json!(true));
    assert_eq!(pref(cx, "launcherTheme"), json!("paper"));
    assert_eq!(pref(cx, "paperAccent"), json!("sea"));
    assert_eq!(
        calls(&fx, "validate-install-path").last(),
        Some(&json!(["C:\\HLSR"]))
    );
    cx.simulate_keystrokes("escape");
    settle(cx, 30);
    assert!(!settings_open(cx));
}

#[gpui::test]
fn bad_path_stops_saving_and_busy_library_is_reported(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let fx = fixtures(base(json!({
        "validate-install-path": {"ok": false, "error": "no-access"}
    })));
    let services: Services = fx.clone();
    let (_, cx) = cx.add_window_view(|_, cx| LauncherRoot::launcher(services, cx));
    cx.run_until_parked();
    open(cx, None);
    let speed = calls(&fx, "download-settings-set").len();
    click(cx, "settings-save");
    assert_eq!(calls(&fx, "validate-install-path"), [json!(["D:\\Games"])]);
    // Ошибка пути — вкладка «Загрузки», дальше сохранение не идёт.
    assert!(crate::snap::bounds_of("settings-path").is_some());
    assert_eq!(calls(&fx, "download-settings-set").len(), speed);
    assert!(settings_open(cx));
}

#[gpui::test]
fn new_folder_goes_to_the_library_and_language_closes_the_window(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let fx = fixtures(base(json!({
        "select-install-folder": "E:\\Runs",
        "validate-install-path": {"ok": true, "path": "E:\\Runs", "hasCyrillic": false},
        "set-library-path": {"ok": true, "path": "E:\\Runs"}
    })));
    let services: Services = fx.clone();
    let (_, cx) = cx.add_window_view(|_, cx| LauncherRoot::launcher(services, cx));
    cx.run_until_parked();
    open(cx, None);
    click(cx, "settings-language");
    // Щелчок открыл список и дал кнопке фокус: ↓↓ — «English», Enter выбирает.
    cx.simulate_keystrokes("down down enter");
    settle(cx, 30);
    click(cx, "settings-tabs-1");
    click(cx, "settings-browse");
    assert_eq!(calls(&fx, "select-install-folder"), [json!(["D:\\Games"])]);
    click(cx, "settings-save");
    assert_eq!(calls(&fx, "set-library-path"), [json!(["E:\\Runs"])]);
    let path = cx.update(|_, cx| {
        ShellContext::get(cx)
            .unwrap()
            .flows
            .with_state(|s| s.install_path.clone())
    });
    assert_eq!(path, "E:\\Runs");
    // Язык меняется последним, окно закрыто; язык — в settings.json и программе обновления.
    assert!(!settings_open(cx));
    assert_eq!(
        calls(&fx, "updater-set-language").last(),
        Some(&json!(["en"]))
    );
    let StoreRead::Found(settings) = fx.store_read("settings") else {
        panic!("нет settings.json");
    };
    assert_eq!(settings["config"]["language"], json!(0));
}

/// Фикстуры, но ответ `validate-install-path` ждёт `gate`: окно успевают закрыть.
struct Gated {
    fx: Arc<FixtureServices>,
    gate: Arc<Notify>,
}

impl MainProcess for Gated {
    fn invoke(&self, channel: &str, args: Vec<Value>) -> BoxFuture<'static, Reply> {
        let reply = self.fx.invoke(channel, args);
        if channel != "validate-install-path" {
            return reply;
        }
        let gate = self.gate.clone();
        Box::pin(async move {
            gate.notified().await;
            reply.await
        })
    }
    fn send(&self, channel: &str, args: Vec<Value>) {
        self.fx.send(channel, args)
    }
    fn send_sync(&self, channel: &str, args: Vec<Value>) -> Value {
        self.fx.send_sync(channel, args)
    }
    fn store_read(&self, name: &str) -> StoreRead {
        self.fx.store_read(name)
    }
    fn store_write(&self, name: &str, data: Value) -> bool {
        self.fx.store_write(name, data)
    }
    fn store_quarantine(&self, name: &str) -> Value {
        self.fx.store_quarantine(name)
    }
    fn subscribe(&self) -> broadcast::Receiver<ServiceEvent> {
        self.fx.subscribe()
    }
}

#[gpui::test]
fn closing_mid_save_finishes_the_steps_but_not_apply_prefs(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let fx = fixtures(base(json!({})));
    let gate = Arc::new(Notify::new());
    let services: Services = Arc::new(Gated {
        fx: fx.clone(),
        gate: gate.clone(),
    });
    let (_, cx) = cx.add_window_view(|_, cx| LauncherRoot::launcher(services, cx));
    cx.run_until_parked();
    open(cx, None);
    let speed = calls(&fx, "download-settings-set").len();
    let splits = calls(&fx, "tool-splits-set").len();
    click(cx, "settings-save");
    cx.simulate_keystrokes("escape");
    settle(cx, 30);
    assert!(!settings_open(cx));
    gate.notify_one();
    settle(cx, 30);
    // Как `submit` 0.4.9: лимит скорости сохранён, `$emit("apply")` снятого окна — ничто.
    assert_eq!(calls(&fx, "download-settings-set").len(), speed + 1);
    assert_eq!(calls(&fx, "tool-splits-set").len(), splits);
}

#[gpui::test]
fn diagnostics_run_and_export_like_0_4_9(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let fx = fixtures(base(json!({
        "diagnostics-run": {"runId": "r1", "checks": [
            {"id": "net-dns", "group": "network", "status": "ok"},
            {"id": "win-clock", "group": "windows", "status": "warn", "code": "CLOCK_SKEW"}
        ]},
        "game-crash-info": {"dumpPath": null},
        "export-diagnostics-zip": {"ok": true}
    })));
    let services: Services = fx.clone();
    let (_, cx) = cx.add_window_view(|_, cx| LauncherRoot::launcher(services, cx));
    cx.run_until_parked();
    open(cx, Some("diagnostics"));
    click(cx, "diag-run");
    assert_eq!(calls(&fx, "diagnostics-run"), [json!([])]);
    assert!(crate::snap::bounds_of("diag-details-win-clock").is_some());
    click(cx, "diag-export");
    assert_eq!(calls(&fx, "game-crash-info"), [json!([null])]);
    click(cx, "diag-export-go");
    assert_eq!(
        calls(&fx, "export-diagnostics-zip"),
        [json!([{"includeDump": false}])]
    );
}

/// HLSR-CE: «Свои моды» — список из `custom-mods-list`, «Играть» шлёт `custom-mods-launch`, мод
/// с проблемой не запускается.
#[gpui::test]
fn custom_mods_section_lists_and_launches(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let snapshot = json!({
        "ok": true,
        "mods": [
            {"id": "m1", "name": "Entropy : Zero 2", "modDir": "C:/sourcemods/EZ2",
             "exe": "C:/SDK/hl2.exe", "args": "-novid", "problem": null},
            {"id": "m2", "name": "Broken", "modDir": "C:/sourcemods/X",
             "exe": "", "args": "", "problem": "NO_ENGINE"}
        ],
        "engines": [{"exe": "C:/SDK/hl2.exe", "label": "Source SDK Base 2013 Singleplayer"}]
    });
    let fx = fixtures(base(json!({
        "custom-mods-list": snapshot,
        "custom-mods-launch": snapshot,
    })));
    let services: Services = fx.clone();
    let (_, cx) = cx.add_window_view(|_, cx| LauncherRoot::launcher(services, cx));
    cx.run_until_parked();
    // Колонка списка модов не читает: его читают библиотека модов и окно настроек.
    let at_start = calls(&fx, "custom-mods-list").len();
    open(cx, None);
    // И окно настроек — при открытии и на вкладке «Свои моды».
    assert_eq!(calls(&fx, "custom-mods-list").len(), at_start + 1);
    click(cx, "settings-tabs-2");
    assert!(calls(&fx, "custom-mods-list").len() >= at_start + 2);
    click(cx, "settings-mods-play-m1");
    assert_eq!(calls(&fx, "custom-mods-launch"), [json!(["m1"])]);
    // Без движка «Играть» выключена.
    click(cx, "settings-mods-play-m2");
    assert_eq!(calls(&fx, "custom-mods-launch").len(), 1);
    assert!(settings_open(cx));
}

/// HLSR-CE: «Библиотека модов» в колонке → плитки → страница мода (щелчок не запускает); галочка
/// параметра правит строку параметров; «Играть» — запуск.
#[gpui::test]
fn mod_library_tiles_open_the_mod_page(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let snapshot = json!({
        "ok": true,
        "mods": [{"id": "m1", "name": "Awakening", "modDir": "C:/sourcemods/Awakening",
                  "exe": "C:/SDK/hl2.exe", "args": "-novid -fullscreen", "problem": null,
                  "missingEpisodes": ["ep2"]}],
        "engines": [{"exe": "C:/SDK/hl2.exe", "label": "Source SDK Base 2013 Singleplayer"}]
    });
    let fx = fixtures(base(json!({
        "custom-mods-list": snapshot,
        "custom-mods-launch": snapshot,
        "custom-mods-set-args": snapshot,
    })));
    let services: Services = fx.clone();
    let (_, cx) = cx.add_window_view(|_, cx| LauncherRoot::launcher(services, cx));
    cx.run_until_parked();
    settle(cx, 10);
    let tab = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let nav = ShellContext::get(cx).expect("оболочка").nav.clone();
            (nav.active_tab(), nav.selected_mod())
        })
    };
    click(cx, "rail-mods");
    assert_eq!(tab(cx), ("mods".into(), String::new()));
    click(cx, "mods-tile-m1");
    assert_eq!(tab(cx), ("mods".into(), "m1".into()));
    // Щелчок только открывает страницу — мод не запускается.
    assert!(calls(&fx, "custom-mods-launch").is_empty());
    // «В окне» снимает «Полный экран» и сохраняется.
    click(cx, "mod-flag-windowed");
    assert_eq!(
        calls(&fx, "custom-mods-set-args").last(),
        Some(&json!(["m1", "-novid -windowed"]))
    );
    click(cx, "mod-play");
    assert_eq!(calls(&fx, "custom-mods-launch"), [json!(["m1"])]);
    // «Все моды» — назад в библиотеку.
    click(cx, "mod-back");
    assert_eq!(tab(cx), ("mods".into(), String::new()));
}
