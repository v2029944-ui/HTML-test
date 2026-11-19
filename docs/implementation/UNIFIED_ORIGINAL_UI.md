# Полный исходный интерфейс — единая SQLite-версия

Запрос: полностью вернуть интерфейс исходного index.html, убрать отдельные версии/режимы, сохранить порционное чтение SQLite и исправления issues64/67/PR65. Номер установленной версии и реальная анимация загрузки остаются. Импорт XLSX остаётся источником данных.

Первый этап выполнялся локально. После отдельного поручения пользователя «Релизни» разрешены commit/push/PR и выпуск 1.8.0 при успешных обязательных проверках. Никаких load_data/save_data, полной истории JS или старых обработчиков частичной state.reports. Браузерный исходник остаётся эталоном разметки/стилей, в приложении используется один SQL-controller.

## Контракт frontend

`window.unifiedUI` создаёт unified-base.js после определения исходных чистых render helpers (старые main/wire НЕ выполняются):
- `api`: native desktopAPI БЕЗ load/save; `state`: исходное UI-state, reports всегда[].
- `revision`: текущая safeinteger/null; `snapshot`: nativeSettingsBootstrap DTO (revision,reports,indexed_reports,groups,catalog,org_names).
- `$`, `el(tag,text,className)`, `notice(text,error)`, `fmt(value,metric)`, `table(root,metric,caption,headers,rows,options)` uses original makeTable. For native CSV options.csvContext={export:async()=>...,text:string}; when no export explicit page-only CSV label.
- `drawChart`, `drawDepartments`, `setPeriodInput(id,kind,date?)`, `range(kind,start,end,source)` use nativePeriodSelection.
- `onSnapshot(callback)` runs on each fresh bootstrap; `onView(view,callback)` lazy load callback; `activate('dashboard'|'changes'|'upload'|'groups'|'storage')` original view markup/menu.
- `refresh()` safely obtains nativeSettingsBootstrap, invalidates current queries via generation and notifies subscribers; `onScope(callback)` and `publishScope(scope)` bridge dashboard controls to extended views; `scope` is latest analysis selection.
- scope={revision,source:'daily'|'imported',displayType:'day'|'week'|'month'|'year',type:'day'|...,from:ISO,to:ISO,organizations:[names],selectedNames:[exact names],selectedKeys:[work:/group:keys],nameRows:[{key,name,kind}]}.
- `request(scope,queryOverrides={})` -> AdvancedRequest {revision,source,displayType,organizations,selectedNames,query:{method:'previous'|'range',from,to,percent:30,min_count:5,min_amount:1000}}.
- `isCurrent(revision,ticket?)` modules own tickets; root generation invalidates pages on snapshot. Mutations must use expected revision and call await UI.refresh() after success. Errors clear stale rows/plots; no catch success.

Modules define `window.unifiedAnalytics`, `window.unifiedExtended`, `window.unifiedReports`, `window.unifiedManagement`; each `.init()` binds existing canonical IDs once; root invokes init then UI.refresh(). Each adds snapshot/view hooks. No main/wire/renderAll/oldsaveSettings callbacks.

## Ownership
- Root: canonical build/prepare, startup single entry, original helper bridge/base, dashboard charts/KPI and commonfilters, bridge/IPCregistration, Cargo features, browser/tests/integration.
- Analytics backend: unified_analytics.rs, analytics.rs, advanced_analysis.rs; SQL search/filter/sort/stats/CSV, period/name paging.
- Management backend: unified_management.rs, management.rs, unified_settings_io.rs, catalog conversion; group exclusivity, pages, import/export.
- Data admin: unified_data_admin.rs child management; org rename/clear with backup + source lifecycle.
- Reports fullstack: new report query APIs+native UI reports/import/calendar/quality (see assigned task); coordinate only own modules.
- Extended frontend: unified-extended.js for all classic comparison/contributions/structure/usual/regularity/changes controls.
- Management frontend: unified-management.js for groups/catalog/orgsettings/backup.

