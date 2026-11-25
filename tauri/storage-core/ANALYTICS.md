# D2a bounded analysis API

This additive backend is compiled by `storage-core` from production
`src-tauri/src/analytics/analytics.rs`. No Tauri commands or frontend changes are included.
D1 `aggregate()` remains a raw-detail aggregate; use the `analysis_*` APIs below
for report completeness, overlap exclusions, grouped metrics and changes.

## Preparation and persisted schema

Initialize `Storage`, capture `revision()`, enumerate headers in bounded D1
`report_page` calls, then call `analysis_index_report(id, revision)` for each
report of the type being analyzed. Call `analysis_index_settings(revision)` once.
A query rejects any unindexed history of its selected type, including history
outside the selected dates; it never silently interprets an absent manifest as
zero. Index maintenance does not advance the data revision.

Additive tables (original reports, kv, JSON files and backup are preserved):

```sql
analysis_reports(report_id TEXT PRIMARY KEY, valid INTEGER, issue TEXT, warned INTEGER)
analysis_orgs(report_id TEXT, org TEXT, PRIMARY KEY(report_id,org)) WITHOUT ROWID
-- secondary index: (org,report_id)
analysis_settings(singleton INTEGER PRIMARY KEY CHECK(singleton=1))
analysis_groups(work TEXT PRIMARY KEY, key TEXT, name TEXT) WITHOUT ROWID
analysis_filters(id TEXT PRIMARY KEY, revision INTEGER, kind TEXT, sealed INTEGER)
analysis_filter_items(id TEXT, item TEXT, ordinal INTEGER, PRIMARY KEY(id,item)) WITHOUT ROWID
```

Report backfill uses readonly SQLite incremental blobs for both original TEXT
and UTF-8 BLOB JSON, an 8 KiB reader and the D1 64 KiB raw-token guard. It streams
one row/organization at a time into FILE temp tables. Required root metadata must
match the original columns. Duplicate recognized fields, malformed JSON, trailing
content, excessive nesting and oversized raw tokens fail without publication.
Invalid row values or period bounds instead publish an excluded-report quality
manifest (`invalid_rows` / `invalid_bounds`); the issue page exposes `invalid`
and the detailed `reason_code`.
Unknown fields are consumed with `IgnoredAny`, including large arrays.

Settings backfill likewise streams the settings object, each group and each work
list into FILE temp tables, even if `works` precedes `id`/`name`. Catalog and
irrelevant settings fields are streamed and ignored, not loaded as a Value.
First group in settings order wins exact, case-sensitive work-string membership.
Group id/name/work must be strings. Duplicate group ids with different display
names are explicitly unsupported: indexing fails, rather than selecting a name
arbitrarily. Duplicate ids with the same name and overlapping membership work.
A catalog browsing/index API is a later frontend requirement, not a metric input.

Parsing holds a read snapshot and publishes all normalized data/manifests in its
transaction after checking the captured revision. A concurrent writer prevents
snapshot upgrade/publication. Long parsing and queries do not hold `Storage.gate`;
the write lock is needed only for publication. SQLite still serializes writes.
Report triggers invalidate analysis manifests/organization indexes; settings kv
triggers invalidate group/settings indexes, in the same transaction as every
normal save, prepared import or external SQL report/kv writer. Rollback restores
everything together. Direct changes to derived tables, triggers or schema are
outside the supported writer contract. Incremental BLOB mutations must belong to
the same transaction as a normal report INSERT/UPDATE, as prepared imports do.

Valid indexed reports reuse D1 normalized detail rows. Old JavaScript ignores
rows with a non-string/missing organization for string organization selections;
analysis preserves that behavior, but withholds the D1 raw-row manifest for these
reports so D1 cannot silently certify a reduced raw index. Invalid reports are
excluded by the analysis manifest irrespective of any existing D1 raw index.

## Bounded disk filter sessions

Large selections do not cross the API as an arbitrary vector:

1. `analysis_filter_create(revision, "organizations" | "names") -> id`.
2. `analysis_filter_append(revision, id, &[String])` in batches of 1..=500.
3. `analysis_filter_seal(revision, id)` makes contents immutable.
4. `analysis_filter_delete(id)` explicitly removes an unused/stale selection.

Each string is at most 64 KiB UTF-8. Duplicate items are ignored. There is no
implicit truncation or total item-count cap: selection size consumes SQLite disk
space. Empty sealed organization sets produce an incomplete range; an absent
names filter selects all descriptors, while an empty names filter selects none.
`AnalysisScope { report_type, organizations: id, names: Option<id> }` must choose
exactly one of day/week/month and correctly typed sealed filters from the expected
revision. Names match the descriptor **display name**, not `work:`/`group:` keys.
Sessions do not advance the data revision and become unusable after a data write.
Callers should delete them after use; automatic expiry/quota/cancellation is later.

