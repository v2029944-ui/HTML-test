# C1 prepared import and atomic application

This is a synchronous Rust foundation, compiled by the GUI-independent storage
harness. It is **not registered as a Tauri command** and does not switch production
import/UI paths. Ordinary B2a triples are the only currently supported prepared-apply format. The XLSX core can also convert daily-wide, but C1 explicitly rejects it at preparation time until provenance, source-wide diagnostics and proven zero days have a compatible lifecycle. Daily-wide,
full multi-source UI, and whole-source/folder replacement remain later work.

## API and ownership

Initialize normal storage first (normal startup or `Storage::revision()`). Import
preview/application deliberately open only an already initialized database; they
do not perform legacy JSON migration, create missing databases, or call `load()`.

1. `StageArea::ingest(path, limits, cancel, progress)` binds the canonical source
   path and SHA256 privately. Its existing public `source_hash` display copy cannot
   override this binding. The private stage name has 24 random characters.
2. Convert with `Stage::convert_legacy`. `Stage::prepare(self, cancel, progress)`
   consumes the complete stage and returns an opaque `PreparedImport`. There is no
   constructor from caller-selected files and no restore/resume of incomplete
   stages. A private IMMEDIATE staging transaction holds one conversion snapshot
   throughout document preparation. It does not open the main database.
3. `Storage::preview_import(&prepared)` or
   `preview_import_with_cancel(&prepared, &AtomicBool)` checks source, manifest
   database hash and every complete document. A consistent main read snapshot
   captures the monotonic revision and computes conflicts over **every report**.
   `ImportPreview` has private operation/database/revision/count fields and read
   accessors `revision()` and `counts()`.
4. `Storage::apply_import(&prepared, &preview, ImportConfirmation, &ImportJob,
   progress)` revalidates before opening an IMMEDIATE main transaction. The
   transaction rechecks receipt, revision and complete conflict counts, writes
   documents/normalized rows/manifests, and commits one durable receipt. No full
   history/report Value or String is created or returned. `ImportConfirmation`
   has `replace_exact` and `accept_cross_type`, both false by default.
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
that entire database's hash and complete report/row counts. Altered/missing files,
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
structured property names remain native B2a representations; **exact legacy
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

The only additive main schema is:

```sql
CREATE TABLE import_receipts (
  operation_id TEXT PRIMARY KEY,
  revision INTEGER NOT NULL,
  reports INTEGER NOT NULL,
  rows INTEGER NOT NULL,
  replaced INTEGER NOT NULL
);
```

Documents, all `report_rows`, all `report_row_index` manifests, and this receipt
share one transaction. Existing D1 triggers advance revision/invalidate replaced
indexes normally; import never assigns revision. The transaction reads the exact
new token before inserting its receipt. Schema/trigger tampering and direct
external edits of private stage/read-model/receipt tables are outside the
supported writer contract; ordinary reports/kv writers are detected by revision.

## Conflict and cancellation rules

Only **manual exact-period replacement** exists. Matching type/start/end needs
`replace_exact=true`; same-type overlaps with non-identical periods fail. Every
cross-type overlap is counted as a prepared/existing report pair and requires
`accept_cross_type=true`. Counts and totals are never distributed over days.

There is no automatic filename/folder/whole-source replacement option. New JSON
retains filename and canonical folder, but legacy `desktopFolder` is not scanned
or assumed. Importing a smaller source therefore must not be advertised as
whole-source replacement; obsolete old periods require a future coherent C2
source-identity index/plan. The same filename in different folders never triggers
automatic replacement. Explicit manual period replacement can intentionally
replace an exact period regardless of source, as its separate flag specifies.

The Storage gate serializes an instance, and SQLite's IMMEDIATE transaction plus
in-transaction revision check serialize separate instances/processes. No main
transaction spans XLSX parsing/preparation. Large application still holds the
write lock while copying/indexing/checking source and stage, so measured lock
latency is a remaining integration concern.

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

The full storage harness passes 40 tests, plus one ignored helper that is actually
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
