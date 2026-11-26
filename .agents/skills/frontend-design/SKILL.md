---
name: frontend-design
description: Design or refine compact, consistent analytical UI in HTML-test; use for layout, typography, themes, controls and responsive desktop design.
---

# Analytical frontend design
Inspired by [Anthropic frontend-design](https://github.com/anthropics/skills/tree/main/skills/frontend-design); original project-specific guidance.

- Inspect current screens, theme tokens, shared components and actual build entry points before editing. Preserve the established visual identity unless redesign is requested.
- Prioritize information density, legibility and clear hierarchy over decorative cards, gradients, oversized headings or motion. Use consistent spacing, typography, control heights and alignment.
- Reuse existing CSS variables and components; verify light/dark themes and all supported palettes, including hover, selected, disabled and focus states.
- Keep filters, navigation and primary actions predictable. Use available width efficiently; long Russian labels must wrap or truncate with accessible full text without hiding essential information.
- Respect existing scale options and narrow windows; avoid fixed dimensions that clip controls. Do not introduce frameworks, fonts, CDN or dependencies without approval.
- For Figma handoff use the supplied design only when requested; preserve functional behavior. Validate actual screenshots via ui-ux-review when available.
