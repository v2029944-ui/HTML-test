---
name: sqlite-tauri-expert
description: Apply HTML-test database invariants to Rust/rusqlite storage, Excel import, SQLite schema, aggregation, caching, backup and SMB synchronization. Use for changes to persistent data or shared database behavior.
---

# HTML-test SQLite/Tauri
Read AGENTS.md and docs/CODEX_SAFETY_RULES.md. This skill owns project-specific storage invariants; invoke the performance or review skill only when that workflow is needed.

- Locate actual code under tauri/src-tauri/src (storage.rs, database, analytics, shared, commands); inspect current schema, bundled SQLite version, tests and open PRs. Do not assume old file names or releases are current.
- Preserve raw report rows and source granularity: never distribute weekly/monthly totals into days or double count overlapping sources. Unknown data is not zero. Group/classification membership must not multiply source rows.
- For amount/count averages use only paired known values with compatible units; nonpositive denominator or no paired data yields unavailable, with partial coverage shown. Follow current #79 acceptance criteria across SQL, IPC, UI, CSV and print.
- Use bounded queries and revision/workspace-aware caching; invalidate on mutations, group changes, database switch or shared generation changes. No full history in JS.
- Keep local WAL and FULL durability unless separately approved; do not open a WAL database directly on SMB. Preserve verified snapshot publication, locks, generation/digest checks and recovery. Tune cache/temp storage only with memory and performance evidence.
- For writes verify transactional rollback, cancellation, stale revisions, duplicate imports, disk/network failures and restart recovery. Use existing SQLite Online Backup and verify copies; never copy live database files alone as a backup. No experiments on real data.
- Schema or aggregate changes are separate PRs with independent totals, data preservation and failure tests. Follow project compatibility policy; explain incompatibility, never silently discard user data.
- Run existing relevant Rust/Node tests and Windows/WebView2/SMB checks as required by risk; report exact SHA, results and blockers. No release or merge without owner approval.

On demand: [WAL](https://sqlite.org/wal.html), [backup](https://sqlite.org/backup.html), [PRAGMA](https://sqlite.org/pragma.html), [corruption prevention](https://sqlite.org/howtocorrupt.html).
