# Changelog

All notable changes to Dicturs are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/). `scripts/bump-version.sh` turns the
"Unreleased" section into the new version.

## [Unreleased]

## [0.1.2] - 2026-10-30

Fix the website deploy workflow

### Added

- Voice commands: say a keyword, run a shell command. Offline speech recognition with
  whisper.cpp (or any external engine), fuzzy matching, optional wake word.
- Commands in nested categories, each command or category can be turned off without deleting it.
- `{args}` / `{args_url}`: the words said after the keyword, restricted to letters and digits.
- Built-in voice commands: stop listening, change model, change language, dry run.
- Commands run in your shell (bash, zsh, fish, nushell…) with your aliases.
- Recognition language from the system locale; sample commands in English or French.
- Three binaries on the same config: `dicturs` (CLI), `dicturs-tui` (terminal UI with mouse
  support) and `dicturs-gui` (egui desktop app following the system dark/light mode).
- Launcher integration (wofi, rofi, fuzzel, tofi, bemenu, walker, dmenu, fzf) with `dicturs menu`.
- Whisper model catalog with download, switch and removal.
- Install script, release builds for Linux, macOS and Windows, website on GitHub Pages.

[Unreleased]: https://github.com/cyrill62/dicturs/compare/0.1.2...HEAD
[0.1.2]: https://github.com/cyrill62/dicturs/releases/tag/0.1.2
