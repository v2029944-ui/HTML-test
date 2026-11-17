# Native daily-wide conversion (experimental core)

This additive core path converts annual/quarterly work-by-day sheets. The
production Tauri and standalone browser UI still use the existing parser. This
does not complete the native import project or remove browser limits.

## Storage and bounds

Raw ZIP/XML ingestion writes cells, shared strings, structured workbook/style
attributes and merges to private SQLite. XML namespace resolution accepts normal
relationship-prefix aliases and rejects undefined prefixes. Worksheet roots are
recognized even when their relationship target is outside `xl/worksheets/`.
No ZIP member is extracted to a filesystem path and no relationship is fetched.

Style formats, style indices, merge ranges, converted rows, issues, source checks
and report provenance remain indexed on disk. One sheet's at-most-1,024 column
pairs/day descriptors and at-most-2,048 column flags are resident. Workbook sheet
metadata is limited to 1,024 entries with bounded names; at most 12 sheets can be
selected. A `(sheet,column,row)` index avoids a full-sheet scan for every day.
One detail row and four organization accumulators are retained during financial
conversion. The existing archive, XML, token, coordinate, three-million-cell and
two-GiB staging limits are unchanged. SQLite cache is bounded and temporary work
uses files. SQL progress hooks own an atomic cancellation flag.

Conversion publishes its complete result in a private transaction. Cancellation,
invalid dates, duplicate days or source errors roll back to the previous complete
preview; they do not write the live database. Index/style setup may be retained
after cancellation. Stage cleanup/restart behavior is inherited from raw staging.

## Semantics and APIs

`detect_wide` returns bounded candidate metadata. `convert_wide` uses local today,
matching the existing browser's missing-historical-day rule;
`convert_wide_as_of` provides deterministic tests. `WideOptions` accepts a confirmed
year, ordered sheet subset and optional per-sheet mappings. Native rows/columns
are **one-based**; browser manual mappings are zero-based. The options must be
translated at the future UI boundary.

Merged headers, short/Russian/ISO dates, cached formula results, shared strings,
Excel serials including the 1904 system and manual columns are supported.
Uncached formulas are not evaluated. Invalid/fake leap dates, conflicting pair
headers, ambiguous years and duplicate days reject the conversion. Filename dates
are never inferred. Numeric zeros are values. Entirely blank day pairs produce no
report. A proven zero day with no details has explicit-zero provenance, rather
than fabricated rows. Source-period controls and missing-day diagnostics retain
addresses, expected/actual values and scope/date.

Existing `report_page`, `report_rows`, `report_issues`, `report_totals` (maximum
100 items) and bounded `report_organizations` remain available.
`report_provenance` adds sheet, source format/block, coverage, explicit zero and
source control date. `source_issues(sheet,after,limit)` pages complete source-period
diagnostics, including their historical relevance. Relevant issues attach to the
first imported day per sheet, as in the old parser. A historical selected sheet
with relevant diagnostics but no usable reports fails the whole conversion.

`regex` provides bounded, non-backtracking date/header matching; expressions use
JavaScript whitespace and ASCII digit semantics. `chrono` provides checked
calendar operations and local-date behavior; it does not read the workbook or
materialize sheets. `rusqlite/hooks` adds responsive cancellation during long SQL
index builds/scans. Existing ZIP/XML dependencies and safety limits remain.

## Local verification

```sh
cargo test --manifest-path tauri/import-core/Cargo.toml
cargo build --release --manifest-path tauri/import-core/Cargo.toml
python tools/compare_native_legacy.py --binary tauri/import-core/target/release/stage-xlsx
python tools/compare_native_wide.py --binary tauri/import-core/target/release/stage-xlsx
python tools/profile_native_wide.py --binary tauri/import-core/target/release/stage-xlsx --output /tmp/wide-profile
```

