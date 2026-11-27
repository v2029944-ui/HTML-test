# Проверка Tauri на Windows: GitHub-hosted и собственный runner

Проверки PR выполняются на изолированном GitHub-hosted Windows runner **без подписи, публикации релиза и доступа к пользовательской базе**.

Workflow [tauri-windows.yml](../.github/workflows/tauri-windows.yml) запускается для PR, меняющих `tauri/**`, `index.html`, `tests/**` или сам workflow, а также для `main` и ручного запуска. Он проверяет Node/frontend и три Rust crate: `tauri/src-tauri`, `tauri/import-core`, `tauri/storage-core`. Дополнительно проверяются SQLite и независимые клиенты через временную Windows SMB-папку.

После сборки обычного NSIS-установщика workflow проверяет значки при чистой установке и обновлении, восстановление SQLite на копиях, вспомогательную GUI-логику, два установленных клиента через SMB и приложение с настоящим WebView2. Для GUI отдельно собирается диагностическая версия. В артефакт `Tauri-Windows-experimental` попадает обычный установщик, сохранённый **до** этой сборки; диагностический EXE с удалённой отладкой не публикуется. Подписанные обновления проверяются отдельно.

Установленные GUI/SMB-проверки включаются для событий PR, ветки `main` и выделенной preview-ветки, указанной в workflow. Ручной запуск на другой ветке может их пропустить: перед приёмкой проверяйте результаты конкретных этапов, а не только общий статус запуска.

Следующая инструкция описывает необязательную локальную альтернативу. Самостоятельный runner на персональном ПК **не рекомендуется для публичного репозитория**, особенно при проверках внешних pull request.

## 1. Обычная локальная проверка

Нужна Windows 10/11 x64 с Node.js **24**, Rust с Windows MSVC toolchain, Microsoft C++ Build Tools, Git и WebView2. Текущий Windows workflow фиксирует Node.js **24.22.3** и Rust **1.100.0**; для воспроизведения CI используйте эти версии и lock-файлы репозитория. Установка зависимостей требует доступа к интернету; запуск готовой программы — нет.

Откройте PowerShell в корне клонированного репозитория на нужной доверенной ветке. До проверки убедитесь, что выбран нужный коммит и нет неучтённых локальных изменений:

```powershell
git status --short
git rev-parse HEAD
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\verify-tauri-windows.ps1 -BuildInstaller
```

Скрипт устанавливает Tauri CLI через `npm ci --include=optional`, запускает Node-тесты из `tauri/tests/*.test.cjs`, `tests/test_*.cjs` и `.github/scripts/pr-safety.test.cjs`, подготавливает интерфейс и значки. Вспомогательные `native_*_golden.cjs`, ожидающие JSON из stdin, не включаются в `node --test`. Проверяются неизменность самостоятельного `index.html` и наличие активных `unified-app`, `reference-desktop`, bridge и updater в собранном HTML.

Затем выполняются три набора Rust-тестов (`tauri/src-tauri`, `tauri/import-core`, `tauri/storage-core`) с `--locked` и, при `-BuildInstaller`, обычная Windows-сборка NSIS с тем же флагом. Ошибка любого этапа завершает проверку неуспехом. Результирующий установщик останется **локально** в `tauri/src-tauri/target/release/bundle/nsis/`; скрипт не подписывает и не публикует его. Если нужен прогон без установщика, уберите `-BuildInstaller`.

**Границы проверки:** локальный скрипт не устанавливает и не запускает Tauri, не создаёт SMB-папку и не выполняет Windows GUI-проверки. Python-браузерный набор `python -m unittest discover -s tests -v` запускается отдельно с Playwright и Chromium; часть тестов ожидает Linux-путь `/usr/bin/chromium`.

Проверки восстановления и вспомогательной GUI-логики можно запустить отдельно:

```powershell
python -m unittest discover -s tests -p test_sqlite_restore_copy.py -v
python -m unittest discover -s tauri/tests -p 'test_reference_*.py' -v
```

Эти команды не заменяют запуск установленного приложения с WebView2 и SMB-проверки полного Windows workflow.

После успешной сборки проверьте приложение в отдельной виртуальной машине или в отдельной учётной записи Windows с пустым каталогом «Документы/Анализ работ». Не запускайте непроверенную сборку поверх рабочей базы: Tauri использует настоящий `reports.sqlite3` в системных «Документах». Для проверки достаточно синтетических Excel, сохранения и повторного открытия SQLite, проверки календаря и выбора папки.

## 2. Собственный GitHub runner

Безопаснее всего использовать **отдельную Windows VM**, а не основной рабочий компьютер. Не храните на runner рабочие Excel, закрытые ключи обновлений, личные данные или постоянные привилегированные токены. Установите на эту машину те же средства разработки и Git.

1. Используйте только доверенную изолированную VM без важных данных и секретов; в настройках репозитория откройте **Settings → Actions → Runners → New self-hosted runner** и выберите Windows x64. Не разрешайте запускать на ней недоверенный код из внешних PR.
2. Выполните **предоставленные самим GitHub** команды установки и регистрации runner на VM; при настройке добавьте пользовательскую метку **`analiz-tauri`**. Регистрационный токен не копируйте в переписку или репозиторий.
3. Убедитесь, что runner отображается как **Idle** и имеет метки `self-hosted`, `Windows`, `X64`, `analiz-tauri`.
4. В **Actions → Tauri Windows self-hosted (manual) → Run workflow** выберите нужную доверенную ветку, проверьте её SHA и запустите проверку. Параметр `build_installer` включает локальную сборку установщика.

Workflow [tauri-self-hosted.yml](../.github/workflows/tauri-self-hosted.yml) имеет **только `workflow_dispatch`**: он не запускается автоматически при новых PR, push или по расписанию. Он вызывает тот же `verify-tauri-windows.ps1` на выделенном runner, выдаёт токену `contents: read` и не публикует выпуск, не подписывает установщик, не загружает бинарники в GitHub Artifacts. Его успешное выполнение не означает прохождения установленных GUI- и SMB-проверок.

GitHub: [добавление runner](https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/add-runners), [защита self-hosted runner](https://docs.github.com/en/actions/reference/security/secure-use), [условия оплаты Actions](https://docs.github.com/en/billing/concepts/product-billing/github-actions).

## 3. Приёмка изменений

Проверки выбираются по риску согласно [CODEX_SAFETY_RULES.md](CODEX_SAFETY_RULES.md). Для Tauri-кода перед слиянием требуется полный Windows `verify-and-build` на последнем SHA PR: Node + Rust, сборка NSIS, установленные WebView2/SMB-проверки и проверки данных в изолированном окружении. Для изменений только документации достаточно проверки ссылок и содержания; Windows workflow может не запуститься из-за фильтра путей.

Фиксируйте точный SHA, что проверено и какие ограничения остались. Успех старого коммита, отдельного этапа или локального скрипта не подтверждает полный Windows-прогон. Решение о слиянии принимает владелец отдельно.

Не запускайте `tauri-signed-release.yml`: он содержит команды публикации релиза и не относится к этой процедуре.
