# Contributing to AppChecker

Thank you for helping improve AppChecker. Contributions of code, documentation, packaging, tests, and bug reports are welcome.

By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Report a bug

Open a [GitHub issue](https://github.com/ronylicha/appchecker/issues/new/choose) and include:

- AppChecker version (`appchecker --version`)
- Linux distribution and architecture
- Steps to reproduce the problem
- Expected and actual behavior
- Relevant logs, with personal paths or other private information removed

Do not post credentials, private repository contents, or security vulnerabilities in a public issue. Use [private vulnerability reporting](https://github.com/ronylicha/appchecker/security/advisories/new) for security reports.

## Propose a change

For a substantial feature or architectural change, open an issue first to discuss the scope and approach. Small fixes can go directly to a pull request.

1. Fork the repository and clone your fork.
2. Create a branch from `main`.
3. Make a focused change.
4. Add or update tests when behavior changes.
5. Update the README, changelog, or other documentation affected by the change.
6. Run all checks locally.
7. Open a pull request with a clear description and verification steps.

## Development setup

Requirements: Linux, Rust 1.85 or newer, and Git. A D-Bus notification daemon is needed to test desktop notifications.

```bash
git clone https://github.com/ronylicha/appchecker.git
cd appchecker
cargo build --locked
```

To exercise the CLI without changing your normal configuration:

```bash
mkdir -p target/tmp/dev-home
appchecker --config target/tmp/dev-home/config.toml add /path/to/a/git/repository
XDG_DATA_HOME="$PWD/target/tmp/dev-home/data" \
  cargo run -- --config target/tmp/dev-home/config.toml daemon --foreground
```

Use `air_gap_mode = true` in that temporary configuration to avoid registry requests during local development.

## Required checks

Run these commands before opening a pull request:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features
cargo test --locked
cargo build --release --locked
```

Format code with `cargo fmt` if needed. Add regression coverage for bug fixes and parser fixtures for newly supported manifest variants.

## Style and commits

- Keep changes focused and consistent with the existing Rust modules.
- Prefer small, testable functions and explicit error propagation over hidden failures.
- Do not commit local configuration, databases, credentials, or build artifacts.
- Use [Conventional Commits](https://www.conventionalcommits.org/) where practical, for example `fix(store): create missing data directory` or `docs: explain air-gap mode`.
- Update `CHANGELOG.md` under an `Unreleased` heading for user-visible changes after v0.1.0.

## Pull requests

A pull request should explain:

- What changed and why
- Which commands or manual scenarios verified it
- Any documentation, configuration, or compatibility impact
- Remaining limitations or follow-up work

A maintainer may ask for changes before merging. By submitting a contribution, you agree that it will be distributed under the project's [MIT License](LICENSE).
