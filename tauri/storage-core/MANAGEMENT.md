# D2b bounded settings and management domain

Production `management.rs`, `management_tree.rs` and `management_reports.rs` are
compiled by the GUI-independent storage-core harness. These are Rust APIs only:
no command registration, bridge changes or frontend activation. They never call
legacy full `load()`/`save()` to implement a missing operation. The old snapshot
API remains for the old application and must not receive native header/empty caches.

## Preparation and compatibility

Start with `management_revision()`. It initializes additive schema and a small
migration marker only when no unmigrated legacy JSON/config files exist. If those
files exist without a migration marker, it fails explicitly: management startup
cannot silently enter the old full-file migration. Existing files remain intact.
All management calls use this bounded connection path. The older `revision()` /
full loader's explicit migration behavior is unchanged and is not a native fallback.

Call `management_index_settings(revision, job)` to stream the complete current
settings JSON into a validated disk model. For report cards/navigation, use the
existing bounded D1 `index_report(id, revision)` first when its raw manifest is
absent, then `management_index_report(id, revision, job)` for administrative
metadata. Metadata indexing rejects an absent raw manifest; it does not implicitly
run another uncancellable backfill. C1 saves/imports already produce raw indexes.
D1 header pages can enumerate work for the indexing worker. Missing indexes remain
explicit errors, never zeros or silently partial lists.

Original reports and other kv data remain intact. Settings may be original TEXT
or UTF-8 BLOB. Settings publication writes UTF-8 BLOB; legacy `read_key`/explicit
full `load()` now accept both TEXT and BLOB consistently. That full response is
still unbounded and is never used by the new APIs. An older String-only native
executable fails on BLOB settings instead of loading a partial/default settings
snapshot; no downgrade-to-TEXT conversion is performed. No Windows executable or
WebView2 proof is claimed.

## Full-fidelity JSON model

`analysis_groups` is not the management model: it stores only effective first
membership. The management tree retains every original ordered group/catalog
entry and every membership, including empty groups, duplicate IDs, duplicate
work strings, losing memberships and unknown nested metadata. Existing object
field order and all array order are preserved; whitespace/number spelling can
be canonicalized on serialization.

Additive schema:

```sql
management_nodes(handle INTEGER PRIMARY KEY, parent INTEGER, ordinal INTEGER,
                 field TEXT, kind TEXT, value TEXT)
-- children index: (parent,ordinal,handle)
management_settings(root INTEGER,groups_node INTEGER,catalog_node INTEGER,orgs_node INTEGER)
management_groups(handle INTEGER PRIMARY KEY,ordinal INTEGER,id TEXT,name TEXT,
                  works_node INTEGER,members INTEGER,unique_members INTEGER)
management_members(handle INTEGER PRIMARY KEY,group_handle INTEGER,ordinal INTEGER,work TEXT)
-- indexes: (group_handle,handle), (work,group_handle)
management_catalog(handle INTEGER PRIMARY KEY,ordinal INTEGER,work TEXT,organization TEXT)
-- index: (work,organization,handle)
management_orgs(ordinal INTEGER PRIMARY KEY,name TEXT)
management_report_meta(report_id TEXT PRIMARY KEY,amount REAL,saved_amount REAL,
                       warnings INTEGER,shared_source_artifact INTEGER)
management_report_orgs(report_id TEXT,ordinal INTEGER,org TEXT,
                       PRIMARY KEY(report_id,ordinal)) WITHOUT ROWID
```

A recursive streaming serde visitor stages one JSON node at a time in SQLite;
there is no whole settings/group works/catalog Value or Vec. TEXT/BLOB input is
read through an 8 KiB buffer and the 64 KiB raw-token guard. Settings structure
requires an object with groups/catalog/orgNames arrays; group id/name and work
members must be strings, catalog work must be a string and organization is an
optional string. Up to 100 bounded org names are allowed. Unknown nested JSON
values remain nodes, including null/bool/number/string/object/array. Duplicate
recognized fields fail explicitly instead of choosing a version arbitrarily;
unknown duplicate fields remain in original order. Nesting above 120 or raw
string/numeric tokens above 64 KiB fail. New strings must also fit the serialized
raw-token budget, so control-character escaping cannot create unreadable output.
Unsupported/corrupt data remains stored and cannot publish a partial manifest.

Indexing parses/validates in a private SQLite stage under a source read snapshot,
then publishes the full tree and projection manifests by revision-checked snapshot
upgrade. Source writers prevent stale publication. Settings triggers invalidate
the settings manifest in the writer's transaction; stale nodes are unreachable
until a complete backfill replaces them. Existing analysis triggers independently
invalidate first-match indexes. Report writers invalidate only affected report
management metadata/org rows alongside D1/D2a indexes. Index maintenance itself
does not advance the data revision.

## Reads, filters and cursors

