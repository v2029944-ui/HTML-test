# Проверка Tauri на Windows без расхода минут GitHub-hosted Actions

Проверка PR #13 и будущих изменений **без публикации релиза**, подписи обновлений и изменения пользовательской базы.

## 1. Обычная локальная проверка

Нужна Windows 10/11 x64 с Node.js **24**, актуальным Rust (не ниже **1.82** для `tauri/import-core`), Microsoft C++ Build Tools, Git и WebView2. Установка зависимостей требует доступа к интернету; запуск готовой программы — нет.

Откройте PowerShell в корне клонированного репозитория:

```powershell
git fetch origin
git switch cleanup/tauri-only-remove-electron-20261117
git pull --ff-only
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\verify-tauri-windows.ps1 -BuildInstaller
```

Скрипт выполнит все Node-тесты, подготовку интерфейса, оба набора Rust-тестов (`tauri/src-tauri` и `tauri/import-core`), проверку неизменности самостоятельного HTML и обычную Windows-сборку NSIS. Ошибка любого этапа завершает проверку неуспехом. Результирующий установщик останется **локально** в `tauri/src-tauri/target/release/bundle/nsis/`; он не подписывается и не публикуется. Если нужен только быстрый прогон без установщика, уберите `-BuildInstaller`.

**Ограничение:** Python-браузерные тесты в `tests/` в текущем состоянии жёстко используют `/usr/bin/chromium`, поэтому не входят в этот Windows-сценарий. Они проверяются отдельно в Linux с Playwright и Chromium. Сам скрипт также **не устанавливает и не запускает** Tauri.

После успешной сборки проверьте приложение в отдельной виртуальной машине или в отдельной учётной записи Windows с пустым каталогом «Документы/Анализ работ». Не запускайте непроверенную сборку поверх рабочей базы: Tauri использует настоящий `reports.sqlite3` в системных «Документах». Для проверки достаточно синтетических Excel, сохранения и повторного открытия SQLite, проверки календаря и выбора папки.

## 2. Собственный GitHub runner

Безопаснее всего использовать **отдельную Windows VM**, а не основной рабочий компьютер. Не храните на runner рабочие Excel, закрытые ключи обновлений, личные данные или постоянные привилегированные токены. Установите на эту машину те же средства разработки и Git.

1. В приватном репозитории откройте **Settings → Actions → Runners → New self-hosted runner** и выберите Windows x64.
2. Выполните **предоставленные самим GitHub** команды установки и регистрации runner на VM; при настройке добавьте пользовательскую метку **`analiz-tauri`**. Регистрационный токен не копируйте в переписку или репозиторий.
3. Убедитесь, что runner отображается как **Idle** и имеет метки `self-hosted`, `Windows`, `X64`, `analiz-tauri`.
4. В **Actions → Tauri Windows self-hosted (manual) → Run workflow** выберите нужную доверенную ветку (для проверки PR #13 — `cleanup/tauri-only-remove-electron-20261117`) и запустите проверку.

Workflow `.github/workflows/tauri-self-hosted.yml` добавлен в `main`, чтобы работал ручной запуск, но имеет **только `workflow_dispatch`**: он не запускается автоматически при новых PR, push или по расписанию. Он использует только выделенный runner, выдаёт токену `contents: read` и не публикует выпуск, не подписывает установщик, не загружает бинарники в GitHub Artifacts.

GitHub: [добавление runner](https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/add-runners), [защита self-hosted runner](https://docs.github.com/en/actions/reference/security/secure-use). Согласно текущей документации [GitHub Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions), self-hosted runner не расходует оплачиваемое вычислительное время GitHub, однако VM и её обслуживание оплачивает владелец.

## 3. Условие слияния PR #13

Сначала должен пройти **полный Node + Rust прогон и сборка NSIS** из проверяемого коммита. Для окончательного подтверждения функциональности Tauri дополнительно требуется запуск GUI в отдельной Windows-среде и проверка SQLite/импорта без доступа к реальной пользовательской базе.

Не запускайте `tauri-signed-release.yml`: он содержит команды публикации релиза и не относится к этой процедуре.
