---
name: sql-code-review
description: Review SQLite and rusqlite SQL correctness, injection risks, schema dependencies and regression impact in HTML-test. Use for SQL review or before changing tables, columns, migrations or analytics formulas.
---

# SQL review and dependency trace
Inspired by [SQL code review](https://github.com/github/awesome-copilot/tree/main/skills/sql-code-review) and [SQL object impact analysis](https://github.com/github/awesome-copilot/tree/main/skills/sql-object-impact-analysis); rewritten for SQLite/Rust.

- Trace affected tables/columns/indexes/views/triggers through Rust SQL strings, dynamically assembled queries, migrations, tests, IPC responses, JS callers, CSV and print. Confirm references and distinguish live code from retired code.
- Use rusqlite bound parameters for values. Dynamic identifiers, sort keys and SQL fragments require explicit allowlists; review untrusted XLSX/JSON inputs and limits.
- Verify JOIN cardinality, grouping keys, NULL vs zero, empty sets, duplicate matches, numeric types/overflow, stable ordering, foreign key enforcement per connection and transaction failure behavior. Do not propose SQL Server roles or PostgreSQL-specific syntax as SQLite features.
- For changed calculations test independent source-row expectations, missing/partial reports and incompatible units. Do not silently coerce NULL to zero or average averages.
- For schema changes map readers, writers, backup/restore and shared snapshots; identify incompatibility and test on isolated copies. Follow repository approval and separate-PR rules.
- Report confirmed defects first, severity, file/line, reproduction, expected/actual behavior and minimal fix; label hypotheses and untested paths. Do not change code during review unless requested.
