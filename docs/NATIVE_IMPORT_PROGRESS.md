# Native import: implementation status, 2026-11-17

This document records partial implementation. The requested end-to-end native import is **not complete**.

## A — dates (existing PR #16)

Removed the invented 19.10.2026 suggestion in manual/folder import. Reset dates when switching an already-opened workbook; retain explicit dates before the first file and within the current workbook. Saved rules are retained, with a warning for the known legacy suggestion. General reliable inference of ordinary report start dates, including provenance and conflicting candidates, remains a separate extension. Wide daily header recognition is unchanged.

Local evidence: 112 Node tests, 45 Linux/Chromium tests, and 14 tests of the unchanged Rust storage module passed. The Rust module was linked into a separate Linux harness without Tauri GUI. PR #16 includes current main; no merge to main was performed.

## B1 — raw streaming and temporary storage

A standalone `tauri/import-core` reads ZIP/XML with bounded buffers into a private temporary SQLite database. Cells, shared strings and structural metadata remain on disk. Bounded indexed preview, source fingerprint checks, cancellation, error cleanup and exclusive startup recovery exist. There is no live report database write or production importer switch.

Validation includes malformed XML/DTD/entities, ZIP paths and ZIP64 preflight, coordinates/sparse addresses, oversized tokens, shared-string references, cancellation/source modification, SQLite FULL, manager ownership, keyset pages and a real process SIGKILL followed by recovery/re-ingestion. See the crate README for commands and precise budgets.

The resource profile covers raw ingestion and staging, not report validation/application or analytical/UI performance. Generated books have 250,000 cells per worksheet, with numeric and inline-text data or 90% empty formatted cells. The generator streams output. These are structural cell fixtures, not business-report parity fixtures.

## Not yet implemented

- B2: format recognition/conversion and complete business validation for all supported layouts/dates, with disk-backed report rows and diagnostics.
- C: preview confirmation, live database revision/conflict checks, bounded atomic application, cancellation/commit boundary and live-database crash/disk fault tests.
- D: bounded startup/post-import/selection reads and complete server-side analytics, exports and backups. Existing `load()` still loads full history and production `save()` can replace the full report set.
- E: native worker commands, integrated import states/progress/cancellation/pages, repeated full-pipeline load tests, UI reaction measurements and end-to-end regression parity.
- Windows/WebView2 and actual installed EXE checks. No such environment was available.

No safe-3-million-cell claim is made for the application. The standalone stage's resource results cannot establish complete-import memory bounds or compatibility. The autonomous HTML parser and its existing protective limits remain unchanged.

## Raw staging measurements (code 144bd32)

Linux, Rust 1.100 release build. Each fixture has two process invocations. Peak RSS is the Linux kernel `wait4().ru_maxrss` for the standalone Rust process, in KiB. No WebView2 process is involved. The first invocation is **not a cold-cache measurement**: fixture generation already warms filesystem caches; caches were not flushed. Shared-machine load varies. Time includes fingerprinting, ZIP/XML reading, staging SQLite writes/validation and cleanup; these phases are not timed separately.

| Cells | Content | XLSX bytes | XML bytes | Run 1 / run 2, s | Peak RSS, KiB |
|---:|---|---:|---:|---:|---:|
| 500,000 | numeric + text | 1,596,314 | 16,956,920 | 4.32 / 6.16 | 11,392 |
| 500,000 | 90% empty formatted | 1,208,534 | 12,206,900 | 8.77 / 11.95 | 11,504 |
| 1,500,000 | numeric + text | 4,786,278 | 50,868,428 | 24.11 / 12.66 | 11,504 |
| 1,500,000 | 90% empty formatted | 3,622,938 | 36,618,368 | 12.00 / 10.47 | 11,504 |
| 3,000,000 | numeric + text | 9,571,234 | 101,735,708 | 40.19 / 29.67 | 11,504 |
| 3,000,000 | 90% empty formatted | 7,244,554 | 73,235,588 | 25.74 / 25.86 | 11,504 |

Full per-component XML sizes, CPU times, content counts and repeated-run output are in [the measurement JSON](perf/native-xlsx-staging-2026-11-17.json). Counts refer to generated cell data, not valid business reports. No saved-report duplication, final application, restart-to-analytics time, cancellation latency or UI reaction measurement is inferred from these results.

The unchanged browser reader rejected both 3-million-cell variants at the existing 1.5-million-cell limit: 29.712 s for numeric/text and 26.770 s for formatting-heavy input in Linux/Chromium. This is the expected protected failure, not an application regression.

The earlier docs/XLSX_LIMITS.md profiles include report conversion and Chromium/Playwright RSS; this standalone staging profile excludes those components. The datasets also differ. The figures therefore demonstrate bounded raw staging on these fixtures, not a comparable whole-application speedup or a complete-import memory guarantee.

Final local B1 evidence: 14 Rust integration tests passed. A separate process test killed the CLI during staging writes, reopened the same staging root, completed re-ingestion and verified cleanup. Source-read and SQLite-FULL tests fail safely. Windows ACL/locking and Windows/WebView2 have not been tested.
