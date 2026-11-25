# Native AppChecker Desktop Redesign Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the terminal-first experience with a legible native desktop application that manages repositories, selective and global dependency updates, and AI-assisted breaking-change remediation without hiding side effects.

**Architecture:** Build a native `eframe/egui` desktop application in Rust alongside the existing CLI and TUI. Reuse project configuration, SQLite, parsers, registry clients, update execution, and notifications. Add `rfd` for the native directory picker and official Claude Code/Codex CLIs for OAuth and AI work.

**Tech Stack:** Rust 2024 with a minimum toolchain of 1.95, `eframe` 0.37, `rfd` 0.18, Tokio, rusqlite, existing AppChecker services, official `claude` 2.1.332 and `codex` 0.172.0 executables, GitHub Actions with mandatory Linux GUI dependencies.

---

## Intent and user jobs

AppChecker is for a developer tracking several local Git repositories. The existing TUI makes dependency status hard to read and project actions hard to discover. The new desktop application must make the common jobs explicit:

| User job | Screen | Question answered | Primary action |
| --- | --- | --- | --- |
| Add a repository | Projects | Which folder should be monitored? | Browse and Add |
| Understand project health | Projects and dependency details | What needs attention and why? | Inspect selected project |
| Update one dependency | Dependency details | Which package changes? | Update selected |
| Update one project | Dependency details | What changes in this project? | Update project |
| Update every project | Global updates | What changes everywhere? | Review and Update all |
| Handle breaking changes | AI remediation | What breaks and what is the minimal fix? | Analyze, review, Apply |
| Avoid losing work | Confirmation and Activity | What ran, what changed, can I undo it? | Review or Roll back |

## Decisions

- **Approved approach:** native Rust desktop using `eframe/egui`, with `rfd` for directory selection. CLI and TUI remain supported as fallback interfaces.
- **Design:** light and dark themes with WCAG-checked contrast, restrained accent color, clear type hierarchy, visible focus, 8 px button corners, no status conveyed by color alone.
- **Layout:** left navigation for Projects, Updates, AI remediation and Activity; contextual action bar for the selected project; global actions in a separate toolbar. Selected dependency, project and global update actions are never visually conflated.
- **Native interactions:** real buttons and modal windows, directory picker, confirmation dialogs, scrollable update review, busy states, empty states, partial failures, cancellation and keyboard navigation. Directory picking, scans, updates, discovery, provider calls and synchronous store reads run on workers; bounded channels deliver generation-checked results without freezing rendering or removing projects late.
- **OAuth and models:** launch unmodified official `claude`, `codex`, and Antigravity (`agy`) authentication flows in a real terminal or display the exact guided command, then poll `claude auth status --text`, `codex login status`, and `agy models`. Load model choices from `claude --help`, `codex debug models`, and `agy models`. Headless analysis uses `claude -p` with JSON Schema, `codex exec --output-schema`, and sandboxed `agy --print` with a model selected by the user. Never use Claude `--bare`, read, display or persist provider tokens. Missing executables or unauthenticated accounts produce actionable errors, never simulated AI results. Antigravity requires an explicit third-party usage acknowledgement and no provider calls in air-gap mode.
- **AI safety:** each AI request requires per-project consent naming the exact files sent. Analysis runs from a disposable directory containing only approved size-bounded files, with provider instructions/hooks/MCP disabled, Claude restricted/no-shell settings, Codex `exec --sandbox read-only --ask-for-approval never --ephemeral --ignore-user-config`, scrubbed environment, and no network calls in air-gap mode. Release notes and source context are treated as untrusted prompt content. AI returns structured findings and a proposed patch, never executes commands. The user reviews every patch before application.
- **Updates:** dependency-selected, project-selected and all-project updates use existing `UpdateManager`. Previews rescan and revalidate every project, package and version immediately before confirmation and execution. Global execution is sequential, guarded per project, and continues after individual failures with an aggregate result. Lifecycle scripts are suppressed where the ecosystem supports it; other repositories require explicit trusted-repository opt-in. UI explains lockfile/transitive dependency changes and post-update checks instead of claiming a dependency-manager dry run.
- **Additional useful ideas:** search/filter, “updates only” filter, pinned projects, session-only sanitized Activity with bounded retention, update cancellation, explicit range/private/air-gap states, and a safe dry-run summary before global updates. Raw AI prompts, source context and patches are not persisted by default.
- **Vulnerability information:** query OSV for exact public package versions and display CVE/GHSA aliases and severity in a scrollable dependency table. Empty results mean “no known CVE”, never “safe”; failures and air-gap mode remain explicit.
- **Non-goals:** unverified vulnerability-audit claims, autonomous background AI edits, token storage, replacement of the CLI/TUI, bypassing confirmation, or hiding unsupported manifests and failed scans.
- **Anti-goals:** decorative dashboard cards without a job, low-contrast badges, fake AI output, silent shell execution, unreviewed code changes, irreversible deletion, global buttons that act on an implicit selection, or destructive rollback over subsequent user work.

## Snapshot and recovery rules

