//! Лаунчер HLSR: выбор режима по аргументам и запуск окна.

// Сборка выпуска на Windows — без окна консоли, как HLSR.exe 0.4.7.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// HLSR-CE: свои Source-моды (каналы `custom-mods-*`).
mod custom_mods;
mod demo;
mod dialogs;
// Живой инспектор окна (`HLSR_LIVE_PORT`, scripts/gpui/live.py).
#[cfg(feature = "dev-hooks")]
mod inspect;
// Живой прогон каналов по сценарию (V2).
#[cfg(feature = "dev-hooks")]
mod live;
mod modes;
mod online;
mod platform;
mod reduced_motion;
mod services;
// Сцены сверочного стенда (`hlsr-snap screens`).
#[cfg(feature = "dev-hooks")]
mod snap;
mod startup;
mod tray;
mod update;
mod visibility;
mod window;
mod window_ops;
// Рабочая область Linux (`_NET_WORKAREA`); на Windows её даёт GPUI (`rcWork`).
#[cfg_attr(windows, allow(dead_code))]
mod work_area;

use hlsr_core::args::{LaunchMode, ModeEnv, parse_args_with};
use hlsr_core::error::exit;
use hlsr_core::logging;
use hlsr_core::notifications::add_startup_error;
use hlsr_core::paths::Paths;
use std::panic::AssertUnwindSafe;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const NOTIFICATIONS_FILE: &str = "notifications.json";

/// Признаки режимов установщика: окружение процесса и нагрузка установщика в конце своего exe
/// (Р1, `HLSR-Setup-<v>.exe`).
fn mode_env() -> ModeEnv {
    let mut env = ModeEnv::from_process();
    env.setup_payload = cfg!(windows)
        && std::env::current_exe()
            .is_ok_and(|exe| hlsr_update::windows::payload::has_trailer(&exe));
    env
}

/// Имя приложения (`app.getName()` 0.4.7): от него папки данных и журналов. Windows — по списку
/// файлов своей сборки (нагрузка установщика или `resources\install-files.json` рядом с exe):
/// QA-сборка (`hlsr-release setup --qa`) — `hlsr-qa`, как QA-сборка 0.4.7 с
/// `extraMetadata.name=hlsr-qa`. Linux — `hlsr`: QA-сборки Linux в 0.4.7 нет.
fn app_name() -> &'static str {
    if !cfg!(windows) {
        return hlsr_core::paths::APP_NAME;
    }
    std::env::current_exe().map_or(hlsr_core::paths::APP_NAME, |exe| {
        hlsr_update::windows::setup::app_name_of(&exe)
    })
}

fn main() {
    let started = Instant::now();
    let args = parse_args_with(std::env::args_os().skip(1), &mode_env());
    let paths = match Paths::detect(app_name()) {
        Ok(paths) => paths,
        Err(e) if args.mode == LaunchMode::Launcher => {
            dialogs::startup_error(&e.to_string());
            std::process::exit(exit::STARTUP_ERROR);
        }
        Err(e) => {
            eprintln!("каталог данных не определён: {e}");
            std::process::exit(exit::STARTUP_ERROR);
        }
    };
    if let Some(problem) = modes::redirect_problem(args.mode, &|name| std::env::var(name).ok()) {
        eprintln!("{problem}");
        std::process::exit(exit::STARTUP_ERROR);
    }
    // Внешний апдейтер включался в `entry.js` раньше журнала лаунчера: у него свой `updater.log`.
    if args.mode == LaunchMode::Updater {
        std::process::exit(modes::run(&args, &paths));
    }
    let _log = logging::init(&paths)
        .inspect_err(|e| eprintln!("журнал не подключён: {e}"))
        .ok();
    logging::install_panic_hook();
    platform::prepare_exe_dir();

    if args.mode != LaunchMode::Launcher {
        tracing::info!(target: "launcher", "режим {:?}", args.mode);
        std::process::exit(modes::run(&args, &paths));
    }

    let result = std::panic::catch_unwind(AssertUnwindSafe(|| startup::run(args, &paths, started)));
    let code = match result {
        Ok(Ok(code)) => code,
        Ok(Err(e)) => fail(&paths, &e.to_string()),
        // Паника до показа окна — как ошибка запуска; после — обычное падение.
        Err(payload) if !window::window_shown() => {
            fail(&paths, &format!("паника: {}", panic_text(payload.as_ref())))
        }
        Err(payload) => std::panic::resume_unwind(payload),
    };
    std::process::exit(code);
}

/// `reportStartupError` 0.4.7: журнал, `notifications.json`, окно ошибки.
fn fail(paths: &Paths, message: &str) -> i32 {
    tracing::error!("Startup error: {message}");
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Err(e) = add_startup_error(&paths.file(NOTIFICATIONS_FILE), message, now_ms) {
        tracing::warn!("[notifications] STARTUP_ERROR не записан: {e}");
    }
    dialogs::startup_error(message);
    exit::STARTUP_ERROR
}

/// Текст паники (`&str` или `String`).
fn panic_text(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(не строка)")
}
