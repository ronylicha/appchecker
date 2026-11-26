# AppChecker

**Native Linux desktop and terminal app for monitoring Git repositories and safely updating dependencies.**

AppChecker keeps track of your development projects in one legible native interface. Monitor Git status, detect outdated dependencies across six ecosystems, review updates at the scope you choose, and optionally analyze breaking changes with Claude Code or Codex — without giving up control of your files. The existing terminal dashboard remains available for headless and SSH workflows.

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Language](https://img.shields.io/badge/language-Rust-orange.svg)
![Platform](https://img.shields.io/badge/platform-Linux-green.svg)

## ✨ Features

- **Native desktop interface** — Responsive light/dark UI, real buttons, accessible modals, mouse and keyboard navigation, and a native folder picker
- **Real OSV vulnerability audit** — Virtualized dependency table with current/latest versions, update level, CVE/GHSA identifiers and severity for the exact installed version, plus explicit unavailable/not-checked states
- **Scoped safe updates** — Update one dependency, one project, or every project with separate actions, clean-Git checks, per-project locks, external backups, cancellation, and conditional rollback
- **Reviewed AI remediation** — Official Claude Code/Codex OAuth flows, explicit file consent, bounded redacted context, structured findings, reviewed diffs, dedicated branches, and post-update checks
- **Multi-repo dashboard** — Add, edit, remove, pin, search, filter, and discover local Git repositories
- **6 ecosystem support** — npm, Cargo, pip, Go modules, Composer, RubyGems
- **Smart notifications** — Severity-based D-Bus alerts with quiet hours, batching, and per-project overrides
- **Hybrid dependency checking** — Local manifest parsing + event-driven registry polling (not wasteful full scans)
- **Reactive filesystem watching** — Debounced inotify events trigger instant rescans on changes
- **Air-gap mode** — Full offline operation with configurable registry URLs for corporate environments
- **Persistent cache** — SQLite-backed stale-while-revalidate for instant startup
- **Auto-discovery** — Scan directories to find projects automatically
- **Keyboard-driven TUI** — Fast navigation with vim-style bindings
- **Visual modals** — Add/edit/remove projects, configure settings, view details — all within the TUI

## 📦 Installation

### From source (recommended)

```bash
# Prerequisites: Rust 1.95+, Git, and a native Linux graphics stack
# Debian/Ubuntu build dependencies:
sudo apt install libdbus-1-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libx11-dev libgl1-mesa-dev libegl1-mesa-dev
git clone https://github.com/ronylicha/appchecker.git
cd appchecker
cargo build --release --locked
sudo cp target/release/appchecker /usr/local/bin/
```

### Debian/Ubuntu (.deb)

```bash
wget https://github.com/ronylicha/appchecker/releases/download/v0.3.0/appchecker_0.3.0-1_amd64.deb
sudo apt install ./appchecker_0.3.0-1_amd64.deb
```

### AppImage (portable, no install)

```bash
wget https://github.com/ronylicha/appchecker/releases/download/v0.3.0/appchecker-0.3.0-x86_64.AppImage
chmod +x appchecker-0.3.0-x86_64.AppImage
./appchecker-0.3.0-x86_64.AppImage
```

### Omarchy / Arch Linux (PKGBUILD)

```bash
git clone https://github.com/ronylicha/appchecker.git
cd appchecker/dist/omarchy
makepkg -si
```

## 🚀 Quick Start

```bash
# Launch the native desktop interface (also the default)
appchecker
appchecker gui

# Or use the terminal dashboard
appchecker tui

# Add a project first from the CLI if preferred
appchecker add ~/Projects/my-app
appchecker gui

# One-time check without TUI
appchecker check

# Auto-discover projects in ~/Projects
appchecker discover

# Start background daemon with notifications (detaches and returns)
appchecker daemon

# Keep the daemon attached to the terminal for logs or debugging
appchecker daemon --foreground
```

## 🖥️ Native desktop workflows

- **Projects**: add a repository through the native directory picker, edit its metadata, pin it, remove its cache, search by name/tag, or discover repositories automatically.
- **Updates**: select a project, inspect current/latest versions and registry or manifest errors, filter to outdated packages, then update one dependency or the whole project. A separate global action reviews every project together.
- **Confirmation**: every update lists the owning project, target version and exact operation. Execution rechecks the expected Git HEAD and a clean worktree, locks each project, revalidates every package, and stores manifest/lockfile backups outside the repository. Global updates continue after individual failures.
- **Activity**: inspect sanitized session-only output, including cancellation, partial failures and recovery. No raw AI prompt, source context, patch, or provider token is persisted by default.
- **Accessibility**: light and dark themes, keyboard focus, native modal dialogs, visible disabled/busy states, high-contrast text and controls, responsive cards, and virtualized long dependency/activity lists.

The GUI uses the system graphics stack (Wayland or X11). The AppImage defaults to the OpenGL wgpu backend; set `WGPU_BACKEND` to override it. On a headless terminal, use `appchecker tui`.

### Optional AI remediation

Install the official `claude`, `codex` and/or `agy` executable, then choose **Connect with OAuth** in AI remediation. AppChecker opens `claude auth login`, `codex login`, or Antigravity's interactive sign-in in a real terminal and checks `claude auth status --text`, `codex login status`, or `agy models`. It never reads, displays, or stores provider tokens.

Use the **Model** selector to choose the provider default or a model from the installed CLI: latest aliases reported by `claude --help`, the visible API catalog from `codex debug models`, or the full dynamic catalog from `agy models`. Antigravity additionally requires an explicit per-session acknowledgement: Google does not support third-party headless use on consumer plans; use a supported Enterprise/API route or accept responsibility for that run.

Choose the exact source files, review the consent, and only then run analysis. Files are size-bounded, redacted and copied to a disposable directory. Air-gap mode disables provider calls. The provider can return structured findings and a patch but cannot execute commands. Authentication or subscription errors are shown verbatim; AppChecker never fabricates an analysis.

Before applying a patch, review the complete diff. AppChecker rejects absolute paths, traversal, `.git`, symlinks, submodules and files outside the approved set. It verifies the expected HEAD, clean worktree and exact reviewed-diff hash, applies on a dedicated branch, and runs only ecosystem-specific checks that exist. Rollback is available only while the tree still exactly matches the applied patch; later edits are never overwritten with `reset --hard`.

## ⌨️ Keyboard Shortcuts

Native GUI: `Ctrl+1` Projects, `Ctrl+2` Updates, `Ctrl+3` AI remediation, `Ctrl+4` Activity, `Ctrl+N` new project. `Tab`/`Shift+Tab` navigates controls and `Esc` closes the active modal.

Terminal UI:

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `g` / `Home` | Go to top |
| `G` / `End` | Go to bottom |
| `Enter` | Show project dependencies |
| `a` | Add project |
| `e` | Edit project |
| `d` | Delete project |
| `f` | Find/discover projects |
| `p` | Pin/unpin project |
| `r` | Refresh selected project |
| `R` | Refresh all projects |
| `u` | Update selected dependency |
| `U` | Update all outdated dependencies |
| `c` | Open configuration |
| `1` / `2` / `3` | Switch view tab |
| `?` / `h` | Show help |
| `q` | Quit |
| `Esc` | Close the modal or return to Projects |
| `Ctrl+C` | Force quit |

## 🔔 Notification System

AppChecker uses a severity-tiered notification system to prevent alert fatigue:

| Severity | Behavior |
|----------|----------|
| **Security** | Immediate D-Bus notification, bypasses batching |
| **Major** | Included in hourly digest |
| **Minor** | Included in daily digest (if enabled) |
| **Info** | Logged only, no notification |

The `c` key edits scan interval, registry concurrency, and air-gap mode. Configure quiet hours, batch intervals, and per-project notification overrides directly in `~/.config/appchecker/config.toml`.

## ⚙️ Configuration

Config file: `~/.config/appchecker/config.toml`

```toml
projects = []
scan_interval_secs = 300        # Auto-refresh interval
max_concurrent_scans = 8        # Parallel scan workers (1-256)
air_gap_mode = false            # Disable registry polling and AI provider calls
trusted_repositories = []       # Project UUIDs allowed to run lifecycle scripts
auto_discover_paths = ["~/Projects"]
max_auto_discover_depth = 3
file_descriptor_limit = 16384

[global_notification_prefs]
enabled = true
quiet_hours_start = "22:00"
quiet_hours_end = "08:00"
min_severity = "minor"
batch_interval_secs = 3600
show_security = true
show_major_updates = true
show_minor_updates = false

[registry_urls]
npm = "https://registry.npmjs.org"
crates_io = "https://crates.io/api/v1"
pypi = "https://pypi.org/pypi"
go_proxy = "https://proxy.golang.org"
packagist = "https://repo.packagist.org"
rubygems = "https://rubygems.org/api/v1"
osv = "https://api.osv.dev"
```

## Project management

Add, list, and remove monitored repositories from the CLI:

```bash
appchecker add ~/Projects/my-app --name "My app" --tags rust,cli --pin
appchecker list --detailed
appchecker remove my-app
```

The same operations are available in the TUI: `a` adds, `e` edits, `d` removes, `f` discovers, and `p` pins a project. Tabs, project and dependency rows, forms, buttons, and dialogs are usable with the keyboard or the mouse. `j`/`k` and the wheel scroll; `g`/`G` jump to the first/last item. Long paths remain visible through left truncation.

## Dependency checks and updates

`appchecker check` queries configured registries unless air-gap mode is enabled. JSON output contains `project`, `branch`, `dirty`, `total_deps`, `outdated_deps`, `registry_errors`, `non_public_skipped`, `air_gap_mode`, `git_error`, `registry_error_details`, and `manifest_errors`. Logs go to stderr, leaving stdout machine-readable.

In the TUI, select a dependency and press `u`, or press `U` to review all outdated dependencies for the selected project. Every operation is shown before confirmation and runs sequentially. The Activity view records sanitized, bounded output.

| Ecosystem | Confirmed remediation |
| --- | --- |
| npm | `npm install <package>@<version> --ignore-scripts` with `--save-dev` when needed |
| Cargo | `cargo add <package>@<version>` with `--dev` and aliases preserved |
| Go | `go get <module>@v<version>` |
| pip | Atomic update of `pyproject.toml` or `requirements.txt` |
| Composer | `composer require <package>:<version> --with-all-dependencies --no-interaction --no-scripts` |
| RubyGems | Update the `Gemfile` constraint, then `bundle update <package>` |

Native update execution requires the corresponding package manager in `PATH`. Python updates edit and verify the manifest rather than changing the active virtual environment. Compound ranges that cannot be rewritten safely remain marked `RANGE` and are not offered as updates.

## Data and privacy

AppChecker has no telemetry. Configuration and scan results stay local. Dependency names are sent only to configured registries when air-gap mode is disabled. Local paths, direct URLs, workspaces, private registries, and non-public sources are skipped and shown as `private/local`, never as fully up to date. The same distinction appears as `non_public_skipped` in JSON.

Registry concurrency is limited to 1–256 checks. AppChecker respects project and user configuration for npm/Yarn, Cargo, Go, pip/uv, Composer, and Bundler, and fails closed when private-source configuration cannot be read safely.

When air-gap mode is disabled, AppChecker sends public dependency names, ecosystems and exact installed versions to the configured registries and to OSV. “No known CVE” means OSV returned no entry for that exact version; it is not a guarantee that the package is safe. OSV failures remain visible as “CVE unavailable”. Air-gap mode makes no OSV request.

AI providers receive only the files explicitly approved for one analysis. Secrets are redacted before provider invocation, and temporary analysis directories are deleted afterward. Provider authentication remains in the official CLI's own credential store. Reviewed patch backups live outside the repository with restrictive permissions.

## 🏗️ Architecture

```
src/
├── core/       # Models (Project, GitState, DepInfo, AppConfig) + error types
├── git/        # gix-based repo scanner with spawn_blocking async wrapper
├── deps/       # 6-ecosystem manifest parser + hybrid registry client
├── notify/     # Severity-tiered D-Bus notification router
├── persist/    # Thread-safe SQLite store (mpsc channel writer thread)
├── watcher/    # Debounced filesystem watcher (notify crate)
├── config/     # TOML config loader + auto-discovery
├── cli/        # Clap CLI commands
├── tui/        # Ratatui dashboard, modals, state management, theming
├── desktop/    # Native eframe/egui app, rfd pickers, safe updates and AI review
└── main.rs     # Entry point, command dispatch, daemon loop
```

Key design decisions:
- **Rust + eframe/egui + rfd** native desktop UI, with **ratatui + gix + tokio** retained for terminal and async workflows
- **Thread-safe SQLite** via dedicated writer thread (Send+Sync for tokio::spawn)
- **Event-driven deps checking** with bounded concurrency (Arc<Semaphore>)
- **Stale-while-revalidate** caching for instant UI startup
- **Git ahead/behind** via `git rev-list` CLI for cross-version compatibility

## 🧪 Testing

```bash
cargo test --locked                 # All tests
cargo test --test deps_parser_test  # Integration tests only
cargo clippy --all-targets --all-features
```

Integration tests cover all 6 dependency parsers against fixture manifests in `tests/fixtures/`.

## 📄 License

MIT — see [LICENSE](LICENSE).

## 🤝 Contributing

Contributions welcome! Please open an issue or PR on [GitHub](https://github.com/ronylicha/appchecker).