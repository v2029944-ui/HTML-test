# Bounded SQLite read-model foundation (D1)

This harness includes the **production** `../src-tauri/src/storage.rs`, without Tauri,
GTK, WebKit, or a GUI build. The original `reports(..., json TEXT)` and `kv` schema,
full `load()` response, save format, and legacy JSON files remain intact. This is an
additive Rust API foundation; the frontend, Tauri commands, charts, JSON export,
and the streaming XLSX import flow have not been migrated to it.

## API

All methods are on `Storage`. Errors are explicit `Result<_, String>` values.

| Method | Behavior |
| --- | --- |
| `revision() -> i64` | Read the current committed revision after normal storage initialization/migration. Repeated reads do not advance it. |
| `report_page(revision, &filter, after: Option<(&str, &str)>, size)` | Headers only, ordered by `(period_start, id)` with SQLite BINARY comparison. Cursor is the previous final `(period_start, id)`. No JSON rows are returned. |
| `index_report(id, revision) -> i64` | Backfill one existing report; returns its complete row count. Does not change report JSON or revision. |
| `row_page(revision, &filter, after: Option<(&str, i64)>, size)` | Indexed rows ordered by `(report_id, row_index)`, zero-based row indexes. Cursor is the previous final pair. |
| `aggregate(revision, &filter)` | Complete matching row count, sum of count, sum of amount, and amount/count weighted average. Does not use a page. |
| `backup_sqlite(&destination)` | Complete consistent SQLite backup, including every report JSON, all kv values, revision, manifests and indexes, even when reports are unindexed. Destination must not exist. |

Page size must be explicit, from 1 through 500 inclusive (there is no implicit page
size). Reuse a cursor only with the same filter and revision. Every query starts a
SQLite read transaction and checks the expected revision in that snapshot. A
later external commit does not change an already established snapshot; the next
page/query with the old token fails. Backfill also checks the token; an external
commit during parsing causes snapshot-upgrade failure and prevents publication.

`ReadFilter::default()` selects **all report types, all dates, all organizations,
and all works**. An absent type means all three types (`day`, `week`, `month`);
callers must choose a type when overlapping day/week/month data should not be
combined. Date bounds are inclusive **full period containment**:
`period_start >= start` and `period_end <= end`. A partly covered week/month is
excluded entirely; its totals are never distributed across days. Organization
and work lists match exact, case-sensitive strings; each list is OR, the two lists
combine with AND. Empty lists impose no restriction. Each list allows at most 100
strings of at most 64 KiB UTF-8 each. Invalid dates/types/reversed bounds fail.

Row queries and aggregates reject a selection containing any unindexed report,
even if that report might have no rows matching the organization/work filter.
Unfiltered metadata header pages can enumerate reports before backfilling; header
queries with organization/work filters require valid indexes. Empty reports get a
valid manifest with zero rows and are included in header pages without row filters.
Header fields and indexed labels/IDs are limited to 64 KiB per string; oversized
header values cause errors rather than unbounded output or truncation.

The aggregate uses SQLite `COUNT(*)`, `SUM(count)`, and `SUM(amount)` over the whole
selection. Weighted average is `SUM(amount) / SUM(count)` and is `None` when total
count is zero, including zero count/nonzero amount. Empty selections produce zero
counts/sums and no average. Negative/fractional values remain supported. Numbers
use SQLite REAL/Rust f64, with no rounding; non-finite inputs/results fail.
SQLite summation and JavaScript summation can differ in the final floating-point
bits; decimal money rounding/chart analytics parity has not been established.

## Additive schema and import confirmation contract

Initialization creates these tables without replacing any existing report:

```sql
CREATE TABLE storage_revision (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  revision INTEGER NOT NULL CHECK(typeof(revision)='integer' AND revision>=0)
);
-- Exactly one row: singleton=1. Initial revision=0.
CREATE TABLE report_row_index (
  report_id TEXT PRIMARY KEY, row_count INTEGER NOT NULL
);
CREATE TABLE report_rows (
  report_id TEXT NOT NULL, row_index INTEGER NOT NULL,
  organization TEXT NOT NULL, work TEXT NOT NULL,
  count REAL NOT NULL, amount REAL NOT NULL,
  PRIMARY KEY(report_id,row_index)
) WITHOUT ROWID;
CREATE INDEX report_rows_by_work ON report_rows(work,organization,report_id);
```

`read_model_report_{insert,update,delete}` and
`read_model_kv_{insert,update,delete}` are AFTER triggers. Each advances the single
revision row within the caller's transaction. Report triggers also delete affected
manifests and normalized rows (both OLD and NEW ID on UPDATE). This covers ordinary
older/external SQLite writers, including writes of identical values. Rollback
restores reports, kv, revision, and indexes together. Checked INTEGER overflow
aborts the mutation instead of wrapping/reusing a token. Index-only maintenance
and revision reads do not advance the revision. The revision is a change token,
not an operation count: one full save can advance it many times.

