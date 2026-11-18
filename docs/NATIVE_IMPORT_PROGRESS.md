# Native import: implementation status, reconciled 2026-11-18

This document records partial implementation. End-to-end native import is **not complete**. Status checked against `main` commit `5d4e99a` on 2026-11-18. The performance tables below retain their historical commit references. The manually built 0.1.3 EXE comes from earlier `f26e5e6` and **does not contain the merged native core**.

## A — dates (merged PR #16 and #24)

PR #16 removed the invented 19.10.2026 date suggestion, resets dates when changing workbooks and preserves explicit user choices as appropriate. Merged PR #24 adds bounded evidence of ordinary report start dates from explicit sheet-header labels and requires confirmation for conflicts; see [date evidence](PERIOD_DATE_EVIDENCE.md). Neither change guesses dates from arbitrary filenames or guarantees recognition of every layout. Wide reports retain their separate date handling.

Historical PR #16 checks passed: 112 Node, 45 Linux/Chromium and 14 Rust storage tests in a separate non-GUI harness. **PR #16 and PR #24 have since been merged into `main`.** These earlier test counts do not verify the present `main` in Windows/WebView2.

## B1 — raw streaming and temporary storage

A standalone `tauri/import-core` reads ZIP/XML with bounded buffers into a private temporary SQLite database. Cells, shared strings and structural metadata remain on disk. Bounded indexed preview, source fingerprint checks, cancellation, error cleanup and exclusive startup recovery exist. There is no live report database write or production importer switch.

Validation includes malformed XML/DTD/entities, ZIP paths and ZIP64 preflight, coordinates/sparse addresses, oversized tokens, shared-string references, cancellation/source modification, SQLite FULL, manager ownership, keyset pages and a real process SIGKILL followed by recovery/re-ingestion. See the crate README for commands and precise budgets.

The resource profile covers raw ingestion and staging, not report validation/application or analytical/UI performance. Generated books have 250,000 cells per worksheet, with numeric and inline-text data or 90% empty formatted cells. The generator streams output. These are structural cell fixtures, not business-report parity fixtures.

## Not yet implemented

