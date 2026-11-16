# Анализ работ — Tauri 2 + SQLite

## Установка

Основное Windows-приложение построено на Tauri 2 и системном WebView2. [Подписанный установщик Tauri 0.1.2](https://github.com/v2029944-ui/HTML-test/releases/download/tauri-v0.1.2/Analiz-rabot-Tauri-Windows-x64-setup.exe). Текущие выпуски — в [GitHub Releases](https://github.com/v2029944-ui/HTML-test/releases). Исходный код может содержать ещё не выпущенные функции.

Для работы готового EXE не требуются Rust, Node.js и Python. WebView2 Runtime может потребоваться установить отдельно.

## Архитектура

- `../index.html` — автономный браузерный интерфейс и источник интерфейса Tauri.
- `prepare.cjs` — копирование HTML и подключение Tauri API в `tauri/web/`, не меняя оригинал.
- `folder-importer.js` — правила и автоматический импорт Excel из выбранной папки.
- `bridge.js` — вызовы Rust-команд из интерфейса. Внутреннее имя `window.desktopAPI` сохранено для совместимости HTML.
- `src-tauri/src/storage.rs` — локальная SQLite, валидация, миграция и безопасная работа с файлами.
- `updates.js` — подписанные обновления с подтверждением.

База размещается в системной папке **«Документы / Анализ работ / reports.sqlite3»**, вне каталога установленной программы. Слежение за выбранной папкой XLSX выполняется примерно раз в три секунды во время работы приложения.

Поддерживаются дни, недели, месяцы и импорт квартальных/годовых Excel с ежедневными данными. Описание формата: [DAILY_WIDE_REPORTS.md](../docs/DAILY_WIDE_REPORTS.md).

## Миграция пользовательских данных

При первом открытии SQLite может перенести находящиеся в старом каталоге `reports.json` и `desktop-config.json`. Это **не исходники старого приложения**, а совместимость с существующими пользовательскими данными: файлы не удаляются и не изменяются. Ошибка миграции блокирует запись, чтобы предотвратить потерю данных.

Исходный `index.html` хранит данные в IndexedDB, перенос браузерных данных выполняется через резервную копию вручную.

## Сборка и тесты

Для разработки на Windows требуются Node.js 24, Rust, C++ Build Tools и WebView2:

```powershell
npm install --prefix tauri
node --test tests/*.cjs tauri/tests/*.test.cjs
npm --prefix tauri run prepare:frontend
npm --prefix tauri run icons
cargo test --manifest-path tauri/src-tauri/Cargo.toml
npm --prefix tauri run build:windows
```

Обычный Windows-установщик создаётся в `tauri/src-tauri/target/release/bundle/nsis/`. CI — `.github/workflows/tauri-windows.yml`. Браузерные тесты при установленном Playwright/Chromium:

```bash
PYTHONDONTWRITEBYTECODE=1 python -m unittest discover -s tests -v
```

## Подписанные онлайн-обновления

Постоянный Tauri-канал: [tauri-latest.json](https://github.com/v2029944-ui/HTML-test/releases/download/tauri-updates/tauri-latest.json). Подписанная сборка проверяет доступность новой версии, а установка запускается только после подтверждения; во время импорта Excel она блокируется. Не меняйте открытый ключ подписанных обновлений без плана миграции.

Для подписанной сборки используются `ANALIZ_UPDATER_PUBLIC_KEY`, `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Закрытый ключ и пароль должны храниться вне репозитория, не попадать в PR и артефакты сборки.

Перед выпуском синхронно увеличьте версию в `package.json` (внутри `tauri/`), `src-tauri/Cargo.toml` и `src-tauri/tauri.conf.json`. Соберите `npm --prefix tauri run build:signed:windows`. Сначала публикуйте установщик и `.sig` в версионном выпуске `tauri-v<версия>`, затем обновляйте `tauri-updates/tauri-latest.json`; не заменяйте подписанные установщики уже выпущенной версии.

Подписанный выпуск автоматизирован в `.github/workflows/tauri-signed-release.yml`. Эта очистка исходников не должна запускать публикацию или менять канал обновлений.

## Обязательные проверки

- Реальный квартальный и годовой Excel, сверка сумм, дат и подразделений.
- Работа Windows/WebView2: выбор папки, обновление файла, сохранение и резервная копия.
- Переход между двумя подписанными версиями с сохранением SQLite, отклонение обновления и работа без сети.
- Нагрузочная проверка больших отчётов: пока фронтенд получает всю модель аналитики, выборочные SQL-запросы — отдельная задача.

Успешная CI-сборка не эквивалентна ручному тестированию установленного EXE.
