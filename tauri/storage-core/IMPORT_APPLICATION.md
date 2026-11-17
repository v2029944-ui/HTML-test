# C1/C2 prepared import, review and atomic application

This is a synchronous Rust foundation, compiled by the GUI-independent storage
harness. It is **not registered as a Tauri command** and does not switch production
import/UI paths. Ordinary B2a triples are the current converter. Daily-wide and full multi-source
UI remain separate work. C2 adds backend quality review and whole-source replacement.

## API and ownership

Initialize normal storage first (normal startup or `Storage::revision()`). Import
preview/application deliberately open only an already initialized database; they
do not perform legacy JSON migration, create missing databases, or call `load()`.

1. `StageArea::ingest(path, limits, cancel, progress)` binds the canonical source
   path and SHA256 privately. Its existing public `source_hash` display copy cannot
   override this binding. The private stage name has 24 random characters.
2. Convert with `Stage::convert_legacy`. `Stage::prepare(self, cancel, progress)`
   consumes the complete stage and returns an opaque `PreparedImport`. Its default
   mode is `ImportMode::Manual`; `prepare_with_mode(ImportMode::Folder, ...)`
   explicitly marks folder-owned documents. Mode is private and artifact-bound. There is no
   constructor from caller-selected files and no restore/resume of incomplete
   stages. A private IMMEDIATE staging transaction holds one conversion snapshot
   throughout document preparation. It does not open the main database.
3. `Storage::preview_import(&prepared)` or
   `preview_import_with_cancel(&prepared, &AtomicBool)` checks source, manifest
   database hash and every complete document. A consistent main read snapshot
   captures the monotonic revision and computes conflicts over **every report**.
   `ImportPreview` has private operation/database/revision/count fields and read
   accessors `revision()`, `counts()` and `quality()`. Generation and full-source
   target-plan hash are also private snapshot bindings.
4. `Storage::apply_import(&prepared, &preview, ImportConfirmation, &ImportJob,
   progress)` revalidates before opening an IMMEDIATE main transaction. The
   transaction rechecks receipt, revision and complete conflict counts, writes
   documents/normalized rows/manifests, and commits one durable receipt. No full
   history/report Value or String is created or returned. `ImportConfirmation`
   has `replace_exact`, `accept_cross_type`, `replace_source` and `accept_quality`,
   all false by default. Invalid/skipped/mismatch/other notices or report mismatch
   flags require `accept_quality`; acknowledgment cannot make an empty or incomplete
   report valid. Folder replacement uses its own `replace_source` acknowledgment.
5. `committed_import(operation_id)` retrieves a persisted receipt after restart.
   Reconfirming a committed artifact returns that receipt even after the source
   changes, stage files disappear, or cancellation arrives. A new import of the
   same period is a new operation and needs explicit conflict replacement.
6. `PreparedImport::discard(self)` reports cleanup separately. Ordinary Drop is
   best effort. StageArea restart recovery, under its existing exclusive OS lock,
   discards unfinished/leftover owned stages. A cleanup failure cannot reverse or
   turn a committed receipt into a retryable failed import.

`ImportReceipt` contains only `operation_id`, exact committed `revision`,
`reports`, `rows`, and `replaced`. Revision is an i64; a future IPC adapter must
preserve its precision (for example, decimal strings). The backend works with
opaque server-owned handles; callers do not submit arbitrary replacement paths,
report lists, IDs, or revision/count edits.

## Documents and bounds

Each document uses a predictable `report-{ordinal}.json` leaf inside its owned
unpredictable private directory, opened with `create_new`. Global live IDs are
`native-{SHA256(random stage path)}-{ordinal}` and remain fixed across preview
and repeated confirmation. The prepared SQLite manifest stores ordinal, local
ID, live ID, type, dates, row count, byte length and SHA256. The opaque handle pins
that entire database's hash and complete report/row/quality counts. Each prepared
report stores its complete scalar quality summary as well. Altered/missing files,
altered normalized rows, incomplete conversion and interrupted preparation fail.

