<div align="center">

<img src="icons/hlsrc.png" width="128" alt="Логотип HLSR">

<h1>HLSR-CE</h1>

**HLSR Community Edition — форк [HLSR](https://github.com/HL-HLSR/HLSR) от сообщества, современного и самого удобного лаунчера со сборками Half-Life.**

Что добавляет CE поверх HLSR:

- **Снять ограничение 60 FPS** — *Настройки → Интерфейс*: интерфейс лаунчера рисуется с частотой вашего монитора (144, 240 Гц и выше), а не 60 кадров в секунду.
- **Свои Source-моды** — *Библиотека модов* в левой колонке: моды плитками, у каждого своя страница (движок, параметры запуска с галочками вроде `-windowed` и `-noborder`, «Играть»). Добавьте папку мода, найдите всё из `steamapps/sourcemods` или поставьте мод из архива zip/7z. Движок — Source SDK Base 2013, Half-Life 2, скачанный лаунчером HL2 Ghosting или свой `hl2.exe`; если моду нужны файлы Episode One/Two, которых у движка нет, лаунчер предупредит (Windows).
- **Секрет** — лаунчер помнит, с чего начинался. Подсказка: наберите одно слово на клавиатуре в любом месте вне полей ввода.

[![release](https://img.shields.io/github/v/release/HL-HLSR/HLSR-Releases?label=release&color=7c3aed)](https://github.com/HL-HLSR/HLSR-Releases/releases/latest)
[![downloads](https://img.shields.io/github/downloads/HL-HLSR/HLSR-Releases/total?label=downloads&color=7c3aed)](https://github.com/HL-HLSR/HLSR-Releases/releases)
[![rust](https://img.shields.io/badge/Rust-1.98-ea580c)](#build)
[![ui](https://img.shields.io/badge/UI-GPUI-0f766e)](https://github.com/zed-industries/zed/tree/main/crates/gpui)
[![platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-0078d4)](#requirements)
[![license](https://img.shields.io/badge/license-GPL--3.0-7c3aed)](LICENSE)

### [Скачать последнюю версию](https://github.com/HL-HLSR/HLSR-Releases/releases/latest)

Установщик для Windows и AppImage для Linux. Дальше лаунчер обновляется сам.

[Что это](#about) • [Возможности](#features) • [Официальный и свой лаунчер](#official) • [Что понадобится](#requirements) • [Сборка](#build) • [Участники](#contributors) • [Лицензия](#license)

<sub>English: [README.md](README.md)</sub>

</div>

<a id="about"></a>
## 🔵 Что за HLSR?

HLSR расшифровывается как **H**alf-**L**ife **S**peed**R**un.
Он собирает все игры Half-Life, которые можно спидранить, в одном лаунчере!
Сейчас это Half-Life (WON), Half-Life 2 (сборки 2707 и 4260) и Ghosting Renewed.
Для игр нужен Steam и купленная игра Valve.

С версии 0.5 лаунчер переписан с Electron на Rust, а интерфейс сделан на
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), том самом,
на котором работает редактор Zed. Один код собирается под Windows и под Linux.
Старый лаунчер на Electron лежит в ветке
[`electron`](https://github.com/HL-HLSR/HLSR/tree/electron), просто для истории.

<a id="features"></a>
## 🔵 Что он умеет?

- Сам обновляется с hlsr.pro: в Windows через наш фирменный HLSR Updater, в Linux из подписанных архивов
- В Linux сам находит установленный Proton, а при первом запуске скачивает проверенный
- Первая загрузка Proton приходит с уведомлением, обновления ставятся кнопкой в уведомлении или в настройках. Прежняя сборка остаётся для старых префиксов и ручного отката
- В Linux игра и LiveSplit запускаются через umu в одном Proton-префиксе
- Настройка конфигов, игры, параметров запуска и т.д.
- Диагностика, которая находит критические ошибки
- Рендер демок в видео прямо из лаунчера
- Экспорт профиля со всей статистикой
- Показывает игровой статус в Discord
- Русский и английский язык

И т.д.

<a id="official"></a>
## 🔵 Официальный лаунчер и свой

Официальные сборки выходят только в
[HLSR-Releases](https://github.com/HL-HLSR/HLSR-Releases/releases) и на
[hlsr.pro](https://hlsr.pro). Подписать сборку может только @prolert, и
лаунчер проверяет эту подпись при каждом обновлении. Наших ключей в
репозитории нет, для сборки и тестов они не нужны: тесты работают на
тестовых ключах.

Онлайн (вход через Steam, привязка профиля, рекорды, друзья) работает только
в официальном лаунчере. Сервер hlsr.pro закрытый и в этот репозиторий не
входит. Он проверяет, что сборка наша, а лаунчер, собранный из исходников,
в онлайн не пускает.

Хотите свой лаунчер? Поднимайте свой сервер, свой канал обновлений и свои
зеркала для сборок игр. Адреса и открытые ключи лежат здесь:

- `crates/hlsr-online/src/telemetry/config.rs` - адрес API;
- `crates/hlsr-delivery/src/delivery_endpoints.rs` - зеркала и каналы;
- `crates/hlsr-delivery/src/channel_config.rs` - ключ канала игр;
- `crates/hlsr-update/src/channel.rs` - ключ канала обновлений лаунчера.

Свою сборку назовите иначе и уберите логотип HLSR, подробнее в [COPYING.md](COPYING.md).

<a id="requirements"></a>
## 🔵 Что понадобится?

Для любой ОС:

- Rust `1.98.0` через [rustup](https://rustup.rs): версия, компоненты и цели закреплены в `rust-toolchain.toml`, rustup поставит их сам;
- [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) для проверки лицензий и уязвимостей зависимостей;
- Git;
- Node.js из `.nvmrc`, только для упаковщика выпуска и его тестов (`tools/release`, `tools/compat-047`).

Windows:

- Windows 10 или Windows 11 x64;
- Visual Studio Build Tools (MSVC и Windows SDK). Сборке выпуска нужен `fxc.exe` из SDK для шейдеров GPUI;
- перед checkout выполните `git config --global core.autocrlf false`, фикстуры сравниваются байт в байт.

Linux:

- Linux x86_64 с glibc 2.27 или новее, X11 или Wayland, Vulkan и драйвер видеокарты;
- для сборки: `pkg-config`, `libfontconfig1-dev`, `libfreetype-dev`, `libxkbcommon-dev`, `libxkbcommon-x11-dev` и `libasound2-dev` (или то же под другими именами в вашем дистрибутиве);
- Python 3.10+ используется, если он уже стоит. Если нет, HLSR сам скачает проверенный Python в свой каталог, без sudo;
- Podman для сборки выпуска на Ubuntu 18.04.

<a id="build"></a>
## 🔵 Сборка

Запуск для разработки. Свои временные папки для данных и журналов нужны, чтобы
не задеть настройки установленного HLSR:

```bash
HLSR_DATA_DIR=/tmp/hlsr-data HLSR_LOGS_DIR=/tmp/hlsr-logs cargo run -p hlsr-app
```

Перед pull request прогоните проверки:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

Сборка выпуска. Ни одна из этих команд ничего не публикует:

```bash
scripts/slint/build-windows.sh --package   # Windows, Git Bash: target/dist/windows/HLSR.exe и установщик
scripts/slint/build-linux.sh --release     # Linux: Ubuntu 18.04 в Podman, target/dist/linux/
```

## 🗂️ Структура проекта

```text
crates/hlsr-app/       бинарник hlsr: запуск, окно, трей, установка и обновление
crates/hlsr-gpui/      интерфейс на GPUI: экраны, тема, текст, анимации
crates/hlsr-ui/        логика интерфейса без фреймворка, локали, темы, картинки
crates/hlsr-core/      пути, настройки, журнал, режимы запуска
crates/hlsr-delivery/  скачивание, установка, патчи и починка игр, конфиги
crates/hlsr-platform/  запуск игр, Steam, Proton и umu, LiveSplit
crates/hlsr-online/    онлайн: профиль, вход, Discord, телеметрия
crates/hlsr-update/    установка и обновление самого лаунчера
crates/hlsr-demo/      инструмент «Демо»
tools/release/         упаковка установщика, архива и AppImage
tools/compat-047/      проверки старых обновлялок 0.4.7–0.4.9
vendor/                GPUI для Windows с нашими правками
config/, resources/    манифесты игр, 7-Zip, LiveSplit
```

<a id="contributors"></a>
## 🔵 Участники

- @InfiniteC0re - создал HLSR, поддерживал лаунчер с 2019 по 2023;
- @prolert - новый CEO;
- @kanoyo-git - перенёс лаунчер на Linux и сделал многие фишки для лаунчера;
- @ScriptedSnark - а это богдан, и он пока просто есть.

Хотите помочь? Как прислать исправление, написано в [CONTRIBUTING.md](CONTRIBUTING.md).
Нашли уязвимость? Пишите закрыто, как сказано в [SECURITY.md](SECURITY.md), а не в issue.

<a id="license"></a>
## 🔵 Лицензия

Код распространяется по [GNU GPL версии 3](LICENSE). Название и логотип HLSR
под неё не попадают, как и картинки из игр Valve: список файлов в
[COPYING.md](COPYING.md). Шрифты, 7-Zip, LiveSplit и зависимости остаются под
своими лицензиями, они перечислены в [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

HLSR — неофициальный фанатский проект, он не связан с Valve Corporation.
Half-Life, Half-Life 2, Steam и Source — товарные знаки Valve Corporation.
