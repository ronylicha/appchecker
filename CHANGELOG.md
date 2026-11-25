# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.0]: https://github.com/ronylicha/appchecker/releases/tag/v0.1.0