Documents stream through a 64 KiB writer/hash/cancellation buffer. Report headers
are read one at a time; rows, issues and controls use pages of at most 100 records;
organizations are at most four. Persisted string fields are preflighted with
`octet_length` before deserialization and capped at 64 KiB. Serialized raw JSON
string tokens, including quotes/escapes, are capped at 64 KiB, matching D1's
backfill guard. Escaped data can hit that cap before decoded text reaches 64 KiB.
The existing `Limits.stage_bytes` (default 2 GiB) is shared with documents, not
increased. Budget checks include actual files/journal and projected dirty SQLite
pages; 64 KiB is reserved per document for its small manifest update. Oversized
or exhausted stages fail and are discarded. SQLite's own document-length/disk
limits can also reject application safely.

Required legacy fields `id`, `type`, `start`, `end`, `rows`, row
`organization/work/count/amount`, root `count/amount`, `organizations`,
`hasMismatch`, `warnings`, and `totals` are retained. Additional fields include
`desktopSource`, canonical `desktopFolder`, `sourceHash`, `sourceSheet`,
`sourceMapping`, `rowCount`, `organizationTotals`, `issueCount`, and `controlCount`.
`issues`, `totals`, and row provenance keep B2a's full structured source rows,
exact cells, expected/actual values, and nullable controls. Warning reasons and
structured provenance remains native B2a representations; diagnostics additionally
serialize legacy-compatible `type`, `row`, `cells`, `expected` and `actual` aliases
without dropping the native fields; **exact legacy
warning prose and quality-screen presentation parity are not claimed**.
Empty reports and overlapping periods within one prepared artifact fail closed.
Future converters can reuse the document/preparation/application contract, but
must supply the same complete report APIs and define additional metadata needs.

## Main SQLite storage and downgrade tradeoff

Original `reports(..., json TEXT NOT NULL)` and `kv` schemas remain intact.
Existing TEXT documents remain TEXT and byte-for-byte unchanged when unrelated.
New import documents use SQLite's dynamic BLOB storage class in `reports.json`:
`zeroblob(length)` followed by 64 KiB incremental Blob writes inside the final
transaction. No `CAST(zeroblob AS TEXT)`, giant bound string, or full-document
`INSERT ... SELECT` is used. UTF-8 JSON bytes are preserved.

`Storage::load()` now accepts both `ValueRef::Text` and `ValueRef::Blob` bytes.
It remains the explicit legacy full-history API and is not itself bounded; import
never invokes it. D1 Blob backfill already accepts both storage classes.
**Older EXEs with String-only load reject new BLOB documents.** This is an
unsupported downgrade, not silent backward-readable storage. Existing
`desktopBlocked` startup behavior blocks writes after read failure, preventing
silent partial loading/data replacement. No downgrade/export migration is added.

The additive main schema includes the receipt plus generation and ownership manifests:

```sql
CREATE TABLE import_receipts (
  operation_id TEXT PRIMARY KEY,
  revision INTEGER NOT NULL,
  reports INTEGER NOT NULL,
  rows INTEGER NOT NULL,
  replaced INTEGER NOT NULL
);
CREATE TABLE import_generation (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1), generation TEXT NOT NULL
);
CREATE TABLE report_source_ownership (
  report_id TEXT PRIMARY KEY, source_name TEXT NOT NULL,
  folder TEXT NOT NULL, mode TEXT NOT NULL
);
```

Generation starts with 32 random bytes encoded as hex. INSERT/UPDATE/DELETE report
triggers invalidate ownership transactionally, including `INSERT OR REPLACE` when
SQLite does not run its implicit DELETE trigger. Native publication writes its
new manifest entry after INSERT. Settings reads/writes do not invalidate ownership.
Legacy discovery uses a private TEMP disk table and does not mutate live reports
or ownership during preview.

Documents, all `report_rows`, all `report_row_index` manifests, and this receipt
share one transaction. Existing D1 triggers advance revision/invalidate replaced
indexes normally; import never assigns revision. The transaction reads the exact
new token before inserting its receipt. Schema/trigger tampering and direct
external edits of private stage/read-model/receipt tables are outside the
supported writer contract; ordinary reports/kv writers are detected by revision.

## Conflict and cancellation rules