All pages and mutation batches require size 1..=500. Labels/IDs are at most 64 KiB
and must fit the raw JSON string-token budget; search text is at most 512 UTF-8
bytes. Every read checks its expected revision in one SQLite snapshot. Reuse
cursors only with the same revision and arguments. Revisions/node handles are
Rust i64; a future IPC layer must encode them losslessly, e.g. decimal strings.

| API | Result and cursor |
| --- | --- |
| `management_bootstrap(revision)` | Only bounded org names, revision, complete group/member/catalog/report counts and count of report summaries with valid raw+metadata indexes. No settings/history payload. |
| `management_group_count(revision, search)` | Complete matching definition count. |
| `management_group_page(revision, search, after_handle, size)` | Saved definition order, opaque handle, original id/name, full member and unique-member counts. Cursor = last handle. Duplicate IDs remain separate definitions. |
| `management_member_page(revision, group_handle, after_handle, size)` | Literal ordered memberships including duplicates; cursor = last member handle. |
| `management_catalog_count(revision, search, organization)` | Complete filtered catalog entry count, including duplicate entries. |
| `management_catalog_page(revision, search, organization, after_handle, size)` | Saved order, handle, work and optional org; cursor = last entry handle. |
| `management_node_page(revision, parent_handle, after_(ordinal,handle), size)` | Full-fidelity immediate children, including unknown fields; parent `None` returns the root. Each row includes field/kind/value/complete child count. Traverse containers through bounded child pages, never one giant group definition. |
| `management_work_summary(revision, search, organization)` | Complete unique-name total/grouped/ungrouped counts. |
| `management_work_page(revision, search, organization, after_work, size)` | Exact unique work strings from reports, catalog and all group memberships; BINARY work cursor. |
| `management_report_summary(revision, filter)` | Full filtered report/row/warning counts, raw detail sum, first/latest start and distinct start count. |
| `management_report_page(revision, filter, after_ReportCursor, size)` | Headers, complete row count/raw amount, separately saved report amount, warning count, explicit org count, shared-source diagnostic flag. Order `(start DESC,id BINARY ASC)`; row `.cursor()` provides position. |
| `management_period_page(revision, filter, after_start, size)` | Distinct actual period starts, end and report count, descending. One report type is required; page size one after latest supplies previous. |
| `management_report_org_page(revision, report_id, after_ordinal, size)` | Original explicit org list, including order/duplicates; cursor = ordinal. |

Node `kind` is object/array/string/number/bool/null. Container values are absent;
string values are decoded, numbers/bools use their JSON scalar spelling. Node
pages expose every unknown nested value without materializing a whole entry.
Opaque handles identify definitions, not business IDs, so duplicate group IDs do
not make edits ambiguous. Existing order is preserved under edits; creation
appends. Arbitrary reordering is not yet an API.

Work discovery is a disk SQL union, requiring complete raw report indexes.
Unscoped discovery includes report works, catalog works, and *all* memberships.
With an exact organization filter, report/catalog entries are filtered by that
org; group-only names with no org association are omitted. Grouped counts test
exact membership across every definition, including losing memberships.

Search is literal **case-sensitive substring** matching and cursors use SQLite
BINARY order. This deliberately does not claim parity with the old Russian
locale/case folding/prefix-ranked `searchNames` UI. Native browsing must not be
activated until its ordering/search contract is ported or explicitly chosen.
Unfiltered coverage, ordered definitions/members and report-card scalars have
actual old-JS golden coverage.

Report filters allow one optional type, exact explicit organization, and inclusive
start-date from/to bounds. Dates are report starts; no proration is performed.
These are administrative raw report summaries: duplicate/overlapping reports
remain separate, and raw amount sums are distinct from `saved_amount` and strict
D2a completeness/quality-exclusion math. Saved quality/calendar remain separate
future APIs. Report summary/navigation reject unindexed selected-type history
before org/date filtering so an unindexed report cannot disappear behind a filter.

## Owned settings mutation sessions

`management_begin_settings(revision, max_bytes, Arc<ManagementJob>)` returns an
owned `SettingsEditor` with a private disk snapshot. It reads only the complete
normalized tree under one snapshot. No reports are accepted by this type or by
its publication API. Keep a clone of the job outside a blocked worker so cancel
can interrupt an active SQLite operation; no borrowed SQLite callbacks or unsafe
lifetime hooks are installed. `records_processed()` counts JSON input nodes parsed,
not a total completion percentage.

Supported editor operations:

- `create_group(id,name) -> handle`, `rename_group(handle,name)`,
  `delete_group(handle)`.
- `append_members(group,&[String])`, `remove_members(group,&[member_handle])`,
  and session `member_page(group,after,size)`.
- `create_catalog(work,organization) -> handle`,
  `update_catalog(handle,work,organization)`, `delete_catalog(&[handle])`.
- `set_scalar(root_field, ManagementScalar::{String,Number,Bool,Null})` for scalar
  settings. It cannot replace nested containers and rejects orgNames/groups/catalog.