## Проверки
Targeted regression RED/GREEN; full Node/Rust when coherent; actual browser exact canonical DOM and all controls. Windows build/CI cannot be claimed unless run, which requires separately authorized push.

## Локальная проверка 19.11.2026

- Сборка frontend: один index.html, исходные стили/разметка и SQL-контроллеры. Отдельные startup/native-workspace маршруты и старые UI-контроллеры не поставляются. XLSX остаётся источником импорта.
- Node: штатный `npm run test:frontend --prefix tauri`, 46 файлов тестов успешно. Новые dashboard/extended тесты включены в штатный набор tauri/tests.
- Storage-core: 189 успешно, 1 прежний тест игнорируется; полный последовательный прогон. Включены атомарный отчёт+справочник, отмена при занятом writer, повторная индексация после смены папки, сохранность настроек и исходных файлов.
- Import-core: полный прогон успешно (39 тестов).
- Chromium с подменой IPC: запуск, исходное меню справа, графики/таблица, отчёты, 29 дней февраля, закрытие месяца, оформление, загрузка; без ошибок JS и переполнения страницы при ширине 1366/390/320. Полная история не загружается (`load_data`/`save_data` не вызываются).
- Windows/WebView2 и компиляция Tauri-обработчиков не проверены. Offline cargo check остановился до компиляции: пакет tauri отсутствует в локальном кэше; GTK/WebKit в Linux также отсутствуют. Браузерная проверка с IPC-подменой этого не заменяет.
- Известные ограничения: максимум 50 явно выбранных работ; «Выбрать все» охватывает всю SQL-выборку с пагинацией. Для legacy Excel с несколькими листами без сохранённого имени лист выбирается вручную.
- До отдельного запроса релиза commit/push/PR/release не выполнялись; канал обновлений сохранялся на 1.7.0. Кандидат 1.8.0 проходит отдельные проверки перед публикацией.

## Кандидат выпуска 1.8.0

Поручение пользователя «Релизни» разрешает подготовку и публикацию после обязательных ворот. Предрелизная проверка дополнительно исправила сохранность посторонних пустых групп и расширений JSON, начальную настройку подразделений без kv.settings, исторические ключи вне выбранного диапазона, границы детализации, ограничение счётчиков качества и фильтры обеих сторон конфликтов. Восстановленные нетипичные строки не блокируют запуск всех разделов; неполный raw-индекс не выдаётся за полный.

Локальный полный прогон кандидата: 196 SQLite-тестов успешно, 1 прежний игнорируется; 51 файл Node-тестов успешно; 7 проверок восстановления успешно. Публикация требует отдельного успешного Windows/NSIS/WebView2 CI на точном коммите. Старые утверждения выше описывают локальный этап до запроса релиза.

WebView2-прогон кандидата дополнительно выявил порядок удаления общих диагностических якорей при полной ручной замене дневного источника. Исправление сохраняет защитный триггер и использует дисковый план точных замен внутри той же транзакции. После исправления полный локальный SQLite-прогон: 198 успешно, 1 игнорируется. Проверены частичная замена с откатом, отмена после копирования, ошибка INSERT, сохранность постороннего отчёта и повторное чтение квитанции. Повторный Windows-прогон остаётся обязательным.

Прогон Windows после исправления замены прошёл все операции первой сессии, но выявил незакрытые системные подтверждения. Скриншот и исходник tauri-plugin-dialog подтвердили, что window.confirm возвращает Promise. Все активные подтверждения ожидаются до записи; добавлены тесты отложенного ответа, отказа, ошибки, повторного нажатия и устаревания. Windows smoke отвечает реальным системным кнопкам и проверяет ревизию SQLite до ответа и после Cancel; нормальное закрытие и восстановление остаются обязательными. Полный Node-набор после исправления: 51 файл успешно.
