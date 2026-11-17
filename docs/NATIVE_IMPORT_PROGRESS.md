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