## Queries and cursors

All page sizes are explicit 1..=500. Cursor labels/IDs are bounded to 64 KiB;
generated work/group/absent keys allow 64 KiB plus their at-most-seven-byte prefix.
Every call uses one SQLite snapshot and checks the expected revision within it.
Carry the same revision, scope, query, organization and problem filter across
pages. Cursors are typed position tokens, not authenticated scope tokens.
Ordering is SQLite BINARY, stable regardless of localized display sorting.

| Method | Complete result / page order |
| --- | --- |
| `analysis_range_summary(revision, scope, RangeQuery)` | Overall completeness, period/org/value counts, missing count, complete scalar count/amount/average and whole-history quality/import-warning counts. |
| `analysis_range_page(..., org: Option<&str>, after: Option<&str>, size)` | One range's descriptor values; cursor = last key. `None` combines selected organizations. An explicit org must belong to the scope. |
| `analysis_organization_page(..., after: Option<&str>, size)` | Per-org completeness, missing count and complete scalar totals; cursor = last org. |
| `analysis_missing_page(..., after: Option<(org,start)>, size)` | Every expected missing org/period; ordered `(org,start)`. |
| `analysis_issue_page(revision, scope, after: Option<IssueCursor>, size)` | Whole-history invalid/duplicate/overlap issues, ordered `(report_id,org,kind,other_report_id)`; row `.cursor()` creates the next cursor. |
| `analysis_comparison_summary(revision, scope, ComparisonQuery)` | Both complete range summaries, complete union value count, overall count/amount/average changes. |
| `analysis_comparison_page(..., org, after: Option<&str>, size)` | Range-to-range changes by descriptor key, ordered key. Explicitly selected display names absent from both ranges get the old comparison's `absent:name` zero descriptor. |
| `analysis_changes_summary(revision, scope, ChangeQuery)` | All adjacent comparison rows, new/gone/existing counts, missing/import-warning pair counts, quality counts, complete count/amount change measures over **comparable rows**, and ratio-of-sums average change. |
| `analysis_changes_page(..., after: Option<ChangeCursor>, size)` | Every comparable org/period/descriptor row, ordered `(current_start,org,key)`; row `.cursor()` creates the next cursor. |
| `analysis_change_pair_page(..., problems_only, after: Option<ChangePairCursor>, size)` | Every pair, or only missing/import-warning pairs, ordered `(current_start,org)`; row `.cursor()` creates the next cursor. |

`RangeQuery { from, to }` endpoints are inclusive period **starts**, not report end
dates. Expected periods are generated into a disk table one at a time. Monthly
endpoints must be the first; weekly endpoints must differ by a whole number of
seven-day steps. Invalid dates/bounds/shifted endpoints error explicitly. Calendar
overflow errors instead of looping or truncating. Selected weeks/months are never
prorated into days.

Availability is independent of rows: organization presence is the union of
`report.organizations` and detail organizations. An explicitly present org with
zero detail rows is available zero. Missing reports or absent orgs are missing.
Duplicate/overlapping valid reports of the same type for the same org exclude
**both**, before selecting dates. Reports for disjoint orgs do not conflict;
other period types do not conflict. The SQL conflict search bounds both start-date
index endpoints (same start for valid day/month; at most six days earlier for
valid weeks). Each conflicting pair contributes two issue rows, as old JS does;
pathologically many duplicates can still require quadratic diagnostic disk space.

A range is complete only if it has periods, selected orgs and every expected
org/period. Incomplete overall/per-org count, amount and average are null,
including range-page metrics for that scope. Observed descriptor keys and active
flags remain pageable; missing diagnostics remain complete. A complete missing
work is zero via `rangeMetric` semantics: callers rendering a matrix may combine
the global key page with complete per-org key pages and fill absent keys as zero.
Averages are `count > 0 ? amount/count : null`, including negative-count edges.
`active` preserves activity of input rows even if their grouped totals cancel.

`ComparisonQuery { before, after, percent, min_count, min_amount }` compares full
ranges with no duration normalization. Incomplete sides yield null changes and
status. `ChangeQuery { method, from, to, percent, min_count, min_amount }` supports
old `previous` (only to versus one preceding period) and `range` (actual candidate
starts within from..to, with preceding start inside the same interval). Missing
pairs produce diagnostics, never fictitious zero changes. Empty zero-detail
pairs are comparable but have no descriptor rows. Status new/gone/existing uses
net nonzero count or amount. All thresholds must be finite and nonnegative;
thresholds mark `smallBase`, `notable`, `significant`, and do not discard rows.
Count/amount measures include delta, percent of absolute base, negative and the
old flags. Average changes are null if either count is not positive. SQL sums and
Rust f64 calculations are unrounded; overflow errors. Last-bit differences from
ordered JavaScript addition remain possible for adversarial floating-point data.

