# Release notes per application version

**Current stable signed Windows release:** [Tauri 1.5.1](1.5.1.md), [GitHub Release](https://github.com/v2029944-ui/HTML-test/releases/tag/tauri-v1.5.1). Subsequent features merged into `main` require a **new version greater than 1.6.1**; never rewrite the released installer or its historical notes.

**Published prerelease (manual installation and signed online updates):** [Tauri 1.6.1](1.6.1.md), [GitHub Release](https://github.com/v2029944-ui/HTML-test/releases/tag/tauri-v1.6.1). Installer source: `27fb93e4ccf897dd726e62d1a55d3a68471c0018`. SQLite Analytics is the default startup screen; the classic screen remains an explicit choice. The online channel serves 1.6.1. Existing 1.6.0 assets remain immutable.

For **every new signed Tauri version**, create `tauri/release-notes/<version>.md` before running the signed release workflow. The publisher rejects missing notes and placeholders; it supplies the file directly to GitHub Releases via `gh release create --notes-file`.

A release note must have a heading and concrete list of changes (new features, fixes, storage/migration/backup changes and known limitations as applicable). Describe what actually shipped, not everything merged or planned. Confirm the exact source commit and Windows/WebView2 tests. The published version must include the installer and signature. Keep existing version assets immutable.

For a **manual unsigned prerelease**, the same rule applies: publish the installer as an asset of the separate versioned GitHub Release with release notes, but **do not** advance the signed `tauri-updates` channel.

Example for a future version (do not copy generic placeholders into a release):

```markdown
# Анализ работ 0.1.X

## Что изменилось
- Исправлена конкретная ошибка ...
- Добавлена проверенная возможность ...

## Обновление и сохранность данных
- Совместимость, резервная копия и известные ограничения ...
```

See [release policy](../../docs/RELEASE_POLICY.md) and root [AGENTS.md](../../AGENTS.md). Publishing requires the owner's explicit approval.
