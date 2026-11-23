//! Selective personal -> shared import. It never deletes either source or destination rows.
//! The caller must publish the target through shared_runtime::Runtime::execute(Write).
use super::{err, read_model, valid_date, Storage, REPORT_TYPES};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const TRANSFER_PAGE: usize = 25;
const MAX_REPORT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferRow {
    pub id: String,
    pub report_type: String,
    pub start: String,
    pub end: String,
    pub source_name: Option<String>,
    pub row_count: Option<i64>,
    pub status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferPreview {
    pub source_revision: i64,
    pub target_revision: i64,
    pub total_reports: i64,
    pub page_index: usize,
    pub page_size: usize,
    pub rows: Vec<TransferRow>,
    pub has_next: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferReceipt {
    pub imported: usize,
    pub revision: i64,
    pub backup_path: String,
}

fn overlap(conn: &rusqlite::Connection, id: &str, start: &str, end: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM reports
          WHERE id=?1 OR (period_start<=?2 AND period_end>=?3))",
        params![id, end, start], |r| r.get(0),
    ).map_err(err)
}

#[derive(Debug)]
struct Candidate {
    id: String,
    kind: String,
    start: String,
    end: String,
    source_name: Option<String>,
    json: Value,
}

impl Storage {
    /// Page through the personal reports with destination conflict markers.
    /// No private Excel path or report body is sent to the webview.
    pub fn personal_transfer_preview(
        &self, personal: &Storage, page: usize,
    ) -> Result<TransferPreview, String> {
        if self.db_path() == personal.db_path() {
            return Err("Подключите общую базу перед переносом.".into());
        }
        let offset = page.checked_mul(TRANSFER_PAGE)
            .and_then(|n| i64::try_from(n).ok())
            .ok_or("Номер страницы слишком велик.")?;
        let source = personal.connect()?;
        let dest = self.connect()?;
        let source_revision = read_model::revision(&source)?;
        let target_revision = read_model::revision(&dest)?;
        let total_reports: i64 = source.query_row("SELECT COUNT(*) FROM reports", [], |r| r.get(0)).map_err(err)?;
        let mut stmt = source.prepare(
            "SELECT r.id,r.report_type,r.period_start,r.period_end,r.source_name,i.row_count
             FROM reports r LEFT JOIN report_row_index i ON i.report_id=r.id
             ORDER BY r.period_start,r.id LIMIT ?1 OFFSET ?2"
        ).map_err(err)?;
        let iter = stmt.query_map(params![TRANSFER_PAGE as i64,offset], |r| {
            Ok(TransferRow {
                id: r.get(0)?, report_type: r.get(1)?, start: r.get(2)?, end: r.get(3)?,
                source_name: r.get(4)?, row_count: r.get(5)?, status: String::new(),
            })
        }).map_err(err)?;
        let mut rows = Vec::new();
        for row in iter {
            let mut row = row.map_err(err)?;
            row.status = if overlap(&dest,&row.id,&row.start,&row.end)? { "conflict" } else { "ready" }.into();
            rows.push(row);
        }
        Ok(TransferPreview {
            source_revision, target_revision, total_reports,
            page_index: page, page_size: TRANSFER_PAGE,
            has_next: offset + (rows.len() as i64) < total_reports, rows,
        })
    }