- Production B/C integration: standalone staging and conversion from **merged PR #17, #19 and #22** are in `main`, but are **not connected to the production importer or live SQLite**. Draft [#20](https://github.com/v2029944-ui/HTML-test/pull/20) adds prepared manual application, draft [#21](https://github.com/v2029944-ui/HTML-test/pull/21) handles ownership and quality acknowledgment, and draft [#25](https://github.com/v2029944-ui/HTML-test/pull/25) extends preparation for wide reports. They remain unmerged; their integration and source-diagnostics lifecycle still need work.
- Production D: draft [#18](https://github.com/v2029944-ui/HTML-test/pull/18) provides bounded revisioned reads/backups, and dependent drafts [#23](https://github.com/v2029944-ui/HTML-test/pull/23) and [#26](https://github.com/v2029944-ui/HTML-test/pull/26) provide SQL analytics/CSV and bounded settings/management. **None is activated in `main`.** Production `load()` still reads the entire report history and `save()` can replace the full report set.
- E: Tauri background worker/IPC commands, integrated progress/cancellation/review/pages, complete frontend analytics and export migration, repeatable full-pipeline memory/UI profiling, and end-to-end regression parity are not implemented.
- Windows/WebView2 validation of the **new native-import path** and installation of an EXE built from the current `main` are missing. The earlier successful beta GUI smoke does not verify this new core; the locally built 0.1.3 EXE has not been run in Windows.

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

## B2 — disk-backed business conversion

Ordinary triples support explicitly selected day/week/month dates and manual
columns. Daily-wide conversion adds annual/quarter sheets, shared work columns,
merged headers, 1904 styles/serials, short-date year confirmation, cached formulas,
zero-day provenance, coverage and source-period controls. Financial rows and all
diagnostics remain in private SQLite; the CLI streams pages as JSONL. Relevant
source issues attach once to the first imported day per sheet. Source issue
scope/date and explicit-zero validity still need preparation/application
integration; the production importer has not switched.

Final wide code validation: 35 Rust tests pass; 19 real-XLSX comparisons against
the current JavaScript importer pass, alongside the ordinary reference checks.
Independent review found split-cell coverage, year-suffix whitespace and cropped
text-boundary defects. Focused failures were reproduced and all were corrected;
the follow-up code review passed for this core scope. Exact warning prose is
not claimed equivalent. See [native wide contracts and commands](../tauri/import-core/WIDE_REPORTS.md).

### Measured wide core, code 1af6599

Linux release Rust 1.100, exact XML-cell totals distributed over four quarterly
business sheets plus formatting-only padding. Both invocations start a new Rust
process/private stage. The first is **not cold cache**. Other work, compilation
and Chromium probes ran on this shared machine; the wide timing spread is retained
below. VmHWM is read **inside** the Rust child through `/proc/self/status`, avoiding
the fixture generator's inherited high-water mark. It is process RSS, not private
memory and not application/WebView2 memory. Fixture creation/statistics are excluded.

| Cells | Content | Read incl. staging, s (1 / 2) | Convert incl. checks/SQLite, s (1 / 2) | Wall incl. cleanup, s (1 / 2) | Maximum Rust VmHWM, KiB |
|---:|---|---:|---:|---:|---:|
| 500,000 | numeric/text | 4.171 / 11.162 | 9.779 / 14.696 | 13.969 / 25.950 | 10,464 |
| 500,000 | mostly formatted empty | 9.380 / 15.659 | 2.513 / 2.926 | 11.915 / 18.612 | 10,328 |
| 1,500,000 | numeric/text | 9.752 / 9.736 | 18.926 / 19.371 | 28.723 / 29.146 | 10,392 |
| 1,500,000 | mostly formatted empty | 10.912 / 10.017 | 2.999 / 2.169 | 13.954 / 12.213 | 10,288 |
| 3,000,000 | numeric/text | 26.308 / 46.513 | 54.690 / 158.187 | 81.079 / 204.808 | 10,312 |
| 3,000,000 | mostly formatted empty | 29.483 / 29.002 | 4.501 / 5.606 | 34.027 / 34.669 | 10,572 |

The 3m value workbook is 6,743,420 bytes compressed / 80,722,002 bytes XML;
2,992,806 cells contain values and 7,190 are empty formatted (four further empty
cells are unformatted). It yields 366 reports / 1,469,856 detail rows and a
383,033,344-byte final private stage. The formatting variant is 7,084,153 bytes /
60,313,339 bytes XML, with 2,992,118 formatted-empty cells and 7,878 values;
366 reports / 1,464 details occupy a 267,362,304-byte stage. These are final stage
file lengths, not total peak filesystem use. Per-component lengths, all content
counts, runs and methodology are in [the measurement JSON](perf/native-wide-2026-11-17.json).

Additional repeated stress cases use 50,000 disk-backed styles and maximum-height
sheets: two reports of 59,996 rows each finished in 6.851 / 5.698 s, peaking at
10,352 KiB. A variant with 119,994 diagnostic records finished in 6.527 / 6.696 s,
peaking at 10,228 KiB. Neither diagnostics nor the whole report was accumulated
in a Rust vector. These fixtures do not prove every pathological input safe.

Twelve real cancellation probes cover both content variants at all three sizes,
during reading and at conversion/index setup. A thread requested cancellation
30 ms after phase entry. All exited with cancellation/interruption and no private
operation directory; the maximum observed response plus stage cleanup was
36.039 ms. The unit test separately cancels after report processing starts and
checks preservation of the previous complete preview. No live database is involved
in these core probes, and these are not universal cancellation latency guarantees.

On these same 3m files, the unchanged browser reader correctly rejected the
1.5m-cell limit after 81.951 s (values) / 54.169 s (formatting) in Linux/Chromium.
The earlier `XLSX_LIMITS.md` measurements include Chromium/Playwright and different
books; these Rust-only figures cannot be used as a direct speed/memory ratio.

Final live-application timings, deduplication after saved reimport, application
relaunch/analytics availability, backup/export, cold launches and UI reaction are
**not measured here**. No claim that the production app safely handles 3m cells
follows from this core profile. The remaining integration/Windows gates above
still apply.
