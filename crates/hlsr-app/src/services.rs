//! Сервисы окна в приложении: каналы окна — фасады `DeliveryApi`, `PlatformApi`, `OnlineApi`,
//! `UpdateApi`, `DemoApi` (`demo.rs`) и каналы, на которые в 0.4.9 отвечал сам `AppManager` ([`AppChannels`]:
//! `ui-scale-apply`, `window-command`, `clipboard-write-text`, `open-logs-folder`); `sendSync` —
//! версия и папка библиотеки по умолчанию (preload 0.4.7); хранилище окна — `library.json` и
//! `settings.json` папки данных; события фасадов — в общий канал окна (`support-thanks` ждёт
//! `support-thanks-ready` окна, как `pending-event.js` 0.4.9).

use crate::online::Online;
use crate::platform::{self, Platform};
use hlsr_core::paths::Paths;
use hlsr_delivery::api::DeliveryEvent;
use hlsr_delivery::library_path::default_library_path;
use hlsr_delivery::ports::Shell;
use hlsr_delivery::providers::BoxFuture;
use hlsr_ui::services::router::ChannelRouter;
use hlsr_ui::services::{ChannelService, Reply, ServiceEvent};
use hlsr_update::api::UpdateApi;
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::broadcast;

/// Журнал лаунчера (`log.transports.file.getFile()`), его пишет `hlsr_core::logging`.
const LOG_FILE: &str = "main.log";

/// Каналы окна, на которые отвечал `AppManager.js` 0.4.7 (`:534-551`, `:787-791`).
pub struct AppChannels {
    logs_dir: PathBuf,
    shell: Arc<dyn Shell>,
}

impl AppChannels {
    pub fn new(logs_dir: PathBuf, shell: Arc<dyn Shell>) -> AppChannels {
        AppChannels { logs_dir, shell }
    }

    /// `ui-scale-apply`: масштаб сохранило окно (`settings.json`), здесь он только применяется к
    /// окну (`applyUiScale`): `{status: "ok", scale}` — масштаб, который поместился. Новый размер —
    /// из обычного окна: полный экран кнопки снимается, у плеера «Демо» — `FULLSCREEN`.
    async fn ui_scale_apply(&self, value: Option<&Value>) -> Value {
        let scale = hlsr_core::window_size::normalize_ui_scale(value.unwrap_or(&Value::Null));
        if hlsr_gpui::bridge::call(crate::window_ops::leave_for_ui_scale).await == Some(false) {
            return json!({"status": "error", "code": "FULLSCREEN"});
        }
        let applied = hlsr_gpui::bridge::call(move |cx| crate::window::apply_ui_scale(cx, scale));
        match applied.await.flatten() {
            Some(applied) => {
                json!({"status": "ok", "scale": hlsr_core::json_text::js_number(applied)})
            }
            None => {
                tracing::warn!("[ui-scale] не применён: окна нет");
                json!({"status": "error", "code": "APPLY_FAILED"})
            }
        }
    }

    /// `clipboard-write-text`: только ответ и журнал; текст в буфер кладёт вид
    /// (`cx.write_to_clipboard`, `hlsr_ui::clipboard`), канал отвечает, как main 0.4.7, с тем же пределом.
    fn clipboard_write_text(value: Option<&Value>) -> Value {
        let text = match value {
            Some(value) if hlsr_core::js::truthy(value) => hlsr_core::js::to_string(value),
            _ => String::new(),
        };
        if hlsr_ui::clipboard::is_too_large(&text) {
            return json!({"status": "error", "code": "TOO_LARGE"});
        }
        json!({"status": "ok"})
    }

    /// `open-logs-folder`: `main.log` в проводнике; ответ — папка журналов.
    fn open_logs_folder(&self) -> Value {
        self.shell
            .show_item_in_folder(&self.logs_dir.join(LOG_FILE));
        Value::String(self.logs_dir.to_string_lossy().into_owned())
    }
}

impl ChannelService for AppChannels {
    fn call<'a>(&'a self, channel: &'a str, args: &'a [Value]) -> BoxFuture<'a, Option<Reply>> {
        Box::pin(async move {
            let reply = match channel {
                "ui-scale-apply" => self.ui_scale_apply(args.first()).await,
                "window-command" => {
                    let command = args.first().cloned();
                    hlsr_gpui::bridge::call(move |cx| {
                        crate::window_ops::window_command(cx, command.as_ref())
                    })
                    .await
                    .unwrap_or_else(|| json!({"ok": false, "code": "NO_WINDOW"}))
                }
                "clipboard-write-text" => Self::clipboard_write_text(args.first()),
                "open-logs-folder" => self.open_logs_folder(),
                _ => return None,
            };
            Some(Ok(reply))
        })
    }
}

