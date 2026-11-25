//! Selective transfer between two SQLite workspaces.
//! Only transfers into the shared workspace use the publication pipeline.
use super::{err, read_model, valid_date, Storage, REPORT_TYPES};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const TRANSFER_PAGE: usize = 25;
const MAX_REPORT_BYTES: usize = 16 * 1024 * 1024;
// Batch JSON is decoded and indexed in one transaction; cap cumulative input.
const MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TransferDirection {
    #[default]
    ToShared,
    ToPersonal,
}

impl TransferDirection {
    fn target(self) -> &'static str {
        match self { Self::ToShared => "общей", Self::ToPersonal => "личной" }
    }

    fn backup_prefix(self) -> &'static str {
        match self {
            Self::ToShared => "shared-before-personal-transfer",
            Self::ToPersonal => "personal-before-shared-transfer",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferPeriod {
    pub from: String,
    pub to: String,
}

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
    pub initializes_organizations: bool,
    pub organization_warning: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferReceipt {
    pub imported: usize,
    pub revision: i64,
    pub backup_path: String,
}

fn overlap(conn: &rusqlite::Connection, id: &str, kind: &str, start: &str, end: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM reports
          WHERE id=?1 OR (report_type=?2 AND period_start<=?3 AND period_end>=?4))",
        params![id, kind, end, start], |r| r.get(0),
    ).map_err(err)
}

fn clean_source_name(raw: Option<String>) -> Option<String> {
    let raw = raw?;
    let name = raw.rsplit(['/', '\\']).next()?.trim();
    (!name.is_empty() && !name.contains(':')).then(|| name.to_owned())
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

fn organization_settings(conn: &rusqlite::Connection) -> Result<(Vec<String>, bool), String> {
    let stored: Option<String> = conn.query_row(
        "SELECT CAST(json AS TEXT) FROM kv WHERE name='settings'", [], |r| r.get(0)
    ).optional().map_err(err)?;
    let settings: Value = match &stored {
        Some(s) => serde_json::from_str(s).map_err(err)?,
        None => super::default_settings(),
    };
    let list = settings["orgNames"].as_array().ok_or("У базы отсутствует список подразделений.")?;
    if list.len() != 4 || list.iter().any(|v| !v.is_string()) {
        return Err("Ожидаются четыре названия подразделений.".into());
    }
    Ok((list.iter().map(|v| v.as_str().unwrap().to_owned()).collect(), stored.is_some()))
}

fn validate_report_organizations(json: &Value, allowed: &HashSet<String>) -> Result<(), String> {
    let rows = json["rows"].as_array().ok_or("Отчёт не содержит массив работ.")?;
    for row in rows {
        let org = row["organization"].as_str().ok_or("У работы не задано подразделение.")?;
        if !allowed.contains(org) {
            return Err(format!("Подразделение «{org}» отсутствует в настройках выбранной базы. Исправьте названия до переноса."));
        }
    }
    Ok(())
}


/// Compare verified report identity and content without machine-local file paths.
/// This distinguishes an already transferred source anchor from a conflicting ID.
fn same_report(source: &rusqlite::Connection, target: &rusqlite::Connection, id: &str) -> Result<bool, String> {
    fn read(conn: &rusqlite::Connection, id: &str) -> Result<Option<(String,String,String,Value)>,String> {
        let row: Option<(String,String,String,Vec<u8>)> = conn.query_row(
            "SELECT report_type,period_start,period_end,CAST(json AS BLOB) FROM reports WHERE id=?1",
            [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))
        ).optional().map_err(err)?;
        row.map(|(kind,start,end,raw)| {
            let mut json: Value = serde_json::from_slice(&raw).map_err(err)?;
            if let Some(object) = json.as_object_mut() {
                object.remove("desktopFolder");
                object.remove("desktopSource");
            }
            Ok((kind,start,end,json))
        }).transpose()
    }
    match (read(source,id)?, read(target,id)?) {
        (Some(a),Some(b)) => Ok(a==b),
        _ => Ok(false)
    }
}

