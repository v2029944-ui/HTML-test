# Ordinary report conversion (B2a)

`Stage::convert_legacy` sequentially reads three mapped columns at a time from staged SQLite cells and writes report detail, totals and diagnostics to separate SQLite tables. One source row and four organization accumulators are held in memory. Report detail and diagnostics are never collected into report-sized arrays. The entire conversion transaction is on the private staging database; it does not lock or mutate the live user database. Failure/cancellation rolls back the new conversion and retains the previous complete preview.

`LegacyOptions` requires one component path, explicit kind (`day`, `week`, `month`), explicit ISO start date and four configured organization names. No start date is inferred from the filename. Monthly starts must be the first day. `TripleMapping` row and columns are **one-based**, unlike existing UI mapping indexes; the future adapter must convert explicitly. Without mappings, headers are detected in the first ten rows using the same quantity/amount labels as `readTripleColumns`. At most684 triples,2048 columns,60000 rows are permitted. Dates advance one day/week/month per triple.

Numeric parsing preserves finite values, zeros, decimals with comma, ECMAScript whitespace and numeric prefixes. Boolean cells are not numeric quantities. Detail rows retain source row and the exact work/count/amount addresses. Organization and grand-total comparisons retain the0.001 count and0.03 amount tolerances; report totals retain six count/two amount decimal rounding. Overflow fails conversion instead of publishing non-finite totals. No work or missing quantity is manufactured as zero. Unnamed rows with both numbers under a known organization keep the existing temporary-name behavior and a diagnostic.

`report_page`, `report_rows`, `report_issues` require a complete conversion and cap responses at100 items. Cursors are exclusive one-based report ordinal, detail-row index and diagnostic sequence. Report IDs are job-local, not globally unique live IDs. Live import must assign/validate persistent IDs before applying. Diagnostics expose structured expected/actual amounts and exact source cells. Text reasons are currently concise native descriptions; **exact legacy warning-message presentation is not yet ported**. No full message/quality UI parity claim is made.

For headless comparison, the CLI accepts optional `kind start component` after `source staging_root`. It completes conversion first, then emits bounded JSONL report/row/issue records. This is a developer tool, not the final import interface.

```sh
cargo test --manifest-path tauri/import-core/Cargo.toml
cargo build --manifest-path tauri/import-core/Cargo.toml
python tools/compare_native_legacy.py --binary "$PWD/tauri/import-core/target/debug/stage-xlsx"
```

The control comparison covers six ordinary periods for each of weeks/days/months: date bounds, all detail rows, rounded totals and mismatch flags match old `parseSheet`. A corrupted quantity also matches diagnostic kinds, rows, organizations and source-cell addresses. This does not establish parity for every Excel layout or all warnings/export/analytics.

Not implemented by B2a: daily-wide quarter/year interpretation; multi-sheet business conversion; merged date headers/styles/1904 dates; full source-period controls; exact warning text; confirmation/atomic live application/recovery; bounded native frontend analytics/exports/backups; Windows/WebView2. Those are prerequisites for switching the production importer. The existing autonomous HTML and live database remain unchanged.
