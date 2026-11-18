# Instructions for GitHub Copilot and other coding agents

Read [AGENTS.md](../AGENTS.md) and [docs/RELEASE_POLICY.md](../docs/RELEASE_POLICY.md) before changing the Tauri release process.

**Every new application version MUST be published as a separate GitHub Release with a Windows installer and a concrete changelog / release notes describing what changed.** A successful GitHub Actions artifact alone is not a release. Confirm published assets and update README links. Obtain the owner's approval for publishing; never advance the signed updater channel without signed, validated artifacts. If release gates fail, report the version as not released.

**The application is still under development: backward compatibility between versions (including 0.1.3) is NOT a release or PR merge requirement.** Old-version migrations and upgrade tests are optional unless the owner explicitly requests them. Schema, internal formats and APIs may change incompatibly. Disclose known incompatibilities in release notes; do not claim automatic migration. Keep data-integrity tests, safe backups and no-unapproved-data-loss safeguards for the current version.

Keep standalone `index.html` and existing user SQLite safe from unapproved deletion or overwrite. Avoid merging draft PRs or publishing diagnostic WebView2 installers.
