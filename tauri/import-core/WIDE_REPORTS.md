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
cargo build --release --manifest-path tauri/import-core/Cargo.toml --example cancel_wide
python tools/profile_native_wide_cancel.py --binary tauri/import-core/target/release/examples/cancel_wide --fixtures /tmp/wide-profile --output /tmp/wide-cancel.json
python tools/profile_native_wide.py --binary tauri/import-core/target/release/stage-xlsx --output /tmp/wide-stress --stress-only
```

The parity tools use actual legacy JavaScript on small real XLSX files; Chromium
is required for wide fixtures. Profiling creates exact 500k/1.5m/3m XML-cell books,
numeric/text and mostly empty-formatted variants, then repeats each Rust child.
Quarter sheets isolate cell-count scaling from component-size limits. Generation
and fixture statistics are outside the measured child. First invocation does not
mean a cold OS cache. In-child Linux `/proc/self/status` VmHWM measures the Rust
process, not WebView2. Raw reading and business conversion timings include their
respective private SQLite work; no separate final live-apply timing is claimed.
Archived repeated results, stress/cancellation probes and comparison limits are
in [the progress report](../../docs/NATIVE_IMPORT_PROGRESS.md#measured-wide-core-code-1af6599).

## Integration still required

Prepared documents/quality preview and atomic live apply must preserve wide
provenance, source issue scope/date and explicit-zero validity before activation.
The C foundation predates these APIs. Native UI/IPC, full bounded application
reads/analytics/export, final apply/load/restart measurements, real cold launches,
Windows/WebView2 reaction/memory tests and exact warning presentation remain
separate work. No production compatibility or three-million-cell end-to-end
safety claim follows from this core path alone.
