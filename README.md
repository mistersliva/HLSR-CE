<div align="center">

<img src="icons/hlsrc.png" width="128" alt="HLSR logo">

<h1>HLSR-CE</h1>

**HLSR Community Edition — a community fork of [HLSR](https://github.com/HL-HLSR/HLSR), a modern and the most convenient launcher for Half-Life speedrun builds.**

What CE adds on top of HLSR:

- **Uncap 60 FPS** — *Settings → Interface*: the launcher interface draws at your monitor refresh rate (144, 240 Hz and up) instead of being capped at 60 frames per second.
- **Custom Source mods** — *Mod library* in the left column: your mods as tiles, each with its own page (engine, launch options with quick checkboxes like `-windowed` and `-noborder`, Play). Add a mod folder, find everything in `steamapps/sourcemods`, or install a mod from a zip/7z archive. The engine can be Source SDK Base 2013, Half-Life 2, HL2 Ghosting downloaded by the launcher or your own `hl2.exe`; if a mod needs Episode One/Two files the engine lacks, the launcher warns you (Windows).
- **A secret** — the launcher remembers its roots. Hint: type a certain word on the keyboard anywhere outside text fields.

[![release](https://img.shields.io/github/v/release/HL-HLSR/HLSR-Releases?label=release&color=7c3aed)](https://github.com/HL-HLSR/HLSR-Releases/releases/latest)
[![downloads](https://img.shields.io/github/downloads/HL-HLSR/HLSR-Releases/total?label=downloads&color=7c3aed)](https://github.com/HL-HLSR/HLSR-Releases/releases)
[![rust](https://img.shields.io/badge/Rust-1.98-ea580c)](#build)
[![ui](https://img.shields.io/badge/UI-GPUI-0f766e)](https://github.com/zed-industries/zed/tree/main/crates/gpui)
[![platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-0078d4)](#requirements)
[![license](https://img.shields.io/badge/license-GPL--3.0-7c3aed)](LICENSE)

### [Download the latest release](https://github.com/HL-HLSR/HLSR-Releases/releases/latest)

An installer for Windows and an AppImage for Linux. After that the launcher updates itself.

[About](#about) • [Features](#features) • [Official and your own](#official) • [Requirements](#requirements) • [Building](#build) • [Contributors](#contributors) • [License](#license)

<sub>Русская версия: [README.ru.md](README.ru.md)</sub>

</div>

<a id="about"></a>
## 🔵 What is HLSR?

HLSR stands for **H**alf-**L**ife **S**peed**R**un.
It puts every Half-Life game you can speedrun into one launcher!
Right now that is Half-Life (WON), Half-Life 2 (builds 2707 and 4260) and Ghosting Renewed.
You need Steam and your own copy of the Valve game.

Since 0.5 the launcher is rewritten from Electron to Rust, and the UI runs on
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), the same
framework the Zed editor uses. One codebase builds for Windows and Linux. The
old Electron launcher lives in the
[`electron`](https://github.com/HL-HLSR/HLSR/tree/electron) branch, just for history.

<a id="features"></a>
## 🔵 What can it do?

- Updates itself from hlsr.pro: on Windows through our own HLSR Updater, on Linux from signed archives
- On Linux it finds your installed Proton, and on first start it downloads a verified one
- The first Proton download comes with a notification, updates install from a button in the notification or in settings. The previous build stays for old prefixes and manual rollback
- On Linux the game and LiveSplit run through umu in one Proton prefix
- Configs, game settings, launch options and so on
- Diagnostics that find critical errors
- Rendering demos to video right from the launcher
- Exporting your profile with all your stats
- Showing your game status in Discord
- Russian and English

And more.

<a id="official"></a>
## 🔵 The official launcher and your own

Official builds come out only in
[HLSR-Releases](https://github.com/HL-HLSR/HLSR-Releases/releases) and on
[hlsr.pro](https://hlsr.pro). Only @prolert can sign a build, and the launcher
checks that signature on every update. Our keys are not in this repository,
and you don't need them to build or test: the tests use test keys.

Online features (Steam sign-in, profile linking, records, friends) work only in
the official launcher. The hlsr.pro server is closed source and not part of
this repository. It checks that the build is ours and keeps launchers built
from source out of online.

Want your own launcher? Run your own server, your own update channel and your
own mirrors for game builds. The addresses and public keys live here:

- `crates/hlsr-online/src/telemetry/config.rs` - the API address;
- `crates/hlsr-delivery/src/delivery_endpoints.rs` - mirrors and channels;
- `crates/hlsr-delivery/src/channel_config.rs` - the game channel key;
- `crates/hlsr-update/src/channel.rs` - the launcher update channel key.

Give your build a different name and drop the HLSR logo, see [COPYING.md](COPYING.md).

<a id="requirements"></a>
## 🔵 Requirements

Any OS:

- Rust `1.98.0` through [rustup](https://rustup.rs): the version, components and targets are pinned in `rust-toolchain.toml`, rustup installs them itself;
- [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) to check dependency licenses and advisories;
- Git;
- Node.js from `.nvmrc`, only for the release packer and its tests (`tools/release`, `tools/compat-047`).

Windows:

- Windows 10 or Windows 11 x64;
- Visual Studio Build Tools (MSVC and the Windows SDK). Release builds need `fxc.exe` from the SDK for GPUI shaders;
- run `git config --global core.autocrlf false` before checkout, fixtures are compared byte for byte.

Linux:

- Linux x86_64 with glibc 2.27 or newer, X11 or Wayland, Vulkan and a GPU driver;
- to build: `pkg-config`, `libfontconfig1-dev`, `libfreetype-dev`, `libxkbcommon-dev`, `libxkbcommon-x11-dev` and `libasound2-dev` (or the same packages under other names in your distribution);
- Python 3.10+ is used if you already have it. If not, HLSR downloads a verified Python into its own folder, without sudo;
- Podman for release builds on Ubuntu 18.04.

<a id="build"></a>
## 🔵 Building

Run it for development. Separate folders for data and logs keep your installed
HLSR settings untouched:

```bash
HLSR_DATA_DIR=/tmp/hlsr-data HLSR_LOGS_DIR=/tmp/hlsr-logs cargo run -p hlsr-app
```

Before a pull request, run the checks:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

Release builds. None of these commands publish anything:

```bash
scripts/slint/build-windows.sh --package   # Windows, Git Bash: target/dist/windows/HLSR.exe and the installer
scripts/slint/build-linux.sh --release     # Linux: Ubuntu 18.04 in Podman, target/dist/linux/
```

## 🗂️ Project structure

```text
crates/hlsr-app/       the hlsr binary: startup, window, tray, install and update
crates/hlsr-gpui/      the GPUI interface: screens, theme, text, animation
crates/hlsr-ui/        UI logic without a framework, locales, themes, images
crates/hlsr-core/      paths, settings, logging, launch modes
crates/hlsr-delivery/  downloading, installing, patching and repairing games, configs
crates/hlsr-platform/  launching games, Steam, Proton and umu, LiveSplit
crates/hlsr-online/    online: profile, sign-in, Discord, telemetry
crates/hlsr-update/    installing and updating the launcher itself
crates/hlsr-demo/      the "Demo" tool
tools/release/         packing the installer, archive and AppImage
tools/compat-047/      checks of the old 0.4.7–0.4.9 updaters
vendor/                GPUI for Windows with our patches
config/, resources/    game manifests, 7-Zip, LiveSplit
```

<a id="contributors"></a>
## 🔵 Contributors

- @InfiniteC0re - created HLSR, maintained the launcher from 2019 to 2023;
- @prolert - the new CEO;
- @kanoyo-git - ported the launcher to Linux and built a lot of its features;
- @ScriptedSnark - this is Bogdan, and for now he just exists.

Want to help? [CONTRIBUTING.md](CONTRIBUTING.md) explains how to send a fix.
Found a vulnerability? Report it privately as described in [SECURITY.md](SECURITY.md), not in an issue.

<a id="license"></a>
## 🔵 License

The code is licensed under the [GNU GPL version 3](LICENSE). The HLSR name and
logo are not covered by it, and neither are the pictures from Valve games: see
the list of files in [COPYING.md](COPYING.md). Fonts, 7-Zip, LiveSplit and the
dependencies keep their own licenses, listed in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

HLSR is an unofficial fan project and is not affiliated with Valve Corporation.
Half-Life, Half-Life 2, Steam and Source are trademarks of Valve Corporation.