/// Сервисы окна на фасадах запущенного лаунчера. Вызывать в среде tokio.
pub fn start(
    runtime: &Handle,
    paths: &Paths,
    platform: &Platform,
    online: &Online,
    update: &Arc<UpdateApi>,
) -> Arc<ChannelRouter> {
    let version = env!("CARGO_PKG_VERSION");
    let demo = crate::demo::start(runtime, paths, platform, online);
    // HLSR-CE: свои Source-моды.
    let custom_mods = Arc::new(crate::custom_mods::CustomMods::new(
        &paths.data_dir,
        platform.delivery().clone(),
        platform::app_shell(),
        platform::app_dialogs(),
    ));
    let channels: Vec<Arc<dyn ChannelService>> = vec![
        platform.delivery().clone(),
        platform.api().clone(),
        online.api().clone(),
        update.clone(),
        Arc::new(crate::demo::DemoChannels(demo.clone())),
        Arc::new(AppChannels::new(
            paths.logs_dir.clone(),
            platform::app_shell(),
        )),
        custom_mods.clone(),
    ];
    let mut sync = Map::new();
    sync.insert("app-version".into(), json!(version));
    sync.insert(
        "app-default-library-path".into(),
        Value::String(default_library_path().to_string_lossy().into_owned()),
    );
    let router = Arc::new(
        ChannelRouter::new(runtime.clone(), channels, sync)
            .with_store(paths.data_dir.clone())
            .with_pending("support-thanks", "support-thanks-ready")
            .with_build({
                let online = online.api().clone();
                move || online.standing()
            }),
    );

    forward(runtime, platform.delivery().subscribe(), router.clone());
    forward(runtime, platform.api().subscribe(), router.clone());
    forward(runtime, demo.subscribe(), router.clone());
    forward(runtime, custom_mods.subscribe(), router.clone());
    forward_update(runtime, update.subscribe(), router.clone());
    let mut online_events = online.api().subscribe();
    let events = router.clone();
    runtime.spawn(async move {
        while let Some(event) = online_events.recv().await {
            events.emit(ServiceEvent {
                channel: event.channel.to_owned(),
                payload: event.payload,
            });
        }
    });
    router
}

/// События фасада обновления (`updater-state`, `diagnostics-progress`) — в канал окна.
fn forward_update(
    runtime: &Handle,
    mut source: broadcast::Receiver<hlsr_update::api::UpdateEvent>,
    events: Arc<ChannelRouter>,
) {
    runtime.spawn(async move {
        loop {
            match source.recv().await {
                Ok(event) => {
                    events.emit(ServiceEvent {
                        channel: event.channel.to_owned(),
                        payload: event.payload,
                    });
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("[services] окно отстало от событий обновления: {skipped}");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// События фасада доставки или платформы — в канал окна.
fn forward(
    runtime: &Handle,
    mut source: broadcast::Receiver<DeliveryEvent>,
    events: Arc<ChannelRouter>,
) {
    runtime.spawn(async move {
        loop {
            match source.recv().await {
                Ok(event) => {
                    // Отчёт о сбое игры: окно — вперёд, чтобы игрок увидел его и прислал лог.
                    if event.channel == "game-crash-report" {
                        hlsr_gpui::bridge::invoke(crate::window_ops::bring_forward);
                    }
                    events.emit(ServiceEvent {
                        channel: event.channel.to_owned(),
                        payload: event.payload,
                    });
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("[services] окно отстало от событий фасада: {skipped}");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use hlsr_ui::clipboard::MAX_TEXT_UNITS;

    #[test]
    fn clipboard_channel_answers_like_main_047() {
        assert_eq!(
            AppChannels::clipboard_write_text(Some(&json!("demo hlsr"))),
            json!({"status": "ok"})
        );
        // `String(value || "")`: пусто и `null` — пустой текст, тоже «ok».
        assert_eq!(
            AppChannels::clipboard_write_text(Some(&Value::Null)),
            json!({"status": "ok"})
        );
        assert_eq!(
            AppChannels::clipboard_write_text(None),
            json!({"status": "ok"})
        );
        let huge = "я".repeat(MAX_TEXT_UNITS + 1);
        assert_eq!(
            AppChannels::clipboard_write_text(Some(&json!(huge))),
            json!({"status": "error", "code": "TOO_LARGE"})
        );
        let edge = "x".repeat(MAX_TEXT_UNITS);
        assert_eq!(
            AppChannels::clipboard_write_text(Some(&json!(edge))),
            json!({"status": "ok"})
        );
    }
}
