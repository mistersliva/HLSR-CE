//! Окружение, от которого зависят флаги и адреса канала: переменные и признак
//! «production». В 0.4.7 это `process.env` и `process.env.NODE_ENV === "production"`;
//! здесь «production» — сборка выпуска без `dev-hooks` (спецификация, решение 6).

use crate::channel_config::is_production_build;
use std::collections::HashMap;

#[derive(Debug, Clone)]
enum Variables {
    /// `process.env`: переменные читаются при обращении, а не копируются.
    Process,
    /// Заданный набор — аргумент `env` функций 0.4.7.
    Fixed(HashMap<String, String>),
}

#[derive(Debug, Clone)]
pub struct Environment {
    variables: Variables,
    production: bool,
    /// HLSR-CE: встроенный канал загрузок включён и в отладочной сборке (`cargo run`).
    builtin_channel: bool,
}

impl Environment {
    /// Переменные процесса и признак этой сборки.
    pub fn process() -> Environment {
        Environment {
            variables: Variables::Process,
            production: is_production_build(),
            // HLSR-CE: игры качаются из канала в любой сборке, не только в сборке выпуска.
            builtin_channel: true,
        }
    }

    /// Заданный набор переменных и явный признак «production»: так проверяются обе ветки
    /// в отладочной сборке. Признак влияет на результат только в сторону ограничений:
    /// отладочные возможности (QA-канал, локальный HTTP) вне `debug_assertions` и
    /// `dev-hooks` не компилируются, что бы ни было передано.
    pub fn new<K: Into<String>, V: Into<String>>(
        variables: impl IntoIterator<Item = (K, V)>,
        production: bool,
    ) -> Environment {
        Environment {
            variables: Variables::Fixed(
                variables
                    .into_iter()
                    .map(|(name, value)| (name.into(), value.into()))
                    .collect(),
            ),
            production,
            builtin_channel: production,
        }
    }

    pub fn is_production(&self) -> bool {
        self.production
    }

    /// HLSR-CE: встроенный канал загрузок включён — в production и в окружении процесса
    /// ([`Environment::process`]) любой сборки; в заданном наборе — как production.
    pub fn builtin_channel_enabled(&self) -> bool {
        self.production || self.builtin_channel
    }

    /// Значение переменной; не заданной — `None`. Не UTF-8 в переменных процесса не читается.
    pub fn var(&self, name: &str) -> Option<String> {
        match &self.variables {
            Variables::Process => std::env::var(name).ok(),
            Variables::Fixed(variables) => variables.get(name).cloned(),
        }
    }
}
