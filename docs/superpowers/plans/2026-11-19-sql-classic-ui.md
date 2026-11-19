# SQL classic UI Implementation Plan

> Workers: execute continuously, targeted independent tasks via Caveman; final whole-branch review.

**Goal:** привычный Excel-интерфейс и настройки поверх основной SQLite, установленная версия и честная загрузка.
**Architecture:** reuse classic appearance source at build time; keep paged SQL renderers; bounded metadata calendar.
**Tech Stack:** Tauri2, vanilla HTML/CSS/JS, Rust/rusqlite, Node tests, Playwright/WebView2.
**Spec:** ../specs/2026-11-19-sql-classic-ui-design.md

## Global constraints
- Standalone index.html remains independent; never load all reports into SQL WebView.
- Preserve zero-vs-missing semantics, paging, backups, revisions, FULL and cancellation.
- Existing release assets immutable; new unique version and full Windows checks before release.

## Review focus
- Theme/density and dropdown remain usable at320px/200%, keyboard and storage denied.
- Concurrent/failed IPC never leaves spinner stuck; installed version is not update version.
- Calendar revision/year races never show stale or false-complete data.
- Changes tab retains common filters and actual SQL calculation controls.
- Print/export remain meaningful; shared classic CSS cannot hide native print.

## Tasks
- [x] Locate classic UI and previous corrections; baseline Node suite.
- [x] Version/activity: deferred regression RED/GREEN, bridge event tracking, runtime version, full Node42 files. Commit d564ab9.
- [x] Shared appearance: failing actual-script preference tests; extract classic style/settings logic in prepare; familiar shell/menu/settings; green suite.
- [x] Reports: bounded SQL calendar, metadata-only tests + native JS races, friendly paged report list, open details from date.
- [x] Analytics composition: common filters, distinct changes view, classic cards/controls, preserved charts/paging/printing.
- [x] Browser visual/interaction verification, installed WebView2 assertions for new UI, versioned notes, review, PR CI.
- [x] Integration and exact-main signed release through existing pipeline, public manifest verification, README/status.

## Ledger
- Base27f3d26; isolated /workspace/sql-classic-ui. User provided concrete existing UI reference; implement matching reference without speculative redesign.

- Appearance regression RED/GREEN; three tests pass. Calendar/list-range and navigation tests RED/GREEN. Node44 files green; Rust164 pass/1ignored.
- Chromium mock-IPC verification: real DOM/menu, 29-day February, cross-boundary weekly report start-date selection, focus, shared filters, loading, print; 320/390/1280/1366 widths with xlarge without page overflow. This is not installed WebView2 evidence.

- Whole-branch review found: stale calendar after failed workspace reads; missing Changes print action; embeddedfonts blocked by CSP; frozen Tauri core namespace made smoke instrumentation ineffective. Fixed all four with targeted regressions and FontFaceSet browser proof. Calendar keeps the selected year through error recovery.
- Print palette: explicit light output and disabled print transitions; computed white/ink regression RED/GREEN. Reviewer verified Chromium economy already darkens default PDF text; unreadability was not proven.
- Node44files green after fixes, Rust calendar remains164pass/1ignored, browser actual CSP embedded Inter RED/GREEN. No deferred blocking findings.

- Final reference correction: classic division colors stay assigned by full settings order when a subset is selected. SQL now passes that order into the renderer; regression RED/GREEN includes filtering and metric switch.

- Installed WebView2 caught first-screen regression at1280x720: next analysis panel began828px down. Kept the original assertion, removed duplicate import/intro and compacted shared header/filters/overview. Browser exact geometry RED/GREEN includes1270px to account for Windows scrollbar.

- PR75 full Windows NSIS/WebView2 passed on1024c5d (run44528823535), including preserved first-screen assertion, calendar, fonts, version, activity, colors, import/recovery. Merged source754b290; exact-main CI44531479350 passed. Signed release44534276975 passed.

- Published signed prerelease1.7.0 and advanced the online channel. Public EXE/sig sizes and serverSHA256 matched downloaded files; manifest version1.7.0 and signature matched Release. Previous1.6.1 assets unchanged. Source754b290, release451405534.
