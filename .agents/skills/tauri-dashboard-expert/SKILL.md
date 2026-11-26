---
name: tauri-dashboard-expert
description: Implement or review HTML-test analytical tables, charts, filters and desktop interaction in Tauri/WebView2 while preserving SQL/IPC contracts and data semantics.
---

# Tauri analytical dashboard

- Read AGENTS.md and docs/CODEX_SAFETY_RULES.md. Inspect prepare.cjs, unified-build.cjs, actual frontend modules and bridge contracts; preserve root index.html and avoid editing generated output as source.
- Use existing bounded SQL pagination and stable ordering; follow current #79 requirements (50 data rows per page where applicable), sticky headers and first-column behavior only where requested. Never fetch the full database into JS.
- Preserve scroll, focus, selected filters and page context across tab changes; cancel or ignore stale responses. Keep filter placement usable at narrow widths and all supported scales.
- Charts must show units, legend, period and comparable scales. Distinguish missing data from zero, handle negatives and partial coverage; avoid misleading axes, excessive chart height and unsupported averages.
- Preserve SQL/IPC/CSV/print consistency, group membership and source granularity. Do not change calculations as a cosmetic fix; use sqlite-tauri-expert for storage/analytics changes.
- Reuse existing chart/table components and CSS tokens. Avoid heavy animations, extra libraries and duplicated rendering pipelines.
- Test synthetic long names, large selections, empty/partial data, rapid filter changes, pagination and export. Use existing Node and Windows/WebView2 checks as required; report exact SHA and untested paths.