    /// Revalidate both revisions and every selected report, then add only new reports.
    /// A verified private backup is written BEFORE the destination transaction.
    /// The old published network generation is also left intact by the caller.
    pub fn import_selected_personal_reports(
        &self, personal: &Storage, source_revision: i64, target_revision: i64,
        selected_ids: &[String], backup_dir: &Path,
    ) -> Result<TransferReceipt, String> {
        if self.db_path() == personal.db_path() {
            return Err("Личную базу нельзя переносить в саму себя.".into());
        }
        if selected_ids.is_empty() || selected_ids.len() > TRANSFER_PAGE {
            return Err("Выберите от 1 до 25 отчётов за один перенос.".into());
        }
        let ids: HashSet<&str> = selected_ids.iter().map(String::as_str).collect();
        if ids.len() != selected_ids.len() || selected_ids.iter().any(|id| id.is_empty() || id.len() > 65536) {
            return Err("Недопустимые или повторяющиеся идентификаторы отчётов.".into());
        }
        let source = personal.connect()?;
        let dest = self.connect()?;
        read_model::check_revision(&source, source_revision)?;
        read_model::check_revision(&dest, target_revision)?;

        // Validate all selected records before taking the target backup or writing.
        let mut batch = Vec::with_capacity(selected_ids.len());
        for id in selected_ids {
            let row: Option<(String,String,String,Option<String>,Vec<u8>)> = source.query_row(
                "SELECT report_type,period_start,period_end,source_name,json FROM reports WHERE id=?1",
                [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
            ).optional().map_err(err)?;
            let (kind,start,end,source_name,bytes) = row.ok_or("Отчёт отсутствует в личной базе. Повторите проверку.")?;
            if !REPORT_TYPES.contains(&kind.as_str()) || !valid_date(&start) || !valid_date(&end) || start > end {
                return Err("Некорректный тип или даты личного отчёта.".into());
            }
            if bytes.len() > MAX_REPORT_BYTES {
                return Err(format!("Отчёт {id} слишком велик для выборочного переноса (16 МиБ)."));
            }
            if overlap(&dest,id,&start,&end)? {
                return Err(format!("Период отчёта {id} уже представлен в общей базе. Ничего не изменено."));
            }
            let mut json: Value = serde_json::from_slice(&bytes).map_err(err)?;
            if json["id"].as_str() != Some(id.as_str())
                || json["type"].as_str() != Some(kind.as_str())
                || json["start"].as_str() != Some(start.as_str())
                || json["end"].as_str() != Some(end.as_str())
                || !json["rows"].is_array()
            {
                return Err(format!("Некорректная структура отчёта {id}."));
            }
            // Never publish an origin machine's local Excel folder grant.
            if let Some(object) = json.as_object_mut() {
                object.remove("desktopFolder");
            }
            // Some old personal reports used an absolute workbook path as
            // their provenance label. The network recipient only needs its
            // filename; never transfer private local folder paths.
            let source_name = source_name.and_then(|raw| {
                let name = raw.rsplit(['/', '\\']).next()?.trim().to_owned();
                (!name.is_empty() && !name.contains(':')).then_some(name)
            });
            if let Some(object) = json.as_object_mut() {
                if let Some(clean) = source_name.as_ref() {
                    if let Some(Value::String(value)) = object.get_mut("desktopSource") {
                        *value = clean.clone();
                    }
                } else {
                    object.remove("desktopSource");
                }
            }
            let mut links = source.prepare(
                "SELECT anchor_id FROM report_source_diagnostic_links WHERE report_id=?1"
            ).map_err(err)?;
            let anchors = links.query_map([id], |r| r.get::<_,String>(0)).map_err(err)?;
            for anchor in anchors {
                if !ids.contains(anchor.map_err(err)?.as_str()) {
                    return Err("Связанные диагностические отчёты должны переноситься вместе. Выберите также исходный отчёт и повторите.".into());
                }
            }
            batch.push(Candidate { id:id.clone(), kind, start, end, source_name, json });
        }
        drop(dest);
        // Keep the backup on the originating computer, outside the network share.
        fs::create_dir_all(backup_dir).map_err(err)?;
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).map_err(err)?.as_nanos();
        let backup = backup_dir.join(format!("shared-before-personal-transfer-{}-{unique}.sqlite3",std::process::id()));
        let backed_revision = self.backup_sqlite_verified(&backup)?;
        if backed_revision != target_revision {
            return Err("Общая база изменилась во время создания резервной копии.".into());
        }

        let mut dest = self.connect()?;
        let tx = dest.transaction_with_behavior(TransactionBehavior::Immediate).map_err(err)?;
        read_model::check_revision(&tx,target_revision)?;
        read_model::check_revision(&source,source_revision)?;
        // Recheck every conflict BEFORE inserting any selected report. Multiple
        // source reports from the SAME period are allowed in one verified batch.
        for item in &batch {
            if overlap(&tx,&item.id,&item.start,&item.end)? {
                return Err("Общая база изменилась или содержит конфликт. Повторите предпросмотр.".into());
            }
        }
        for item in &batch {
            tx.execute(
                "INSERT INTO reports(id,report_type,period_start,period_end,source_name,json)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![item.id,item.kind,item.start,item.end,item.source_name,item.json.to_string()],
            ).map_err(err)?;
            read_model::index_value(&tx,&item.id,&item.json)?;
            let indexed: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM report_row_index WHERE report_id=?1)",
                [&item.id],|r|r.get(0),
            ).map_err(err)?;
            if !indexed {
                return Err("Индекс перенесённого отчёта недоступен. Ничего не опубликовано.".into());
            }
            if item.source_name.is_some() {
                tx.execute(
                    "INSERT INTO report_source_ownership(report_id,source_name,folder,mode)
                     VALUES(?1,?2,'','manual')",
                    params![item.id,item.source_name],
                ).map_err(err)?;
            }
        }
        for item in &batch {
            let mut stmt = source.prepare(
                "SELECT anchor_id FROM report_source_diagnostic_links WHERE report_id=?1"
            ).map_err(err)?;
            let anchors = stmt.query_map([&item.id], |r| r.get::<_,String>(0)).map_err(err)?;
            for anchor in anchors {
                tx.execute(
                    "INSERT INTO report_source_diagnostic_links(report_id,anchor_id) VALUES(?1,?2)",
                    params![item.id,anchor.map_err(err)?],
                ).map_err(err)?;
            }
        }
        let revision = read_model::revision(&tx)?;
        tx.commit().map_err(err)?;
        Ok(TransferReceipt {
            imported:batch.len(), revision, backup_path:backup.to_string_lossy().into_owned(),
        })
    }
}
