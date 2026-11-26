---
name: sql-optimization
description: Measure and tune SQLite queries, indexes, pagination and analytics latency in HTML-test. Use for slow SQL, query plans, benchmarking or database performance; not ordinary UI changes.
---

# SQLite performance
Inspired by [GitHub awesome-copilot SQL optimization](https://github.com/github/awesome-copilot/tree/main/skills/sql-optimization); rewritten for SQLite.

1. Read the actual query, schema, indexes and bundled SQLite version; reproduce on an isolated database. Follow AGENTS.md and docs/CODEX_SAFETY_RULES.md.
2. Measure cold/warm timings, median/p95, row counts, memory and write/import costs as relevant. For T-19 onward see issue #185; do not require the full benchmark matrix for a small unrelated fix.
3. Use `EXPLAIN QUERY PLAN` with representative bound values. Investigate selective full scans, temporary B-trees, correlated queries, repeated IPC/SQL, deep OFFSET and unnecessary columns. A scan is not inherently bad. Check existing indexes before adding one.
4. Test equality/range/order index layouts, expression or partial indexes only where predicates match, and stable keyset pagination with a unique tie-breaker if compatible with UI behavior. Avoid blanket JOIN/UNION rewrites, especially when they change row multiplicity or NULL behavior.
5. Compare identical inputs and outputs before/after, including duplicate rows, NULL, zero, mixed coverage and ordering. Report plan changes, timings and trade-offs. Do not claim unmeasured improvement.

References on demand: [planner](https://sqlite.org/queryplanner.html), [optimizer](https://sqlite.org/optoverview.html), [EQP](https://sqlite.org/eqp.html).
