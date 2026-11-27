// User-facing errors for the Tauri SQL workspace and native import flows.
// Do not display opaque Rust/SQLite internals or untrusted error strings in the UI.
(function(root){
'use strict';
const rules=[
 [/^Pattern history exceeds 3660 calendar buckets\b/i,'Слишком длинный исторический диапазон. Сократите анализируемый диапазон или число предыдущих окон.'],
 [/^Management session disk quota exceeded\b/i,'Превышен допустимый объём временных данных. Освободите место на диске или уменьшите объём операции.'],
 [/^Session size overflow\b/i,'Объём временных данных слишком велик. Разделите операцию на меньшие части.'],
 [/^Management string\/raw token exceeds64KiB\b/i,'Название или значение слишком длинное. Сократите его и повторите действие.'],
 [/^Management batch\/page must be1\.\.=500\b/i,'Выбрано слишком много элементов для одного действия. Уменьшите выборку.'],
 [/^Management search exceeds512UTF8bytes\b/i,'Поисковый запрос слишком длинный. Сократите текст поиска.'],
 [/^At most4live management sessions per storage\b/i,'Одновременно выполняется слишком много операций. Завершите текущие действия и повторите попытку.'],
 [/^No active organization\b/i,'Не выбрано подразделение. Выберите подразделение и повторите действие.'],
 [/^XLSX header mismatch\b/i,'Заголовки Excel не соответствуют выбранному шаблону. Проверьте лист, названия и расположение столбцов.'],
 [/^Metric aggregation overflow\b/i,'При расчёте показателей получено слишком большое значение. Проверьте исходные числа и единицы измерения.'],
 [/^Management numeric overflow\b/i,'Получено слишком большое числовое значение. Проверьте исходные данные.'],
 [/^Source changed\b/i,'Файл изменился после проверки. Выберите его заново и повторите проверку перед сохранением.'],
 [/^A verified SQLite backup destination is required\b/i,'Перед заменой отчётов необходимо выбрать место для проверенной резервной копии базы.'],
 [/^Invalid number\b/i,'В Excel обнаружено некорректное число. Проверьте указанную ячейку и формат данных.'],
 [/^Source control mismatch\b/i,'Контрольные итоги исходного файла не совпадают с рассчитанными. Проверьте значения в Excel.'],
 [/^mismatch$/i,'Данные не совпадают с ожидаемыми. Проверьте выбранный файл и настройки импорта.'],
 [/^(?:database is locked|database table is locked)\b/i,'База данных занята другой операцией. Закройте другие экземпляры программы и повторите попытку.'],
 [/^(?:database or disk is full|disk full|no space left on device)\b/i,'На диске недостаточно места для выполнения операции. Освободите место и повторите попытку.'],
 [/^(?:permission denied|access is denied)\b/i,'Нет доступа к файлу или папке. Проверьте разрешения и выберите доступное место.'],
 [/^no such file or directory\b/i,'Файл не найден. Проверьте, что он не был перемещён или удалён.']
];
const fallback='Операция не выполнена. Проверьте исходные данные и повторите действие.';
function userError(error){
 const value=typeof error==='string'?error:typeof error?.message==='string'?error.message:'';
 const raw=value.trim().replace(/^(?:Error:\s*)+/i,'');
 for(const [pattern,message] of rules){
  if(!pattern.test(raw))continue;
  const cell=raw.match(/(?:^|[^A-Za-z0-9_])([A-Z]{1,3}[1-9][0-9]{0,6})(?![A-Za-z0-9_])/);
  // Show only a validated Excel coordinate, never a raw exception or path.
  return message+(cell?' Ячейка Excel: '+cell[1]+'.':'');
 }
 // Existing human-readable Russian domain errors can include actionable context.
 // Keep them, but not arbitrary SQL errors with a Russian prefix.
 if(/[А-Яа-яЁё]/.test(raw)&&
    !/(?:\b(?:SELECT|INSERT|UPDATE|DELETE|SQL|token|stack|traceback)\b|sqlite error|at line \d+)/i.test(raw))
   return raw;
 return fallback;
}
root.analizUserError=userError;
if(typeof module!=='undefined'&&module.exports)module.exports={userError};
})(typeof window!=='undefined'?window:globalThis);
