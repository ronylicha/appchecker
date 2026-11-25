<div align="center">
  <img src="assets/logo.svg" alt="AppChecker logo" width="160">

  # AppChecker

  **A native Linux command-line tool and terminal dashboard for monitoring local Git repositories and dependency updates.**

  [![CI](https://github.com/ronylicha/appchecker/actions/workflows/ci.yml/badge.svg)](https://github.com/ronylicha/appchecker/actions/workflows/ci.yml)
  [![Latest release](https://img.shields.io/github/v/release/ronylicha/appchecker)](https://github.com/ronylicha/appchecker/releases/latest)
  [![MIT license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
  [![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
</div>

AppChecker is built for developers who maintain several projects at once. It centralizes Git status, discovers local repositories, checks dependency registries from its monitoring daemon, stores results in SQLite, and sends desktop notifications through D-Bus.

> **Status:** initial public release. AppChecker currently targets Linux. It checks dependency versions, not security vulnerabilities.

## Contents

- [Features](#features)
- [Supported ecosystems](#supported-ecosystems)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Commands](#commands)
- [Terminal dashboard](#terminal-dashboard)
- [Configuration](#configuration)
- [Data and privacy](#data-and-privacy)
- [Architecture](#architecture)
- [Development](#development)
- [Current limitations](#current-limitations)
- [Contributing](#contributing)
- [License](#license)

## Features

- **Multi-repository overview:** branch, dirty state, ahead/behind counts, and stash presence.
- **Project management:** add, remove, list, tag, filter, and pin projects.
- **Automatic discovery:** find Git repositories below configurable directories, with a dry-run mode.
- **Six package ecosystems:** parse local manifests and query their registries for newer versions.
- **Background monitoring:** periodic scans plus debounced filesystem events for Git metadata and dependency manifests.
- **Local persistence:** cache project, Git, and dependency state in SQLite.
- **Desktop notifications:** D-Bus notifications with severity filtering, quiet hours, batching, and per-project overrides.
- **Offline-friendly:** air-gap mode keeps local scans enabled while disabling registry requests.
- **Scriptable output:** one-time local checks can emit JSON.

## Supported ecosystems

| Ecosystem | Manifest files read | Default registry |
| --- | --- | --- |
| npm | `package.json` (`dependencies`, `devDependencies`) | `registry.npmjs.org` |
| Cargo | `Cargo.toml` (`dependencies`, `dev-dependencies`) | `crates.io` |
| Go | `go.mod` (`require`) | `proxy.golang.org` |
| pip | `pyproject.toml` (PEP 621) or `requirements.txt` | `pypi.org` |
| Composer | `composer.json` (`require`, `require-dev`) | `repo.packagist.org` |
| RubyGems | `Gemfile` | `rubygems.org` |

The daemon compares manifest versions with the latest registry versions and classifies updates using semantic versioning. It does **not** currently query CVE databases or perform vulnerability audits.

## Installation

### AppImage (x86_64)

Download `appchecker-0.1.0-x86_64.AppImage` from the [latest release](https://github.com/ronylicha/appchecker/releases/latest), then run:

```bash
chmod +x appchecker-0.1.0-x86_64.AppImage
./appchecker-0.1.0-x86_64.AppImage
```

### Debian / Ubuntu (amd64)

Download the `.deb` package from the [latest release](https://github.com/ronylicha/appchecker/releases/latest), then run:

```bash
sudo apt install ./appchecker_0.1.0-1_amd64.deb
appchecker --version
```

The package depends on Git and the D-Bus runtime library.

### Arch Linux

A [`PKGBUILD`](dist/omarchy/PKGBUILD) is included:

```bash
git clone https://github.com/ronylicha/appchecker.git
cd appchecker/dist/omarchy
makepkg -si
```

### Build from source

Prerequisites:

- Linux
- [Rust](https://rustup.rs/) 1.85 or newer
- Git
- A D-Bus notification daemon for desktop notifications

```bash
git clone https://github.com/ronylicha/appchecker.git
cd appchecker
cargo build --release --locked
sudo install -Dm755 target/release/appchecker /usr/local/bin/appchecker
```

For a user-local Cargo installation instead:

```bash
cargo install --path . --locked
```

Verify the installation:

```bash
appchecker --version
# appchecker 0.1.0
```

Release downloads can be checked against `SHA256SUMS`:

```bash
sha256sum -c SHA256SUMS
```

## Quick start

Add a project and inspect the configuration:

```bash
appchecker add ~/Projects/my-app --tags personal,rust --pin
appchecker list --detailed
appchecker config --path
```

Discover repositories without changing the configuration:

```bash
appchecker discover --paths ~/Projects --depth 3 --dry-run
```

Run the same discovery for real, then start the monitoring daemon:

```bash
appchecker discover --paths ~/Projects --depth 3
appchecker daemon --foreground --interval 300
```

Open the dashboard in another terminal:

```bash
appchecker
# Equivalent to: appchecker tui
```

The daemon must remain running to refresh registry data, persist scans, and send notifications. Stop it with `Ctrl+C`. Use your service manager if you want unattended background execution.

## Commands

Global options:

```text
-c, --config <CONFIG>  Use a specific TOML configuration file
-v, --verbose          Enable debug logging for AppChecker
-h, --help             Show help
-V, --version          Show the version
```

| Command | Purpose |
| --- | --- |
| `appchecker add <PATH>` | Add a project; supports `--name`, `--tags a,b`, and `--pin`. |
| `appchecker remove <NAME_OR_PATH>` | Stop monitoring a project. |
| `appchecker list` | List monitored projects; supports `--detailed` and `--tag`. |
| `appchecker check` | Run a local Git and manifest check; supports `--project` and `--format json`. |
| `appchecker discover` | Find new Git repositories; supports `--paths`, `--depth`, and `--dry-run`. |
| `appchecker daemon` | Monitor projects and query registries; supports `--foreground` and `--interval`. |
| `appchecker config` | Print the effective configuration; supports `--path` and `--reset`. |
| `appchecker tui` | Open the terminal dashboard. Running `appchecker` alone does the same. |

`appchecker check` is a local snapshot: registry update checks are performed by the daemon, not by this command.

Examples:

```bash
appchecker check --project my-app --format json
appchecker list --tag rust --detailed
appchecker --config ./config.toml daemon --foreground --interval 60
```

## Terminal dashboard

| Key | Action |
| --- | --- |
| `j` / `k` or `Down` / `Up` | Select the next / previous project |
| `g` / `G` or `Home` / `End` | Select the first / last project |
| `p` | Toggle the selected project's pin for the current session |
| `q` or `Esc` | Quit |

Indicators show dirty repositories (`*`), commits ahead (`↑N`) or behind (`↓N`), and outdated dependency counts. Project management is currently done through the CLI.

## Configuration

The default files are:

- Configuration: `~/.config/appchecker/config.toml`
- Database: `~/.local/share/appchecker/appchecker.db`

Prefer `appchecker add` and `appchecker discover` to manage project entries. The generated TOML can also be edited directly. A complete configuration looks like this:

```toml
projects = []
scan_interval_secs = 300
max_concurrent_scans = 8
air_gap_mode = false
auto_discover_paths = ["/home/you/Projects"]
max_auto_discover_depth = 3
file_descriptor_limit = 16384

[global_notification_prefs]
enabled = true
quiet_hours_start = "22:00"
quiet_hours_end = "08:00"
min_severity = "minor"
batch_interval_secs = 3600
show_minor_updates = false
show_major_updates = true
show_security = true

[registry_urls]
npm = "https://registry.npmjs.org"
crates_io = "https://crates.io/api/v1"
pypi = "https://pypi.org/pypi"
go_proxy = "https://proxy.golang.org"
packagist = "https://repo.packagist.org"
rubygems = "https://rubygems.org/api/v1"
```

Set `air_gap_mode = true` to disable all registry requests. Custom registry URLs can point to compatible mirrors or corporate proxies. Restart the daemon after editing its configuration.

Useful commands:

```bash
appchecker config          # Print effective TOML
appchecker config --path   # Print only the configuration path
appchecker config --reset  # Replace the configuration with defaults
```

## Data and privacy

AppChecker has no telemetry. Configuration and scan results stay on the local machine.

When the daemon runs with `air_gap_mode = false`, it sends dependency names to the configured package registries to retrieve version metadata. Project names, local paths, and Git history are not intentionally sent to those registries. Git ahead/behind detection invokes the local `git` executable and does not fetch from a remote.

## Architecture

```text
src/
├── cli/       # Clap command definitions
├── config/    # TOML loading, saving, and repository discovery
├── core/      # Configuration, project, dependency, and error models
├── deps/      # Manifest parsers and asynchronous registry client
├── git/       # Git repository scanning
├── notify/    # Notification filtering, batching, and D-Bus delivery
├── persist/   # SQLite migrations and threaded writes
├── tui/       # Ratatui dashboard and terminal event handling
└── watcher/   # Debounced filesystem monitoring
```

Runtime flow:

```text
CLI / filesystem events / periodic timer
                 │
                 ▼
       Git scanner + manifest parsers
                 │
                 ▼
        Registry client (unless air-gap)
                 │
          ┌──────┴──────┐
          ▼             ▼
     SQLite store   Notification router
          │             │
          ▼             ▼
    Ratatui dashboard  D-Bus notifications
```

Main dependencies include Tokio, Ratatui, Crossterm, gix, rusqlite, reqwest with Rustls, notify, and Clap.

## Development

Clone and build:

```bash
git clone https://github.com/ronylicha/appchecker.git
cd appchecker
cargo build --locked
```

Run the standard checks:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features
cargo test --locked
cargo build --release --locked
```

The integration tests cover all six dependency parsers and initialization of the SQLite store in a fresh data directory. CI runs the same checks on Ubuntu.

## Current limitations

- Only Linux is supported in this release; Windows and macOS are not packaged or verified.
- The daemon runs in the foreground; no systemd user unit is installed yet.
- The TUI is primarily an overview. Adding, removing, and discovering projects is done through the CLI.
- `appchecker check` does not contact registries; run the daemon to populate update information.
- Version checks are not a substitute for `cargo audit`, `npm audit`, osv-scanner, or another vulnerability scanner.
- Prebuilt binaries are provided for x86_64/amd64 only.

## Contributing

Bug reports, feature requests, documentation improvements, and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) and follow the [Code of Conduct](CODE_OF_CONDUCT.md).

Please use [private vulnerability reporting](https://github.com/ronylicha/appchecker/security/advisories/new) for security issues instead of public issues; see [SECURITY.md](SECURITY.md).

Changes are recorded in [CHANGELOG.md](CHANGELOG.md), and releases follow [Semantic Versioning](https://semver.org/).

## License

AppChecker is distributed under the [MIT License](LICENSE). Copyright (c) 2026 ronylicha.