- Every update operation acquires a per-project lock and validates expected HEAD plus a clean worktree before execution. Global operations hold only one project lock at a time.
- Backups of affected manifests and lockfiles live outside the repository with restrictive permissions. Cancellation, timeout and partial completion are explicit states; killing a package manager is not presented as rollback.
- AI patches require expected HEAD, a clean worktree, a dedicated branch with collision handling, exact reviewed-diff hash, `git apply --check`, and the same validation immediately before application. Absolute paths, `..`, `.git`, symlinks, submodules and out-of-scope edits are rejected. Rollback runs only when the tree still matches the applied patch, never through `reset --hard` over later edits.
- Check selection is ecosystem-specific: Cargo test/check, npm test when declared, Go test, Composer project scripts, bundle test when declared, and Python tests only when configured. Missing checks are reported, not fabricated.

## Implementation tasks

### Task 1: Native shell and design kit

**Files:**
- Create `src/desktop/mod.rs`, `src/desktop/theme.rs`, `src/desktop/widgets.rs`, `src/desktop/app.rs`
- Modify `src/main.rs`, `src/lib.rs`, `Cargo.toml`, `Cargo.lock`

**Steps:**
1. Add failing tests for theme contrast, navigation selection, and initial application state in `tests/desktop_theme_test.rs`.
2. Run the tests and confirm missing desktop module failures.
3. Add `eframe` 0.37 and `rfd` 0.18, require Rust 1.95+, implement design tokens, accessible widgets, worker/channel architecture, responsive navigation, and a native `appchecker gui` subcommand plus `appchecker` default GUI entry with `tui` retained explicitly.
4. Verify tests, Clippy, and `cargo build --release --locked`.

### Task 2: Project management and native directory picker

**Files:**
- Create `src/desktop/projects.rs`
- Reuse `src/config/projects.rs`, `src/config/loader.rs`
- Test `tests/desktop_projects_test.rs`

**Steps:**
1. Write failing tests for add/edit/remove/pin/discover, invalid paths, duplicate paths, and confirmation state.
2. Implement real forms, validation, `rfd::FileDialog::pick_folder`, discover review, and native confirmations.
3. Persist through the existing ProjectManager/Store path and verify no configuration regression.

### Task 3: Selective, project and global updates

**Files:**
- Create `src/desktop/updates.rs`
- Reuse `src/deps/update.rs`, `src/deps/registry.rs`, `src/persist/store.rs`
- Test `tests/desktop_updates_test.rs`

**Steps:**
1. Write failing tests for selected dependency, whole-project, all-project, partial failure, cancellation and post-update rescan.
2. Implement dependency and project update queues with per-project locks, clean-worktree validation, external backups, lifecycle-script policy, global preview, sequential execution, cancellation/partial states, per-project result aggregation, conditional recovery, and session Activity.
3. Verify normal/dev/alias/range/private behavior matches CLI/TUI and no update begins without confirmation.

### Task 4: OAuth and AI remediation

**Files:**
- Create `src/desktop/ai.rs`
- Test `tests/desktop_ai_test.rs`

**Steps:**
1. Write failing tests for missing provider, unauthenticated provider, malformed AI response, redaction, patch review, rollback, and rejected out-of-scope edits.
2. Implement provider detection and official `claude`/`codex` login launch in a real terminal, status polling, disposable analysis directories, bounded/redacted input, structured response parsing, and clear errors.
3. Implement reviewed patch application with a dedicated Git branch, external backup patch, exact diff hash and path/symlink/submodule validation, expected-HEAD checks, and conditional rollback.
4. Run ecosystem-specific project checks after applying edits and report success or failure honestly.

### Task 5: Visual and workflow verification

**Files:**
- Modify desktop modules and tests as needed
- Create no snapshot files in the repository; screenshots stay under `target/tmp/`

**Steps:**
1. Run all unit/integration tests and real native startup at 1280×800 and 800×600 in light and dark themes, plus 200% text scaling and HiDPI.
2. Exercise keyboard-only modal focus traps, empty/loading/error/long-content states, long-list virtualization, invalid paths, selective/global updates, partial failures, missing OAuth provider, malformed AI output, and rollback.
3. Capture native screenshots and inspect them; check focus order, text contrast, clipped content, AT-SPI labels and status semantics. Test the portal picker and authenticated disposable repositories for both providers when credentials are available.
4. Request an Antigravity UI/UX review with screenshots and code, wait for completion, implement actionable findings and rerun affected checks and review after UI fixes when new defects appear.

### Task 6: Release 0.3.0

**Files:**
- Update `Cargo.toml`, `Cargo.lock`, `README.md`, `CHANGELOG.md`, AppStream, Debian and Omarchy packaging
- Update `dist/appimage/AppChecker.AppDir/AppRun`, both desktop entries and AppStream metadata for a desktop application with `Terminal=false`, including the native GUI portal and wgpu/OpenGL fallback
- Update `.github/workflows/ci.yml` with mandatory Linux GUI build dependencies and clean X11/Wayland validation

**Steps:**
1. Bump minor version to 0.3.0 and document native GUI requirements and fallback interfaces.
2. Validate complete test suite, release build, AppStream, AppImage and Debian artifacts, installation and native startup.
3. Commit and push, require green CI, publish `v0.3.0` with AppImage, `.deb` and SHA256SUMS, then download and verify them.

## Verification contract

Every behavior lands test-first where practical. The full suite runs after each fix. Desktop verification exercises actual native rendering, not only state tests. No feature is considered complete with a placeholder control, fake provider response, unreviewed patch, unhandled error or inaccessible primary action. Antigravity's UI/UX findings are triaged and implemented before release, without additional independent review rounds unless new defects appear.