Mutations retain unrelated fields/order and act literally: appending does not
deduplicate or remove losing memberships, and deleting one handle does not delete
other definitions sharing its ID. The old UI's automatic membership exclusivity,
empty-group pruning and locale name policies are not silently applied. A caller
can compose bounded operations in one session for a large edit. Catalog edits do
not rename historical report works or groups. Organization renaming is expressly
unavailable until an atomic all-report streamed rename exists. Unknown nested
metadata can be read and is preserved; arbitrary nested-field editing/upload and
reorder APIs are later, rather than accepting an unbounded settings payload.

`management_commit_settings(editor)` consumes the session. It fully validates the
staged tree/projections, serializes all nodes one at a time to a new private file,
computes its SHA-256 and size, then opens a live publication transaction and checks
the captured revision. It atomically DELETEs/INSERTs **only kv['settings']** with
zeroblob and copies UTF-8 in 64 KiB chunks, verifying copied size/hash. No whole
TEXT cast/UPSERT old-JSON payload is needed. Full normalized indexes and manifests
publish in the same transaction. Main writer lock is held for BLOB/index copying,
not long JSON parsing, cloning or serialization. An external commit makes the
session stale; failures/cancellation/SQLite FULL roll back data, revision and
indexes. Report/config/migration/receipt rows remain outside this write domain.

Sessions allow 4 active leases per storage root, each with a requested budget
4 MiB..16 GiB. SQLite max_page_count reserves one third for the stage DB; generated
JSON is bounded by another third and i32 BLOB size (actual SQLite limits may be
lower). Stage directory sizes are checked before commits. Main/temp/attached cache
targets are 2 MiB and temp storage is FILE. This is not a filesystem reservation:
SQLite sorter temp files and the live publication WAL/rollback needs can consume
additional disk. Failures stay explicit and preserve the last committed data.

Each directory has a random 128-bit name, owner marker and exclusive fs2 lease;
Unix directories are mode0700. Drop/abort removes only that stage. Beginning a
new stage or `management_cleanup_sessions()` recovers owned unlocked stages from
crashed processes and skips live leases/unknown directories; scans stop at 1000
entries with an error. Restart discards abandoned edits, not resumes them. Cleanup
and arbitrary adversarial filesystem replacement races are not universally solved.
Successful publication returns revision plus a separate cleanup_error; late
cancellation cannot turn a committed receipt into failure. Durable request replay/
receipts and actor/IPC handle maps remain for the worker integration slice.

## Targeted delete/clear and C3 integration gate

`management_delete_reports(revision, &[id], job)` accepts 1..=500 bounded explicit
IDs; duplicate IDs are ignored and an unknown ID fails the whole operation.
`management_clear_reports(revision, job)` is the explicit whole-history operation;
an empty UI cache never means clear. Both compare revision in their write
transaction, retain unrelated reports and all settings/config/kv/import receipts,
and invalidate affected derived indexes through the existing writer triggers.
Cancellation/SQLite errors before the serialized commit decision roll back.

Removal requires current management metadata for every report being removed;
unindexed/corrupt/unsupported reports must be indexed/repaired explicitly first.
The C3 fields `sourceDiagnosticsReport`, `sourceDiagnostics` and `sourceIssues`
were inspected in prepared.rs440–465: any selected report containing any of these
is marked `shared_source_artifact`, and deletion/clear **fails closed**. D2b does
not preserve/reanchor a shared diagnostic anchor. The C3 merge/activation gate
must cover targeted deletion **and import replacement/source deletion** before
those flows can remove anchors while other reports reference them. The current
base is C1/D2a, and this guard does not claim a complete C3 source-quality index or
replacement lifecycle. UI/IPC activation remains off.

## Verification and scope

Tests execute the actual old standalone management functions in a Node VM for
coverage/definition/member/report metadata comparisons. Other tests cover nested
unknown fields, duplicate IDs and memberships, empty groups, page-one vs complete
counts/unions, scalar-only restrictions, targeted report-safe publication, BLOB
compatibility, stale/concurrent publication, mid-parse cancellation, live/stage
SQLite FULL, trigger faults, restart/live lease cleanup, legacy-file migration
gating, raw-index prerequisites and C3 fail-closed removal. Full D1/D2a/C1 storage
and import-core suites remain required. The standalone/Electron paths stay unchanged.

`make_management_fixture.py` runs before the independently measured `management`
example. It creates one TEXT settings object with N catalog entries (nested unknown
metadata), one N-member group, an empty group with duplicate ID, and one original
UTF-8 BLOB report. The measured executable streams indexes, fetches one-row pages
with complete counts/unions, clones an owned settings session, appends 501 members
in bounded batches, patches catalog/scalar metadata, publishes BLOB settings and
checks full counts/report retention. Linux `/proc/self/status` VmHWM is in-process;
fixture generation is excluded. Exact measurements and commits are recorded in
`/workspace/native-work-ledger/d2b-report.md`. These fixtures are not a universal
RSS bound, disk guarantee, performance promise, or Windows validation.
