# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-11-25

### Added

- Complete interactive project management: add, edit, remove, pin, and discover repositories from the TUI with persisted changes.
- Mouse support for tabs, lists, forms, toggles, action buttons, confirmations, and scrolling.
- Modern Projects, Dependencies, and Activity views with loading, empty, error, range, current, and update states.
- Confirmed dependency updates executed sequentially from the interface with exact commands and an activity journal.
- Native remediation for npm, Cargo, Go, Composer, and RubyGems, plus format-preserving, verified updates for Python manifests.
- Development dependency tracking for npm, Cargo, Composer, and Gem, including development groups in Gemfiles.
- Detailed registry error reporting and clean CLI JSON output with a `registry_errors` field.
- Automatic TUI scans, periodic refresh, and protection against update/scan races.
- Tests for project workflows, mouse interaction, rendering, update execution, CLI behavior, registry errors, and SQLite cache consistency.

### Changed

- `appchecker check` now performs real registry checks instead of reporting only local manifest snapshots.
- CLI and TUI project operations share the same validation, duplicate detection, canonical paths, sorting, and persistence rules.
- Dependency scans replace cached snapshots so removed dependencies no longer remain visible.
- Version comparison supports partial versions and common range prefixes without treating unresolved wildcards as mandatory updates.

### Fixed

- Enable SQLite foreign keys and remove a project's cached dependencies when it is deleted.
- Create the SQLite data directory before first use and migrate existing databases with development dependency metadata.
- Preserve comments in `requirements.txt` and correctly fall back from an empty `pyproject.toml`.
- Prevent updates while a project scan is active and rescan after completed updates.
- Restore terminal state correctly when leaving the mouse-enabled interface.
- Keep the SQLite writer alive after cloned stores are dropped and wait for pending writes during shutdown.
- Support project path edits and stale-ID replacement with transactional upserts, ordered deletion, and SQLite busy timeouts.
- Discard stale scans after project path or configuration changes and share one concurrency limit across all registry checks.
- Correct modal cancel hitboxes, update-list scrolling, project sidebar layout, Activity direction, and dependency selection after rescans.
- Prevent unchecked, air-gapped, failed, or unparseable dependencies from being reported as healthy.
- Reject local paths, direct URLs, workspaces, credentials, and other non-public dependency references before registry requests.
- Handle UTF-8 renames, staged same-length edits, nested untracked files, and Git status failures without panicking or reporting clean repositories.
- Verify Python updates by ecosystem, roll back failed manifest changes, reject complex requirements safely, and select the correct manifest in polyglot and non-public projects.
- Update Gemfile constraints before `bundle update`, support exact-version syntax, and preserve Cargo dependency aliases.
- Escape uppercase Go module paths and preserve registry metadata during daemon reactive scans and empty manifests.
- Recover from filesystem watcher lag, config tilde paths, deleted project directories, ambiguous names, and overlapping discovery roots.
- Avoid truncating non-ASCII dependency names mid-character and guarantee update completion events after worker failures.
- Drain large package-manager outputs without pipe deadlocks and cancel between packages as well as before each operation.
- Parse Go `replace` directives in two passes, including block and versioned replacements, and block private Ruby source blocks.
- Separate Git, registry, and manifest errors in CLI and TUI output, and suppress terminal log writes while the TUI is active.
- Close package-manager stdin, disable Composer prompts, and cancel process groups without requiring Python or leaking terminal output.
- Respect scoped Gem sources and block private Composer object repositories and pip secondary indexes before public registry requests.
- Preserve inline comments during Gem and Python updates and keep normal, development, and aliased Cargo dependencies distinct in SQLite.
- Bound registry concurrency, keep air-gap errors visible, recover scans after failed project removal, and align long update confirmations with mouse targets.
- Sanitize and bound multi-line package-manager output in Activity, and align long repository discovery paths with mouse selection.
- Block private pip options and pyproject indexes, validate Ruby source hosts and nested scopes, and normalize Cargo replacement identities before registry requests.
- Log daemon registry failures and avoid reporting unresolved version ranges as fully up to date.
- Migrate legacy caches containing orphaned rows without preventing application startup.
- Respect private `.npmrc` registries, `tool.uv.sources`, multiline Gem declarations, and additive Gem sources.
- Preserve compound Python and Gem constraints as non-actionable ranges instead of offering updates that cannot run.
- Honor the configured daemon interval and keep action buttons clickable on 80-column terminals.
- Preserve project selection after discovery and remove Activity scroll debt.
- Mark private and local-only dependencies as skipped in CLI JSON and the TUI instead of reporting them as fully up to date.
- Handle uv list sources and offline settings, Cargo source replacement, and custom Ruby source options without public registry leaks.
- Show actionable update counts before range notices and clamp zero-second TUI refresh intervals.
- Mark local, editable, URL-based, and included pip requirements as skipped, and apply uv overrides consistently during updates.
- Honor `GOPRIVATE`, `GONOPROXY`, `GOPROXY`, and Go workspaces without leaking private module names.
- Validate Composer Packagist overrides, normalize Cargo replacement names, and preserve Ruby array options.
- Cancel repository discovery on exit and avoid waiting indefinitely for blocking scans during runtime shutdown.
- Read Go environment files and block forms, support standard `GOPROXY` fallbacks, and accept Git worktrees and configured discovery depths.
- Respect Yarn Berry and global npm registry configuration, standalone `uv.toml`, and custom Ruby source blocks.
- Keep long project paths and cursors visible in forms on 80-column terminals.
- Merge global and project npm configuration and honor Cargo, pip, and uv user-level source settings without public registry leaks.
- Reset dependency selection on project Home/End and keep Git errors visible in the overview.
- Accept partial configuration files with safe defaults and correct README installation and control instructions.
- Detect parent Go workspaces, global Composer repositories, pip INI variants, and Bundler mirrors.
- Preserve exact Gem constraint operators during updates, including spaceless `>=` forms.
- Honor Cargo registry overrides, `GOWORK=auto`, npm user configuration, Go environment files, and system pip/uv settings.
- Detach the daemon into a real background worker and deduplicate notifications while honoring visibility preferences and digest batching.
- Allow ordinary Cargo configuration files, preserve project npm precedence, and forward `--interval` to detached workers.

## [0.1.0] - 2026-11-25

### Added

- Native Linux terminal dashboard for monitoring multiple Git repositories.
- Project registration, removal, filtering, tagging, pinning, and automatic discovery.
- Git branch, worktree, upstream, and stash status scanning.
- Dependency manifest parsing for npm, Cargo, Go, pip, Composer, and RubyGems projects.
- Asynchronous registry update checks with configurable concurrency and registry URLs.
- Background monitoring with debounced filesystem events and periodic scans.
- SQLite persistence for projects, Git state, and dependency status.
- Desktop notifications through D-Bus with severity filters, quiet hours, batching, and per-project overrides.
- Air-gap mode for local-only monitoring without registry requests.
- JSON output for one-time local project checks.
- AppImage, Debian package, and Arch Linux PKGBUILD packaging.
- Continuous integration for formatting, linting, tests, and release builds.

### Fixed

- Create missing parent directories before opening the SQLite database, allowing the daemon to start with a fresh data directory.
- Forward command-line arguments correctly through the AppImage launcher.

[0.2.0]: https://github.com/ronylicha/appchecker/releases/tag/v0.2.0
[0.1.0]: https://github.com/ronylicha/appchecker/releases/tag/v0.1.0