impl Storage {
    /// Page through reports in the source with destination conflict markers.
    /// No private Excel path or report body is sent to the webview.
    pub fn report_transfer_preview(
        &self, source_store: &Storage, page: usize, direction: TransferDirection, period: Option<&TransferPeriod>,
    ) -> Result<TransferPreview, String> {
        if self.db_path() == source_store.db_path() {
            return Err("Подключите общую базу перед переносом.".into());
        }
        let offset = page.checked_mul(TRANSFER_PAGE)
            .and_then(|n| i64::try_from(n).ok())
            .ok_or("Номер страницы слишком велик.")?;
        let (from, to) = match period {
            Some(p) if valid_date(&p.from) && valid_date(&p.to) && p.from <= p.to =>
                (Some(p.from.as_str()), Some(p.to.as_str())),
            Some(_) => return Err("Укажите корректный период переноса отчётов.".into()),
            None => (None, None),
        };
        let source = source_store.connect()?;
        let dest = self.connect()?;
        let source_revision = read_model::revision(&source)?;
        let target_revision = read_model::revision(&dest)?;
        let (source_orgs, _) = organization_settings(&source)?;
        let (target_orgs, target_settings_exist) = organization_settings(&dest)?;
        let target_empty: bool = dest.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM reports)", [], |r| r.get(0)
        ).map_err(err)?;
        // The initial blank team can inherit the personal organization labels,
        // but existing team settings/reports must not be overwritten.
        let initializes_organizations = target_empty && !target_settings_exist;
        let allowed: HashSet<String> = if initializes_organizations {
            source_orgs.iter().cloned().collect()
        } else {
            target_orgs.iter().cloned().collect()
        };
        let total_reports: i64 = source.query_row(
            "SELECT COUNT(*) FROM reports WHERE (?1 IS NULL OR period_start >= ?1) AND (?2 IS NULL OR period_end <= ?2)",
            params![from,to], |r| r.get(0)
        ).map_err(err)?;
        let mut stmt = source.prepare(
            "SELECT r.id,r.report_type,r.period_start,r.period_end,r.source_name,i.row_count
             FROM reports r LEFT JOIN report_row_index i ON i.report_id=r.id
             WHERE (?1 IS NULL OR r.period_start >= ?1) AND (?2 IS NULL OR r.period_end <= ?2)
             ORDER BY r.period_start,r.id LIMIT ?3 OFFSET ?4"
        ).map_err(err)?;
        let iter = stmt.query_map(params![from,to,TRANSFER_PAGE as i64,offset], |r| {
            Ok(TransferRow {
                id: r.get(0)?, report_type: r.get(1)?, start: r.get(2)?, end: r.get(3)?,
                source_name: clean_source_name(r.get(4)?), row_count: r.get(5)?, status: String::new(),
            })
        }).map_err(err)?;
        let mut rows = Vec::new();
        for row in iter {
            let mut row = row.map_err(err)?;
            row.status = if overlap(&dest,&row.id,&row.report_type,&row.start,&row.end)? {
                if same_report(&source,&dest,&row.id)? { "duplicate" } else { "conflict" }
            } else {
                // Check indexed rows without sending sensitive JSON to the UI.
                // Unindexed legacy reports are validated again during apply.
                let mut stmt = source.prepare(
                    "SELECT DISTINCT organization FROM report_rows WHERE report_id=?1"
                ).map_err(err)?;
                let mut orgs = stmt.query([&row.id]).map_err(err)?;
                let mut compatible = true;
                while let Some(org) = orgs.next().map_err(err)? {
                    let label: String = org.get(0).map_err(err)?;
                    if !allowed.contains(&label) { compatible = false; break; }
                }
                if compatible { "ready" } else { "organization_conflict" }
            }.into();
            rows.push(row);
        }
        Ok(TransferPreview {
            source_revision, target_revision, total_reports,
            page_index: page, page_size: TRANSFER_PAGE,
            has_next: offset + (rows.len() as i64) < total_reports, rows,
            initializes_organizations,
            organization_warning: if initializes_organizations {
                Some("Пустая база назначения получит названия четырёх подразделений из базы-источника.".into())
            } else if source_orgs != target_orgs {
                Some(format!("Названия подразделений двух баз различаются. Для переноса подразделения должны быть известны {} базе.", direction.target()))
            } else { None },
        })
    }

    /// Revalidate both revisions and every selected report, then add only new reports.
    /// A verified private backup is written BEFORE the destination transaction.
    /// The old published network generation is also left intact by the caller.
    pub fn import_selected_reports(
        &self, source_store: &Storage, source_revision: i64, target_revision: i64,
        selected_ids: &[String], backup_dir: &Path, direction: TransferDirection,
    ) -> Result<TransferReceipt, String> {
        if self.db_path() == source_store.db_path() {
            return Err("Базы источника и назначения должны различаться.".into());
        }
        if selected_ids.is_empty() || selected_ids.len() > TRANSFER_PAGE {
            return Err("Выберите от 1 до 25 отчётов за один перенос.".into());
        }
        let ids: HashSet<&str> = selected_ids.iter().map(String::as_str).collect();
        if ids.len() != selected_ids.len() || selected_ids.iter().any(|id| id.is_empty() || id.len() > 65536) {
            return Err("Недопустимые или повторяющиеся идентификаторы отчётов.".into());
        }
        let source = source_store.connect()?;
        let dest = self.connect()?;
        read_model::check_revision(&source, source_revision)?;
        read_model::check_revision(&dest, target_revision)?;
        let (source_orgs, _) = organization_settings(&source)?;
        let (target_orgs, target_settings_exist) = organization_settings(&dest)?;
        let target_empty: bool = dest.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM reports)", [], |r| r.get(0)
        ).map_err(err)?;
        let seed_organizations = target_empty && !target_settings_exist;
        let source_allowed: HashSet<String> = source_orgs.iter().cloned().collect();
        let target_allowed: HashSet<String> = if seed_organizations {
            source_allowed.clone()
        } else {
            target_orgs.into_iter().collect()
        };

        // Validate all selected records before taking the target backup or writing.
        let mut batch = Vec::with_capacity(selected_ids.len());
        let mut batch_bytes = 0usize;
        for id in selected_ids {
            let row: Option<(String,String,String,Option<String>,Vec<u8>)> = source.query_row(
                "SELECT report_type,period_start,period_end,source_name,CAST(json AS BLOB) FROM reports WHERE id=?1",
                [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
            ).optional().map_err(err)?;
            let (kind,start,end,source_name,bytes) = row.ok_or("Отчёт отсутствует в исходной базе. Повторите проверку.")?;
            if !REPORT_TYPES.contains(&kind.as_str()) || !valid_date(&start) || !valid_date(&end) || start > end {
                return Err("Некорректный тип или даты исходного отчёта.".into());
            }
            if bytes.len() > MAX_REPORT_BYTES {
                return Err(format!("Отчёт {id} слишком велик для выборочного переноса (16 МиБ)."));
            }
            batch_bytes = batch_bytes.checked_add(bytes.len()).ok_or("Размер переносимых отчётов слишком велик.")?;
            if batch_bytes > MAX_BATCH_BYTES {
                return Err("Выбранные отчёты превышают 64 МиБ. Выберите меньше отчётов и повторите перенос.".into());
            }
            if overlap(&dest,id,&kind,&start,&end)? {
                return Err(format!("Период отчёта {id} уже представлен в {} базе. Ничего не изменено.", direction.target()));
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
            validate_report_organizations(&json, &source_allowed)?;
            validate_report_organizations(&json, &target_allowed)?;
            // Never publish an origin machine's local Excel folder grant.
            if let Some(object) = json.as_object_mut() {
                object.remove("desktopFolder");
            }
            // Some old personal reports used an absolute workbook path as
            // their provenance label. The network recipient only needs its
            // filename; never transfer private local folder paths.
            let source_name = clean_source_name(source_name);
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
                let anchor = anchor.map_err(err)?;
                // Large daily/quarterly books may contain far more than 25
                // linked reports. The anchor can be moved in an earlier verified
                // batch and reused only if the source report content matches it.
                if !ids.contains(anchor.as_str()) && !same_report(&source,&dest,&anchor)? {
                    return Err("Сначала перенесите исходный диагностический отчёт или выберите его вместе с зависимыми записями.".into());
                }
            }
            batch.push(Candidate { id:id.clone(), kind, start, end, source_name, json });
        }
        drop(dest);
        // Keep the backup on the originating computer, outside the network share.
        fs::create_dir_all(backup_dir).map_err(err)?;
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).map_err(err)?.as_nanos();
        let backup = backup_dir.join(format!("{}-{}-{unique}.sqlite3",direction.backup_prefix(),std::process::id()));
        let backed_revision = self.backup_sqlite_verified(&backup)?;
        if backed_revision != target_revision {
            return Err("База назначения изменилась во время создания резервной копии.".into());
        }

        let mut dest = self.connect()?;
        let tx = dest.transaction_with_behavior(TransactionBehavior::Immediate).map_err(err)?;
        read_model::check_revision(&tx,target_revision)?;
        read_model::check_revision(&source,source_revision)?;
        if seed_organizations {
            // No team settings or reports exist: carry only the organization names
            // necessary to make the imported report rows visible. Other settings stay private.
            let names = serde_json::json!({
                "orgNames": source_orgs, "groups": [], "catalog": []
            });
            tx.execute(
                "INSERT INTO kv(name,json) VALUES('settings',?1)",
                [serde_json::to_string(&names).map_err(err)?]
            ).map_err(err)?;
        }
        // Recheck every conflict BEFORE inserting any selected report. Multiple
        // source reports from the SAME period are allowed in one verified batch.
        for item in &batch {
            if overlap(&tx,&item.id,&item.kind,&item.start,&item.end)? {
                return Err("База назначения изменилась или содержит конфликт. Повторите предпросмотр.".into());
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