## Complete exports

`analysis_export_range(..., org, destination)`,
`analysis_export_comparison(..., org, destination)` and
`analysis_export_changes(..., destination)` stream all selected result rows in
keyset order from the same read snapshot. No UI page/cursor enters export. CSV
columns use the API's descriptive keys (not localized frontend captions).
CSV preserves old `analyticsCsv`: UTF-8 BOM, semicolon delimiter, quoted cells,
doubled quotes, CRLF, decimal comma, finite numeric cells, blank nulls, textual
formula escaping including JavaScript's Unicode whitespace rules. Controlled
fixtures compare the bytes with the actual old function, including exponent
boundaries, embedded quotes/newlines, negative numbers and zero averages.

A new sibling staging file is reserved with `create_new`, flushed/synced, then
published with an atomic no-clobber hard link. Existing files/symlinks and a target
created during export are never overwritten; failure removes only staging.
Destination filesystems must support hard links. An external write after the read
snapshot is established can coexist with that consistent old-revision export;
the next call with that old revision fails. The existing complete SQLite backup
continues to include original JSON, every kv value, indexes and all reports.

## Verification and measured scope

```sh
CARGO_HOME=/workspace/rust/cargo RUSTUP_HOME=/workspace/rust/rustup /workspace/rust/cargo/bin/cargo test --offline --manifest-path tauri/storage-core/Cargo.toml
node --test tests/test_department_metrics.cjs
CARGO_HOME=/workspace/rust/cargo RUSTUP_HOME=/workspace/rust/rustup /workspace/rust/cargo/bin/cargo build --offline --manifest-path tauri/storage-core/Cargo.toml --example analysis
python3 tauri/storage-core/examples/make_analysis_fixture.py FRESH_ROOT 1000000 5000
tauri/storage-core/target/debug/examples/analysis FRESH_ROOT
```

The fixture generator finishes before the separately measured executable. It
creates one large original TEXT report with N distinct works and N unknown
metadata arrays; H earlier daily reports alternate TEXT and UTF-8 BLOB, have one
row each, and explicitly include an empty second org. Settings contain N catalog
entries and N/10 groups, with work lists before names/IDs. The Rust probe backfills
through 100-header pages, streams settings, creates disk org filters, obtains a
one-row page plus complete range and previous-change totals, and exports every
range/change row. Linux `/proc/self/status` VmHWM is read inside that executable;
Python fixture construction is excluded. Debug-build measurements are recorded
in the D2a work report. These fixture-specific results establish neither a
universal RSS bound nor a latency guarantee, and test no WebView2/Windows GUI.

This API rebuilds disk query workspaces per call, trading repeat work/disk for
bounded memory and snapshot simplicity. Reusable query sessions, disk reservation,
progress/cancellation, quotas and authenticated cursor handles are future work.
UI integration also needs catalog/name/group browsing pages, optional explicit
organization-matrix pages, localized CSV context, and the usual/regularity/audit/
contribution/structure analysis APIs. No claim of those analyses or the frontend
migration is made here.

## Saved source evidence on demand

`report_quality_page(revision, id, scope, offset, limit)` returns at most 50
issues and an `evidence` summary of the entire selected diagnostic array.
It streams only the selected saved report or its current shared diagnostic
anchor. `evidence.document_id` identifies the document actually read.
No source JSON, schema, indexes, or analytical totals are changed.

`explicit_zero` preserves the saved boolean or unknown (`null`); it is not
fresh validation of an Excel file. `coverage` contains valid declared sheet
bounds, which do not certify source completeness. Logical `sheet_name` is
separate from the existing diagnostic location fallback.

Only a typed `missing-day` issue with `source-period` scope, a valid date,
and `relevant: true` increments `missing_day_entries`. False relevance and
unknown date/scope/relevance have separate counters. Counters count saved
entries, not distinct dates. Missing organization attribution is preserved.
An `invalid` or `skipped` issue with explicit `skipped: true` and report or legacy scope
increments `omitted_row_entries`; a generic warning or mismatch does not.
An omitted row with unknown organization remains unattributed.
Malformed classification fields do not erase independent valid facts, and
nullable legacy fields remain unknown. Shared-source counts can concern other
reports or sheets and must not be assigned to the selected matrix cell.