Manual mode preserves C1 behavior: matching type/start/end needs
`replace_exact=true`; same-type overlaps with non-identical periods fail. Every
cross-type overlap counts as a prepared/existing pair and needs
`accept_cross_type=true`; totals are never distributed over days.

Folder mode is explicit at preparation. It identifies one canonical source folder
plus exact filename, and atomically removes ALL its old reports, including old days
absent from the new file. `counts().replacements` and receipt `replaced` count every
removed report; `new_reports` counts prepared periods without an exact old match.
An existing target set needs `replace_source=true`. Manual artifacts reject
`replace_source` rather than imply ownership. `replace_exact` is rejected in
folder mode; another source's same-type overlapping period always blocks it.
A different folder's same filename remains untouched. The complete target set and
all conflicts are rebuilt under the final IMMEDIATE transaction and compared with
the preview's revision, generation, artifact binding and plan hash.

New native documents explicitly serialize `sourceImportMode` as `manual` or
`folder`, and their ownership is indexed. For unindexed legacy reports with the
same filename, metadata is read through incremental SQLite Blob + bounded serde
visitors. Unknown arrays are skipped; all raw JSON tokens retain D1's 64 KiB cap,
including ignored strings/keys. No JSON1 extraction/full report Value is used.
Legacy filename + absolute canonicalizable folder without `sourceHash` may be
classified folder-owned. Explicit manual mode, or absent mode with `sourceHash`
(C1's manual format), is NEVER silently adopted. Missing/malformed folder identity
or ambiguous/duplicate source fields block automatic replacement. There is no
unsafe adoption bypass. Missing old folder paths may be compared as absolute
normalized paths; resolvable paths are canonicalized. Legacy metadata is rescanned
on each snapshot rather than changing live documents during preview.

Prepared review does not reopen document JSON. `reports(after, limit)` pages
1..100 reports; `next_report` remains compatible. `PreparedReport.info` retains
ordinal, mapping, source sheet, totals and mismatch flag. `PreparedReport.quality`
and `PreparedImport::quality()` include complete reports/rows/issues/invalid/
skipped/mismatches/other_notices/controls/mismatch_reports counts. `issues` and
`controls` page 1..100 records; `organizations` returns at most four. Nullable
controls, source cells and expected/actual values remain intact. Quality counts
are accumulated while preparing and frozen in the private manifest, so page one
cannot be mistaken for all diagnostics. Review pages check the frozen database's
length/mtime before accessing it; preview/apply additionally verify its full hash.
Private staging files are not an adversarial filesystem security boundary.

The Storage gate serializes an instance, and SQLite's IMMEDIATE transaction plus
in-transaction revision check serialize separate instances/processes. No main
transaction spans XLSX parsing/preparation. Large application still holds the
write lock while copying/indexing/checking source and stage, so measured lock
latency is a remaining integration concern.

Gate acquisition polls cancellation every 5 ms. SQL work uses a progress handler
(1000 SQLite VM instructions), backed only by an owned Arc flag; a scoped monitor
bridges the compatible borrowed AtomicBool API every 2 ms. RAII clears hooks on
success, error and panic. History scans use 100-row rowid chunks, legacy Blob reads
check cancellation, and writer acquisition retries 10 ms busy waits for at most 5 s.
These are cooperative mechanisms, not universal wall-clock latency guarantees.

`Storage::invalidate_import_previews_after_restore()` rotates persisted generation
without relying on revision changing. A future restore coordinator MUST exclude
all database workers across replacement/initialization/rotation, initialize restored
older schemas, rotate generation, and only then release workers. No restore command
is registered by this work. Pending previews fail even if restored revision/data
match. This API does not atomically solve arbitrary out-of-contract filesystem
replacement races at the same pathname; canonical path binding alone cannot do so.

`ImportJob` can be shared by reference/Arc with the future worker. `cancel()` and
commit serialize on its mutex. Cancellation accepted before the commit boundary
rolls back; commit holds that mutex through SQLite commit and marks committed
before releasing it. Late cancel returns false, and success is never reported as
cancelled. Use one job per apply attempt. Chunk reads/writes and row pages check
cancellation. Callbacks run only before commit; no fallible work/callback follows
successful commit. Source/index hashes are checked again immediately before
commit, but there is **no claim that a filesystem mutation after the last hash
observation is atomic with SQLite commit**.

RAII transactions roll back errors, normal unwinding and process termination.
A panicking callback poisons that Storage instance's mutex; reopening Storage is
needed, while the database remains intact. Production worker/UI recovery remains
later integration work. Physical disk-space reservation is not provided.

## Verification and reproduction

```sh
export CARGO_HOME=/workspace/rust/cargo
export RUSTUP_HOME=/workspace/rust/rustup
export CARGO_TARGET_DIR=/workspace/HTML-native-b/tauri/import-core/target
/workspace/rust/cargo/bin/cargo test --offline --manifest-path tauri/storage-core/Cargo.toml
/workspace/rust/cargo/bin/cargo test --offline --manifest-path tauri/import-core/Cargo.toml
/workspace/rust/cargo/bin/cargo build --offline --manifest-path tauri/storage-core/Cargo.toml --example prepare_apply
python3 tauri/storage-core/examples/make_import_fixture.py /tmp/c1-probe 50000 512
"$CARGO_TARGET_DIR/debug/examples/prepare_apply" /tmp/c1-probe
```

Use fresh fixture directories. Python creates only the XLSX fixture and exits
before the measured Rust worker. The worker reports its own `/proc/self/status`
VmRSS/VmHWM after ingest/conversion, preparation, preview and application. It
checks a one-row page against the complete SQL aggregate, without `load()`.
`lock_before_commit_ms` measures entry to the final transaction through the last
precommit callback; it excludes commit itself and includes lock acquisition wait.
`apply_ms` additionally includes pretransaction validation and commit.

The C1 baseline storage harness passed 40 tests, plus one ignored helper that is actually
invoked by the process-death parent test. Native import-core passes 26 tests.
C1 tests cover read-only preview, initialized-storage boundary, complete multiple
report counts, database/source binding, full UTF-8 load, old TEXT preservation,
BLOB backfill, full normalized aggregates/SQLite backup, explicit replacement,
cross-type acceptance, same-type overlap rejection, source/manifest/file damage,
new conflicts, kv/report revision changes, empty/invalid stage, disk/token budgets,
structured diagnostics, cancellation before/mid-copy/precommit, late cancel and
reconfirmation, private snapshot, independent-instance race, real SQLITE_FULL,
index-write failure, read failure, callback panic, process kill/reopen/stage
recovery, and postcommit cleanup failure.

Final Linux debug-build probes (fixture generation excluded; in-worker VmHWM):

| Rows | JSON bytes | Prepare ms | Preview ms | Apply ms | Before-commit lock ms | Prepare peak KiB | Final peak KiB |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 679,612 | 132 | 34 | 162 | 76 | 6,712 | 8,852 |
| 50,000 | 34,414,263 | 5,973 | 1,778 | 5,969 | 3,543 | 7,464 | 9,712 |

Ingest plus conversion took 340 ms and 14,282 ms respectively, before those
phases. Both use default ingestion limits and 512-byte work labels. The complete
aggregates returned 1,000/50,000 rows and amount 2,000/100,000, while page one held
one row. These are bounded-memory probes, not universal RSS/latency limits.
No physical ENOSPC, Windows/WebView2, Tauri GUI build, cold-cache benchmark, release,
signing, or production UI/backup/export migration is claimed. Main Rust minimum
is now 1.82 to match its new import-core dependency.

## C2 verification

C2 adds focused real-SQLite coverage for quality acknowledgment and complete
multi-page diagnostics/nullable controls, source shrink and all-source receipt
counts/idempotence, source rollback after quality/read/write/cancel/source/revision
failure, other-folder isolation, unknown/malformed legacy identity, explicit C1
manual classification, REPLACE-writer ownership invalidation, restore generation,
Storage-gate/writer waits, long-SQL hook cleanup, 50,000-source cancellation, and a
32 MiB ignored legacy metadata array. Measurements and exact final suite counts
are recorded in `/workspace/native-work-ledger/c2-report.md`; Linux debug probes
are evidence for their fixtures, not platform or universal memory/latency promises.
Whole product D2/E/Windows integration remains unfinished.