The parity tools use actual legacy JavaScript on small real XLSX files; Chromium
is required for wide fixtures. Profiling creates exact 500k/1.5m/3m XML-cell books,
numeric/text and mostly empty-formatted variants, then repeats each Rust child.
Quarter sheets isolate cell-count scaling from component-size limits. Generation
and fixture statistics are outside the measured child. First invocation does not
mean a cold OS cache. In-child Linux `/proc/self/status` VmHWM measures the Rust
process, not WebView2. Raw reading and business conversion timings include their
respective private SQLite work; no separate final live-apply timing is claimed.

## Prepared artifacts and atomic application (C3)

`Stage::prepare` and `prepare_with_mode` now accept wide conversion results. Each
prepared report exposes frozen `provenance` and retains the ordinary `info`, rows,
controls and issue APIs. Wide documents retain `sheetName`, `sourceFormat`,
`sourceBlock`, `sourceCoverage`, `sourceControlDate`, `explicitZero`, zero-based
legacy source-column aliases, and the existing native sheet/mapping fields.
Declared `sourceCoverage` comes from the source title/header and has valid ordered
bounds; actual explicit daily dates may lie outside it. Those days are retained
alongside missing-day diagnostics for the declared period, matching the old parser.
Coverage is not an extra enclosing-date constraint or a substitute for zero proof.
`sourceReportId` retains the local conversion ID, linking `SourceIssue.report_id`
to live documents without guessing issue identity from kind/row.

The common converted-issue schema/API adds optional `scope`, `date` and
`source_issue_sequence`. Relevant source issues copy those values directly into
the first imported day per sheet. Preparation verifies unique complete source
links and identical cells/numeric metadata. `Stage::source_issue_page` and
`PreparedImport::source_issues` page all sheets (1..100), including future-only
sheets without a report. `PreparedImport::source_diagnostics()` freezes total,
relevant and deferred counts plus SHA256 of canonical serialized records followed
by newline. The complete stage hash binds this manifest before apply.

Complete source diagnostics, including relevance, serialize once in the first
artifact report's `sourceIssues` and `sourceDiagnostics`; every wide report has a
`sourceDiagnosticsReport` live-ID anchor. This avoids losing future diagnostics
when their sheet has no report. Relevant issues remain attached only to the first
day of their own sheet, and quality `issues` counts them once. The additional
`source_issues` / `relevant_source_issues` / `deferred_source_issues` quality counts
are source subsets/metadata, not additional notices to add to `issues`.

A zero-row report is accepted only for a complete native wide conversion with
explicit-zero provenance, zero report totals, known zero controls and matching
known organization presence. Incomplete provenance, missing/nullable/nonzero
controls and ordinary empty reports reject. Apply uses the frozen validation
result; there is no caller-provided allow-empty flag. The live normalized index
records a legitimate row_count=0, while JSON and prepared organization summaries
retain known zero-control organizations without fabricated financial rows.

Comparison aliases retain nullable expected components for source control
comparisons with known numeric actuals. Ordinary invalid rows without a comparison
still emit null expected/actual aliases. Controls add legacy type/row/name aliases;
native fields are retained. Exact warning prose remains separate.

C2 source/hash/revision/generation/conflict/whole-source replacement, explicit
acknowledgment, rollback and cancellation/commit boundaries remain in place. No
full document/history Value is introduced into preparation/application, and no
post-apply full load occurs.

## Integration still required

Native UI/IPC, bounded live quality/source-diagnostic queries, full bounded
analytics/export and zero-organization coverage indexing, production restore
coordination, real cold launches, Windows/WebView2 reaction/memory tests and exact
warning presentation remain separate work. Relevance is a frozen conversion-time
snapshot, not recomputed automatically for a long-lived artifact. No production
compatibility or three-million-cell end-to-end safety claim follows from this
backend slice. The separately developed analytics branch requires a reviewed
schema/index integration; this work does not depend on its edits.

Production-blocking lifecycle risk: manual `replace_exact` can remove the first
artifact report while other old days still refer to its `sourceDiagnosticsReport`.
That can orphan complete source diagnostics. No preservation/reference check or
partial-anchor replacement block is implemented here. Keep production import
activation disabled until resolved. Whole-source/full-repeat replacement is safe
for this representation when all old anchor references are removed atomically.
