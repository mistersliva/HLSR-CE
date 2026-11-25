//! Каналы игр с открытым ключом Ed25519 и QA-переопределения (`channel-config.js`).

use crate::delivery_endpoints::game::channel_url;
use crate::environment::Environment;
use crate::feature_flag::is_local_http_allowed;
use url::Url;

pub const QA_URL_ENV: &str = "HLSR_GAME_DELIVERY_CHANNEL_URL";
pub const QA_KEY_ENV: &str = "HLSR_GAME_DELIVERY_CHANNEL_KEY";
pub const QA_CHANNEL_ENV: &str = "HLSR_GAME_DELIVERY_CHANNEL_NAME";

pub const QA_ALLOWED_HTTPS_HOSTS: &[&str] = &["hlsr.pro"];
const QA_LOCAL_HOSTS: &[&str] = &["127.0.0.1", "localhost"];
const QA_CHANNEL_NAMES: &[&str] = &["stable", "beta", "test"];

/// Отладочные возможности (QA-переопределение канала, локальный HTTP) есть только в
/// отладочной сборке и в выпуске с `dev-hooks`.
const DEBUG_FEATURES: bool = cfg!(any(debug_assertions, feature = "dev-hooks"));

/// `isProductionBuild()`: `NODE_ENV === "production"` в 0.4.7 — здесь сборка выпуска без
/// `dev-hooks` (спецификация, решение 6).
pub const fn is_production_build() -> bool {
    !DEBUG_FEATURES
}

/// Открытый ключ канала (`keyId` 7ccb676761a3ed1c) — один на все три игры.
pub const GHOSTING_STABLE_PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAhx8U4Ih2sh7K2JgtHVQiqEzYaoRTsL/KEBEYiPYX854=\n-----END PUBLIC KEY-----\n";

const KEY_IDS: &[&str] = &["7ccb676761a3ed1c"];

/// Игры со встроенным каналом, в порядке `CHANNELS`.
pub const GAME_IDS: [&str; 3] = ["ghosting-renewed", "hl2-4260", "hl2-2707"];

/// Запись `CHANNELS[gameId]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinChannel {
    pub url: String,
    pub channel: &'static str,
    pub require_signature: bool,
    pub public_keys: &'static [&'static str],
    pub key_ids: &'static [&'static str],
    pub release_profile_enabled: bool,
}

/// `CHANNELS[gameId]`; игры без встроенного канала — `None`.
pub fn builtin_channel(game_id: &str) -> Option<BuiltinChannel> {
    GAME_IDS.contains(&game_id).then(|| BuiltinChannel {
        url: channel_url(game_id),
        channel: "stable",
        require_signature: true,
        public_keys: &[GHOSTING_STABLE_PUBLIC_KEY],
        key_ids: KEY_IDS,
        release_profile_enabled: true,
    })
}

/// Вид QA-адреса: https на hlsr.pro или http на локальной машине.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QaUrlKind {
    Https,
    LocalHttp,
}

impl QaUrlKind {
    pub fn as_str(self) -> &'static str {
        match self {
            QaUrlKind::Https => "https",
            QaUrlKind::LocalHttp => "local-http",
        }
    }
}

/// `qaUrlKind(url, env)`. Адрес разбирается как `new URL()`; `http` годится только
/// для `127.0.0.1` и `localhost` и только если разрешён локальный HTTP.
pub fn qa_url_kind(url: &str, env: &Environment) -> Option<QaUrlKind> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    match parsed.scheme() {
        "https" => QA_ALLOWED_HTTPS_HOSTS
            .contains(&host)
            .then_some(QaUrlKind::Https),
        "http" => (is_local_http_allowed(env) && QA_LOCAL_HOSTS.contains(&host))
            .then_some(QaUrlKind::LocalHttp),
        _ => None,
    }
}

/// `hasActiveReleaseProfile()`: в production есть игра со встроенным каналом и ключом.
pub fn has_active_release_profile(env: &Environment) -> bool {
    env.builtin_channel_enabled()
        && GAME_IDS.iter().any(|game_id| {
            builtin_channel(game_id).is_some_and(|base| {
                base.release_profile_enabled && !base.url.is_empty() && !base.public_keys.is_empty()
            })
        })
}

/// Откуда взят адрес канала.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Builtin,
    Qa,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Builtin => "builtin",
            Source::Qa => "qa",
        }
    }
}

/// Итог `resolve`: куда ходить за каналом и чьим ключам верить.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelConfig {
    pub game_id: String,
    pub url: Option<String>,
    pub channel: String,
    pub require_signature: bool,
    pub public_keys: Vec<String>,
    pub source: Option<Source>,
}

/// `resolve(gameId, env)`. Вне production и только при отладочных возможностях
/// `HLSR_GAME_DELIVERY_CHANNEL_URL` подменяет адрес, `..._NAME` — имя канала
/// (`stable`, `beta`, `test`), `..._KEY` — ключ (`\n` в нём — перевод строки).
pub fn resolve(game_id: &str, env: &Environment) -> Option<ChannelConfig> {
    let base = builtin_channel(game_id)?;

    let builtin_active = !base.url.is_empty()
        && base.release_profile_enabled
        && env.builtin_channel_enabled()
        && !base.public_keys.is_empty();

    let mut config = ChannelConfig {
        game_id: game_id.to_owned(),
        url: builtin_active.then(|| base.url.clone()),
        channel: base.channel.to_owned(),
        require_signature: base.require_signature,
        public_keys: base
            .public_keys
            .iter()
            .map(|key| (*key).to_owned())
            .collect(),
        source: builtin_active.then_some(Source::Builtin),
    };

    if DEBUG_FEATURES
        && !env.is_production()
        && let Some(qa_url) = env.var(QA_URL_ENV).filter(|url| !url.is_empty())
        && qa_url_kind(&qa_url, env).is_some()
    {
        config.url = Some(qa_url);
        config.source = Some(Source::Qa);

        if let Some(name) = env.var(QA_CHANNEL_ENV)
            && QA_CHANNEL_NAMES.contains(&name.as_str())
        {
            config.channel = name;
        }
        if let Some(key) = env.var(QA_KEY_ENV).filter(|key| !key.is_empty()) {
            config.public_keys = vec![key.replace("\\n", "\n")];
        }
    }

    Some(config)
}

/// Итог `describe`: без самих ключей и адресов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description {
    pub game_id: String,
    pub configured: bool,
    pub source: Option<Source>,
    pub require_signature: bool,
    pub trusted_keys: usize,
}

pub fn describe(game_id: &str, env: &Environment) -> Description {
    match resolve(game_id, env) {
        Some(config) => Description {
            game_id: game_id.to_owned(),
            configured: config.url.is_some(),
            source: config.source,
            require_signature: config.require_signature,
            trusted_keys: config.public_keys.len(),
        },
        None => Description {
            game_id: game_id.to_owned(),
            configured: false,
            source: None,
            require_signature: true,
            trusted_keys: 0,
        },
    }
}
