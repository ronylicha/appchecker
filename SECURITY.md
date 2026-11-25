# Security Policy

## Supported versions

Security fixes are applied to the latest released version of AppChecker.

| Version | Supported |
| --- | --- |
| 0.1.x | Yes |
| Earlier versions | No |

## Report a vulnerability

Please report suspected vulnerabilities through [GitHub private vulnerability reporting](https://github.com/ronylicha/appchecker/security/advisories/new) rather than a public issue.

Include:

- The affected version and operating system
- A description of the impact and attack scenario
- Minimal reproduction steps or a proof of concept
- Suggested mitigations, if known

Do not include secrets, private repository contents, or information about third parties. The maintainer will acknowledge the report, assess its scope, and coordinate a fix and disclosure timeline with you.

## Scope and security model

AppChecker reads local project configuration, Git metadata, and dependency manifests. Its daemon sends dependency names to configured registries unless `air_gap_mode` is enabled. It does not intentionally transmit repository contents or collect telemetry.

AppChecker's registry version comparison is **not** a vulnerability audit. Use a dedicated tool such as `cargo audit`, `npm audit`, or osv-scanner for vulnerability detection.

Users should protect `~/.config/appchecker/` and `~/.local/share/appchecker/` with normal filesystem permissions and review any custom registry URLs before use.
