# Native XLSX staging core (B1)

This standalone crate implements **raw ZIP/XML ingestion into a temporary SQLite database**. It is not connected to Tauri commands or the production importer. It does not yet convert cells to business reports, validate financial totals, apply reports to the live database, or replace whole-history reads. These are required subsequent stages; this crate alone does not complete native import.

## Local commands

```sh
cargo test --manifest-path tauri/import-core/Cargo.toml
cargo build --release --manifest-path tauri/import-core/Cargo.toml
./tauri/import-core/target/release/stage-xlsx /path/book.xlsx /private/staging/root
python tools/profile_native_xlsx.py --binary "$PWD/tauri/import-core/target/release/stage-xlsx" --output /tmp/native-profile
python tools/test_native_recovery.py "$PWD/tauri/import-core/target/release/stage-xlsx" /tmp/native-profile/500000-values.xlsx /tmp/native-recovery
python tools/profile_browser_reader.py /tmp/native-profile/3000000-values.xlsx
```

Rust tests do not require GTK, WebKit, Node, Tauri GUI or GitHub Actions. Browser comparison requires installed Python Playwright and `/usr/bin/chromium`. Tests are Linux tests, not Windows/WebView2 validation. Development uses Rust 1.100; the standalone crate declares 1.82, which has not separately been tested. The existing Tauri manifest/toolchain is unchanged.

## Ownership and lifecycle

`StageArea::open(private_root)` holds an exclusive OS file lock. A second manager cannot open the same root. On startup, the owner removes `import-*` leftovers from the previous process; other files remain. The root must be an application-owned private directory, not a directory containing user reports. The CLI defaults to a separate temporary root. On Unix the root uses mode 0700; Windows relies on inherited user-directory ACLs and remains untested.

`ingest(source, limits, cancel, progress)` checks the source size, hashes the opened handle, validates the terminal ZIP32 footer and an exactly bounded central directory, then reads one component at a time. The zip library does not discover footers or construct an archive index. It writes shared strings, cells, and structural metadata directly to SQLite. Shared-string references are checked against the disk index after all entries have been read, regardless of ZIP entry order. Hashing the path again detects source replacement or ordinary modification during reading. This is not the final source recheck required at confirmation.

The returned `Stage` borrows its `StageArea`; recovery cannot run while an in-process stage is alive. The database closes before the temporary directory is removed. Errors and cancellation drop the temporary stage. A process kill can leave a stage directory; the next exclusive manager removes it. `manifest.complete` is written only after successful ingestion and final source validation. Earlier committed staging batches are never advertised as complete and never affect the user's database.

`page(sheet_component, after, limit)` uses an indexed `(row, column)` cursor. At most 100 cells are returned, including original addresses and cached formula values. Cursor = `(row - 1) * 2048 + column`; coordinates are one-based. Full cell values stay on disk; one returned value is at most 64 KiB. No all-cells JSON or array is produced.

Progress is a callback at 2048-cell batch boundaries and component completion. Cancellation uses `AtomicBool`, checked between XML events and input reads. Heavy ingestion must run on a worker when native commands are added. The CLI is synchronous and intentionally does not simulate UI progress or cancellation.

## Budgets and dependencies

- ZIP source: 30 MiB; entries: 4096, checked before `ZipArchive::new`.
- ZIP64 and multi-disk archives: explicitly unsupported. ZIP64 sentinels and locator are rejected during preflight.
- Actual decompression: 48 MiB per component, 192 MiB per archive, ratio at most 1000. Declared sizes are an early check; actual reads enforce budgets too.
- XML: no DOM, nesting at most 64, one event at most 1 MiB, one cell/shared string at most 64 KiB.
- Coordinates: 60,000 rows and 2048 columns; no sparse arrays. Duplicate addresses fail.
- Native raw-cell budget: 3,000,000. This belongs only to the experimental disk-backed path. The browser's 1,500,000-cell limit is unchanged.
- SQLite page cache: 2 MiB, temporary SQL structures on disk, main staging database at most 2 GiB. Journals and temporary SQL files require additional disk space; this is not a filesystem quota.
- Preview: 100 cells, at most roughly 6.25 MiB of text before application/serialization overhead.

`zip` 2.4.2 uses only Deflate support. A strict ZIP32 validator builds an index of at most 4096 names/headers; it checks exact directory/local-header/payload/descriptor bounds and rejects overlapping ranges. Each component is passed to the library’s stream reader with a validated local header and a bounded compressed payload. This avoids the library’s footer fallback and unbounded archive-index construction entirely. CRC and actual uncompressed length are verified; data descriptors are checked against directory values. ZIP64, self-extracting/prefixed archives and trailing data after the terminal footer are unsupported. Files are never extracted by archive path. `quick-xml` 0.38.4 provides events without worksheet DOMs; an additional `BufRead` event budget prevents oversized tokens from growing its buffer indefinitely. DTDs, undefined entities, malformed document roots and unsafe/duplicate ZIP names are rejected. There is no networking code or XML entity resolver. `rusqlite`/`sha2` match existing backend choices; `tempfile` supplies unpredictable owned directories, and `fs2` supplies a cross-platform OS lock for recovery ownership. `Cargo.lock` records transitive dependencies. This is not a claim of a complete security audit.

## Required follow-up

Business-format recognition, merged-header interpretation, Excel date/1904 conversion, report/totals validation and bounded diagnostics are **not implemented in this crate yet**. Stored styles/merges/workbook metadata are staging input for that work, not proof of format compatibility. Confirmation, database revision/conflict validation, atomic live application, complete selective analytics/export/backups, async Tauri/UI integration and Windows fault testing remain mandatory. Do not switch the production importer to this crate until those paths have parity tests.