For future import confirmation, initialize storage first, then in the **same write
transaction that publishes the import** compare the captured token with:

```sql
SELECT revision FROM storage_revision WHERE singleton=1;
```

Use an IMMEDIATE transaction or equivalent serialization and abort on mismatch.
Do not compare outside the publication transaction. Mutate original reports/kv
normally so the triggers advance the token and invalidate affected indexes.
Publish normalized rows plus their manifests atomically if producing them during
import; otherwise leave the manifest absent until `index_report` succeeds. Do not
set/reset revision directly. Preserve i64 precision across IPC (e.g. decimal string
when exposing tokens to JavaScript); these methods are not yet registered as Tauri
commands. Direct edits to read-model tables or trigger/schema removal are outside
the supported external-writer contract.

Existing `save()` already owns Values and inserts rows plus manifests in its
existing transaction, without calling full-history `load()`. If legacy-compatible
IDs/labels exceed 64 KiB, save preserves the complete JSON and intentionally leaves
the report unindexed; selective APIs error explicitly. A real index write error
rolls the save back, including its original DELETE/INSERT behavior. Indexing never
deletes or replaces a report to create indexes.

## Bounded backfill and backup

Backfill opens the existing TEXT JSON using a readonly SQLite blob, uses an 8 KiB
BufReader, holds one deserialized row at a time, and sends unknown metadata/row
fields to `IgnoredAny`. A reader guard rejects **raw JSON tokens** above 64 KiB
(including unknown strings and escaped strings); serde_json's recursion limit is
retained. Required root id/type/start/end must match the original report columns;
duplicate required fields/rows, missing rows, malformed JSON, invalid row fields,
and trailing data fail before publication.

Rows stage in a FILE temp table. Main and temp page-cache targets are 2 MiB each.
Only after the complete report validates does one SQLite transaction publish
`INSERT ... SELECT` rows and the manifest. Malformed reports, index SQL failures,
revision mismatches, and concurrent writes cannot leave a partial published index.
Disk requirements grow with report/index size; there is no disk-space reservation
or progress/cancellation UI in this foundation.

Backup uses the SQLite online backup API in 64-page steps and never constructs a
whole-history Value. It writes a new sibling staging file, closes the SQLite target,
then uses an atomic no-clobber hard link to publish the destination. Existing
files/symlinks, including destinations created during backup, are not overwritten.
The destination filesystem must support hard links. Failed backup/publication
removes staging; it does not remove or modify an existing destination. Existing
JSON backup formats are unchanged; this API adds a SQLite backup option.

## Reproduction and measured scope

From the repository root:

```sh
export CARGO_HOME=/workspace/rust/cargo
export RUSTUP_HOME=/workspace/rust/rustup
/workspace/rust/cargo/bin/cargo test --offline --manifest-path tauri/storage-core/Cargo.toml
/workspace/rust/cargo/bin/cargo build --offline --manifest-path tauri/storage-core/Cargo.toml --example backfill
python3 tauri/storage-core/examples/make_fixture.py tauri/storage-core/target/probe-100k 100000
python3 tauri/storage-core/examples/make_fixture.py tauri/storage-core/target/probe-1m 1000000
python3 tauri/storage-core/examples/measure.py tauri/storage-core/target/probe-100k
python3 tauri/storage-core/examples/measure.py tauri/storage-core/target/probe-1m
```

Fixture creation requires fresh directories; the existing measured fixture
locations must be changed for a repeat run. Each fixture is one original-schema
TEXT report (ID `huge`, type `day`, start=end `2026-11-01`) with N unknown metadata
arrays `[1,2,3]` and N rows. Every row is organization `A`, work `W`, count `1`,
amount `2`, plus an unknown array `[1,2,3]`. The Python fixture builder may allocate
the whole JSON for SQLite insertion; it finishes **before** the separate measured
Rust process starts. The probe measures backfill plus the full aggregate, using
debug builds; Python reads child `ru_maxrss` on Linux and monotonic elapsed time.

| Rows | Original JSON bytes | Peak RSS, KiB | Elapsed, seconds | Full aggregate amount |
| ---: | ---: | ---: | ---: | ---: |
| 100,000 | 7,900,088 | 8,960 | 1.345 | 200,000 |
| 1,000,000 | 79,000,088 | 9,216 | 14.228 | 2,000,000 |

These measurements demonstrate near-flat memory for these fixtures, not a universal
maximum RSS or a latency guarantee. Tests also cover a 100,000-row report with
100,000 metadata arrays, one-row page versus full totals/backup, original-schema
upgrade without changing TEXT JSON, old migration/roundtrip tests, concurrent
snapshot invalidation, malformed-data publication failure, full save rollback after
an injected index failure, explicit filter/size limits, and numeric overflow.
The initial test-first run failed to compile because the new APIs did not exist;
the implemented suite passes 21 tests. The Linux harness does not validate the
Tauri/Windows GUI build. Clippy is unavailable in the supplied Rust toolchain.
