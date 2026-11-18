//! Declarative metric-report families, separate from legacy day/week/month reports.
//! A family is defined in metric-kinds.json; all families share one bounded,
//! normalized SQLite store and a preview-before-insert import protocol.
use super::*;
use rusqlite::TransactionBehavior;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::Read;

const MAX_IMPORT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ROWS: usize = 10_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricField {
    pub id: String,
    pub name: String,
    pub unit: String,
    pub operation: String,
    #[serde(default)]
    pub numerator: Option<String>,
    #[serde(default)]
    pub denominator: Option<String>,
    #[serde(default)]
    pub factor: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricKind {
    pub id: String,
    pub name: String,
    pub dimension_label: String,
    pub metrics: Vec<MetricField>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetricRow {
    organization: Option<String>,
    dimension: String,
    values: BTreeMap<String, f64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MetricDocument {
    kind: String,
    id: String,
    start: String,
    end: String,
    source_name: String,
    rows: Vec<MetricRow>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricReview {
    pub kind: String,
    pub name: String,
    pub report_id: String,
    pub start: String,
    pub end: String,
    pub row_count: usize,
    pub values: BTreeMap<String, Option<f64>>,
    pub review_token: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricReceipt {
    pub report_id: String,
    pub row_count: usize,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricSummary {
    pub kind: String,
    pub report_count: i64,
    pub row_count: i64,
    pub values: BTreeMap<String, Option<f64>>,
}

fn symbol(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 &&
    s.as_bytes()[0].is_ascii_lowercase() &&
    s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}
fn label(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 240 && !s.contains('\0')
}
fn catalog() -> Result<Vec<MetricKind>, String> {
    let kinds: Vec<MetricKind> = serde_json::from_str(include_str!("metric-kinds.json")).map_err(err)?;
    if kinds.is_empty() || kinds.len() > 32 { return Err("Invalid report-kind catalog".into()); }
    let mut seen_kinds = HashSet::new();
    for kind in &kinds {
        if !symbol(&kind.id) || !label(&kind.name) || !label(&kind.dimension_label)
            || !seen_kinds.insert(kind.id.as_str())
            || kind.metrics.is_empty() || kind.metrics.len() > 16 {
            return Err("Invalid or duplicate report kind".into());
        }
        let mut metric_names = HashSet::new();
        for metric in &kind.metrics {
            if !symbol(&metric.id) || !label(&metric.name) || metric.unit.len() > 64
                || !metric_names.insert(metric.id.as_str()) {
                return Err("Invalid or duplicate metric definition".into());
            }
        }
        if !kind.metrics.iter().any(|f| f.operation == "sum") {
            return Err("A kind must contain at least one additive metric".into());
        }
        for metric in &kind.metrics {
            match metric.operation.as_str() {
                "sum" if metric.numerator.is_none() && metric.denominator.is_none()
                    && metric.factor.is_none() => (),
                "ratio" if metric.numerator.as_deref().is_some_and(|id|
                        kind.metrics.iter().any(|m| m.id == id && m.operation == "sum"))
                    && metric.denominator.as_deref().is_some_and(|id|
                        kind.metrics.iter().any(|m| m.id == id && m.operation == "sum"))
                    && metric.factor.unwrap_or(1.0).is_finite()
                    && metric.factor.unwrap_or(1.0) > 0.0
                    && metric.factor.unwrap_or(1.0) <= 100.0 => (),
                _ => return Err("Unsupported metric aggregation or formula".into()),
            }
        }
    }
    Ok(kinds)
}
pub fn kinds() -> Result<Vec<MetricKind>, String> { catalog() }

fn kind_by_id(id: &str) -> Result<MetricKind, String> {
    catalog()?.into_iter().find(|k| k.id == id)
        .ok_or_else(|| "Unknown report kind".into())
}

pub(super) fn initialize(conn: &Connection) -> Result<(), String> {
    // Existing reports, index tables and their revision remain untouched.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS metric_report_docs(
            id TEXT PRIMARY KEY,kind TEXT NOT NULL,start TEXT NOT NULL,end TEXT NOT NULL,
            source_name TEXT NOT NULL,row_count INTEGER NOT NULL);
         CREATE INDEX IF NOT EXISTS metric_report_range ON metric_report_docs(kind,start,end);
         CREATE TABLE IF NOT EXISTS metric_report_values(
            report_id TEXT NOT NULL,row_no INTEGER NOT NULL,
            organization TEXT,dimension TEXT NOT NULL,
            metric TEXT NOT NULL,value REAL NOT NULL,
            PRIMARY KEY(report_id,row_no,metric)
         ) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS metric_values_by_report ON metric_report_values(report_id,metric);
         CREATE TABLE IF NOT EXISTS metric_catalog_revision(
            singleton INTEGER PRIMARY KEY CHECK(singleton=1),revision INTEGER NOT NULL);
         INSERT OR IGNORE INTO metric_catalog_revision VALUES(1,0);"
    ).map_err(err)
}
fn metric_revision(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT revision FROM metric_catalog_revision WHERE singleton=1",[],|r|r.get(0))
        .map_err(err)
}
fn read_source(source: &str) -> Result<(Vec<u8>, MetricDocument, MetricKind), String> {
    let path = Path::new(source);
    if !path.is_absolute() || !path.extension().and_then(|s|s.to_str())
        .is_some_and(|s|s.eq_ignore_ascii_case("json")) {
        return Err("Select an absolute .json report file".into());
    }
    let metadata = fs::symlink_metadata(path).map_err(err)?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_IMPORT_BYTES {
        return Err("A regular JSON file <=8 MiB is required".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(err)?.take(MAX_IMPORT_BYTES+1)
        .read_to_end(&mut bytes).map_err(err)?;
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        return Err("JSON file exceeds 8 MiB".into());
    }
    let document: MetricDocument = serde_json::from_slice(&bytes).map_err(err)?;
    let kind = kind_by_id(&document.kind)?;
    if !label(&document.id) || document.id.len() > 128
        || !valid_date(&document.start) || !valid_date(&document.end)
        || document.start > document.end || !label(&document.source_name)
        || document.rows.is_empty() || document.rows.len() > MAX_ROWS {
        return Err("Invalid report ID, period, source or row count".into());
    }
    let required: HashSet<_> = kind.metrics.iter().filter(|f| f.operation == "sum")
        .map(|f|f.id.as_str()).collect();
    for row in &document.rows {
        if !label(&row.dimension) || row.organization.as_deref().is_some_and(|o| !label(o))
            || row.values.len() != required.len()
            || row.values.iter().any(|(id,value)| !required.contains(id.as_str())
                || !value.is_finite() || value.abs() > 1e12) {
            return Err("Report row must provide all and only declared additive metrics".into());
        }
    }
    Ok((bytes, document, kind))
}
fn derived(kind: &MetricKind, sums: &BTreeMap<String, f64>) -> Result<BTreeMap<String,Option<f64>>,String> {
    let mut values = BTreeMap::new();
    for metric in &kind.metrics {
        let value = if metric.operation == "sum" {
            sums.get(&metric.id).copied()
        } else {
            let numerator = sums.get(metric.numerator.as_deref().unwrap_or(""));
            let denominator = sums.get(metric.denominator.as_deref().unwrap_or(""));
            match (numerator, denominator) {
                (Some(n),Some(d)) if *d != 0.0 => Some(*n / *d * metric.factor.unwrap_or(1.0)),
                _ => None,
            }
        };
        if value.is_some_and(|v| !v.is_finite()) {
            return Err("Metric aggregation overflow".into());
        }
        values.insert(metric.id.clone(),value);
    }
    Ok(values)
}
fn review_token(bytes: &[u8], revision: i64, kind: &MetricKind) -> Result<String,String> {
    let mut hash = Sha256::new();
    hash.update(b"metric-report-review-v1\0");
    hash.update(bytes);
    hash.update(revision.to_le_bytes());
    hash.update(serde_json::to_vec(kind).map_err(err)?);
    Ok(format!("{:x}",hash.finalize()))
}
fn preview_values(document: &MetricDocument, kind: &MetricKind)
    -> Result<BTreeMap<String,Option<f64>>,String> {
    let mut sums=BTreeMap::new();
    for row in &document.rows {
        for (metric,value) in &row.values {
            let total = sums.entry(metric.clone()).or_insert(0.0);
            *total += *value;
            if !total.is_finite() { return Err("Metric aggregation overflow".into()); }
        }
    }
    derived(kind,&sums)
}

impl Storage {
    pub fn metric_preview(&self, source: &str) -> Result<MetricReview,String> {
        let _guard=self.gate.lock().map_err(err)?;
        let (bytes, document, kind)=read_source(source)?;
        let conn=self.connect()?;
        let exists:i64=conn.query_row(
            "SELECT COUNT(*) FROM metric_report_docs WHERE id=?1",[&document.id],|r|r.get(0)
        ).map_err(err)?;
        if exists != 0 { return Err("Report ID already exists; no replacement allowed".into()); }
        let revision=metric_revision(&conn)?;
        Ok(MetricReview {
            kind:kind.id.clone(),name:kind.name.clone(),report_id:document.id.clone(),
            start:document.start.clone(),end:document.end.clone(),
            row_count:document.rows.len(),values:preview_values(&document,&kind)?,
            review_token:review_token(&bytes,revision,&kind)?,
        })
    }
    pub fn metric_apply(&self, source: &str, expected_token: &str) -> Result<MetricReceipt,String> {
        if expected_token.len()!=64 || !expected_token.bytes().all(|c|c.is_ascii_hexdigit()){
            return Err("Invalid review token".into());
        }
        let _guard=self.gate.lock().map_err(err)?;
        let (bytes,document,kind)=read_source(source)?;
        let mut conn=self.connect()?;
        let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(err)?;
        let revision=metric_revision(&tx)?;
        if review_token(&bytes,revision,&kind)? != expected_token {
            return Err("Source or metric data changed: preview again".into());
        }
        // Insert-only. A collision fails and rolls back all cells.
        tx.execute(
            "INSERT INTO metric_report_docs(id,kind,start,end,source_name,row_count) VALUES(?1,?2,?3,?4,?5,?6)",
            params![document.id,document.kind,document.start,document.end,
                document.source_name,document.rows.len() as i64]
        ).map_err(err)?;
        {
            let mut insert=tx.prepare(
                "INSERT INTO metric_report_values(report_id,row_no,organization,dimension,metric,value)
                 VALUES(?1,?2,?3,?4,?5,?6)"
            ).map_err(err)?;
            for (index,row) in document.rows.iter().enumerate() {
                for (metric,value) in &row.values {
                    insert.execute(params![document.id,index as i64,row.organization,row.dimension,metric,value])
                        .map_err(err)?;
                }
            }
        }
        tx.execute("UPDATE metric_catalog_revision SET revision=revision+1 WHERE singleton=1",[])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(MetricReceipt{report_id:document.id,row_count:document.rows.len()})
    }
    pub fn metric_summary(&self, id: &str, start: &str, end: &str) -> Result<MetricSummary,String> {
        let kind=kind_by_id(id)?;
        if !valid_date(start) || !valid_date(end) || start>end {
            return Err("Invalid metric report date range".into());
        }
        let _guard=self.gate.lock().map_err(err)?;
        let conn=self.connect()?;
        let (report_count,row_count):(i64,i64)=conn.query_row(
            "SELECT COUNT(*),COALESCE(SUM(row_count),0) FROM metric_report_docs
             WHERE kind=?1 AND start>=?2 AND end<=?3",
            params![id,start,end],|r|Ok((r.get(0)?,r.get(1)?))
        ).map_err(err)?;
        let mut sums=BTreeMap::new();
        let mut stmt=conn.prepare(
            "SELECT v.metric,SUM(v.value) FROM metric_report_values v
             JOIN metric_report_docs d ON d.id=v.report_id
             WHERE d.kind=?1 AND d.start>=?2 AND d.end<=?3
             GROUP BY v.metric"
        ).map_err(err)?;
        let mut rows=stmt.query(params![id,start,end]).map_err(err)?;
        while let Some(row)=rows.next().map_err(err)? {
            let metric:String=row.get(0).map_err(err)?;
            let value:f64=row.get(1).map_err(err)?;
            if !value.is_finite() {return Err("Metric aggregation overflow".into());}
            sums.insert(metric,value);
        }
        Ok(MetricSummary{kind:id.into(),report_count,row_count,values:derived(&kind,&sums)?})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    fn fixture(dir: &Path, id: &str) -> String {
        let file=dir.join("metric-source.json");
        fs::write(&file,serde_json::json!({
            "kind":"labor","id":id,"start":"2026-11-01","end":"2026-11-30",
            "sourceName":"hours.xlsx","rows":[
                {"organization":"СМУ16","dimension":"Монтаж","values":{"hours":2.0,"units":3.0}},
                {"organization":"СМУ16","dimension":"Ремонт","values":{"hours":6.0,"units":12.0}}
            ]
        }).to_string()).unwrap();
        file.to_str().unwrap().to_owned()
    }
    #[test]
    fn declarative_catalog_supports_independent_metrics() {
        let all=kinds().unwrap();
        assert!(all.iter().any(|k|k.id=="labor"));
        assert!(all.iter().any(|k|k.id=="plan_fact"));
        let kind=kind_by_id("plan_fact").unwrap();
        let sums=BTreeMap::from([("plan".into(),10.0),("actual".into(),8.0)]);
        assert_eq!(derived(&kind,&sums).unwrap()["completion"],Some(80.0));
        assert_eq!(derived(&kind,&BTreeMap::from([("plan".into(),0.0),("actual".into(),8.0)])).unwrap()["completion"],None);
    }
    #[test]
    fn import_and_summary_use_sum_of_inputs_not_average_of_ratios() {
        let dir=tempdir().unwrap();
        let store=Storage::new(dir.path().join("data")).unwrap();
        let path=fixture(dir.path(),"labor-1");
        let preview=store.metric_preview(&path).unwrap();
        assert_eq!(preview.values["units_per_hour"],Some(1.875));
        let result=store.metric_apply(&path,&preview.review_token).unwrap();
        assert_eq!(result.row_count,2);
        let summary=store.metric_summary("labor","2026-01-01","2026-12-31").unwrap();
        assert_eq!(summary.report_count,1);
        assert_eq!(summary.values["hours"],Some(8.0));
        assert_eq!(summary.values["units_per_hour"],Some(1.875));
        let empty=store.metric_summary("labor","2025-01-01","2025-12-31").unwrap();
        assert_eq!(empty.values["hours"],None);
        assert_eq!(empty.values["units_per_hour"],None);
        assert_eq!(store.load().unwrap()["reports"].as_array().unwrap().len(),0);
    }
    #[test]
    fn rejects_stale_review_and_duplicate_without_changing_stored_rows() {
        let dir=tempdir().unwrap();
        let store=Storage::new(dir.path().join("data")).unwrap();
        let path=fixture(dir.path(),"one");
        let first=store.metric_preview(&path).unwrap();
        fs::write(&path,fs::read_to_string(&path).unwrap().replace("3.0","4.0")).unwrap();
        assert!(store.metric_apply(&path,&first.review_token).is_err());
        let fresh=store.metric_preview(&path).unwrap();
        store.metric_apply(&path,&fresh.review_token).unwrap();
        assert!(store.metric_apply(&path,&fresh.review_token).is_err());
        assert!(store.metric_preview(&path).is_err());
        let current=store.metric_summary("labor","2026-01-01","2026-12-31").unwrap();
        assert_eq!(current.report_count,1);
        assert_eq!(current.values["units"],Some(16.0));
    }
    #[test]
    fn rejects_missing_base_metrics_and_unknown_kinds() {
        let dir=tempdir().unwrap();
        let store=Storage::new(dir.path().join("data")).unwrap();
        let path=fixture(dir.path(),"one");
        fs::write(&path,fs::read_to_string(&path).unwrap().replace(
            r#""hours":2.0,"units":3.0"#,r#""hours":2.0"#)).unwrap();
        assert!(store.metric_preview(&path).is_err());
        fs::write(&path,fs::read_to_string(&path).unwrap().replace(
            r#""kind":"labor""#,r#""kind":"not_registered""#)).unwrap();
        assert!(store.metric_preview(&path).is_err());
        assert!(store.metric_summary("not_registered","2026-01-01","2026-12-31").is_err());
    }
}
