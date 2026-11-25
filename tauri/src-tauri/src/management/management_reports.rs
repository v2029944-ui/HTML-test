use super::super::management_job::JobRead;
use super::*;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
struct List<'a> {
    c: &'a Connection,
    organizations: bool,
    job: &'a ManagementJob,
}
impl<'de> DeserializeSeed<'de> for List<'_> {
    type Value = i64;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<i64, D::Error> {
        struct V<'a>(List<'a>);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = i64;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("report metadata array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<i64, A::Error> {
                use serde::de::Error;
                let mut n = 0i64;
                if self.0.organizations {
                    while let Some(s) = a.next_element::<String>()? {
                        self.0.job.check().map_err(A::Error::custom)?;
                        label(&s).map_err(A::Error::custom)?;
                        self.0
                            .c
                            .execute(
                                "INSERT INTO temp.management_org_stage VALUES(?1,?2)",
                                params![n, s],
                            )
                            .map_err(A::Error::custom)?;
                        n = n
                            .checked_add(1)
                            .ok_or_else(|| A::Error::custom("Too many organizations"))?;
                    }
                } else {
                    while a.next_element::<IgnoredAny>()?.is_some() {
                        self.0.job.check().map_err(A::Error::custom)?;
                        n = n
                            .checked_add(1)
                            .ok_or_else(|| A::Error::custom("Too many warnings"))?;
                    }
                }
                Ok(n)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<i64, E> {
                Ok(0)
            }
        }
        d.deserialize_any(V(self))
    }
}
struct Meta<'a> {
    c: &'a Connection,
    job: &'a ManagementJob,
}
impl<'de> DeserializeSeed<'de> for Meta<'_> {
    type Value = (Option<f64>, i64, bool);
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        struct V<'a>(Meta<'a>);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = (Option<f64>, i64, bool);
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("report object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                use serde::de::Error;
                let mut seen = [false; 3];
                let mut amount = None;
                let mut warnings = 0;
                let mut shared = false;
                while let Some(k) = a.next_key::<String>()? {
                    self.0.job.check().map_err(A::Error::custom)?;
                    if let Some(i) = ["amount", "organizations", "warnings"]
                        .iter()
                        .position(|x| *x == k)
                    {
                        if seen[i] {
                            return Err(A::Error::custom("Duplicate report metadata"));
                        }
                        seen[i] = true;
                        match i {
                            0 => {
                                amount = a.next_value::<Option<f64>>()?;
                                if amount.is_some_and(|n| !n.is_finite()) {
                                    return Err(A::Error::custom("Invalid saved amount"));
                                }
                            }
                            1 => {
                                a.next_value_seed(List {
                                    c: self.0.c,
                                    organizations: true,
                                    job: self.0.job,
                                })?;
                            }
                            _ => {
                                warnings = a.next_value_seed(List {
                                    c: self.0.c,
                                    organizations: false,
                                    job: self.0.job,
                                })?
                            }
                        }
                    } else {
                        if matches!(
                            k.as_str(),
                            "sourceDiagnosticsReport" | "sourceDiagnostics" | "sourceIssues"
                        ) {
                            shared = true;
                        }
                        a.next_value::<IgnoredAny>()?;
                    }
                }
                Ok((amount, warnings, shared))
            }
        }
        d.deserialize_map(V(self))
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManagementReportFilter {
    pub report_type: Option<String>,
    pub organization: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementReportCursor {
    pub start: String,
    pub id: String,
}
#[derive(Debug, Serialize)]
pub struct ManagementReport {
    pub shared_source_artifact: bool,
    pub id: String,
    pub report_type: String,
    pub start: String,
    pub end: String,
    pub source_name: Option<String>,
    pub rows: i64,
    pub amount: Option<f64>,
    pub saved_amount: Option<f64>,
    pub warnings: i64,
    pub organizations: i64,
}
impl ManagementReport {
    pub fn cursor(&self) -> ManagementReportCursor {
        ManagementReportCursor {
            start: self.start.clone(),
            id: self.id.clone(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct ManagementReportSummary {
    pub reports: i64,
    pub rows: i64,
    pub amount: Option<f64>,
    pub warnings: i64,
    pub first: Option<String>,
    pub latest: Option<String>,
    pub periods: i64,
}
#[derive(Debug, Serialize)]
pub struct ManagementPeriod {
    pub start: String,
    pub end: String,
    pub reports: i64,
}
fn valid_filter(f: &ManagementReportFilter) -> Result<(), String> {
    if f.report_type
        .as_deref()
        .is_some_and(|s| !REPORT_TYPES.contains(&s))
        || f.from.as_deref().is_some_and(|s| !valid_date(s))
        || f.to.as_deref().is_some_and(|s| !valid_date(s))
        || matches!((&f.from,&f.to),(Some(a),Some(b))if a>b)
    {
        return Err("Invalid management report filter".into());
    }
    if let Some(o) = &f.organization {
        label(o)?;
    }
    Ok(())
}
const REPORT_TABLES: &str = "FROM reports r JOIN management_report_meta m ON m.report_id=r.id JOIN report_row_index i ON i.report_id=r.id";
const FILTER:&str="(?1 IS NULL OR r.report_type=?1) AND (?2 IS NULL OR EXISTS(SELECT 1 FROM management_report_orgs o WHERE o.report_id=r.id AND o.org=?2)) AND (?3 IS NULL OR r.period_start>=?3) AND (?4 IS NULL OR r.period_start<=?4)";
fn ready(c: &Connection, rev: i64, f: &ManagementReportFilter) -> Result<(), String> {
    check_revision(c, rev)?;
    check_metadata_bounds(c)?;
    valid_filter(f)?;
    let missing:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM reports r LEFT JOIN management_report_meta m ON m.report_id=r.id LEFT JOIN report_row_index i ON i.report_id=r.id WHERE (?1 IS NULL OR r.report_type=?1) AND (m.report_id IS NULL OR i.report_id IS NULL))",[&f.report_type],|r|r.get(0)).map_err(err)?;
    if missing {
        Err("Report summaries contain unindexed history".into())
    } else {
        Ok(())
    }
}
impl Storage {
    /// Analysis accepts legacy rows without a string organization, but their
    /// organization-filtered projection cannot certify a complete raw index.
    pub fn index_unified_startup_report(
        &self,
        header: &ReportHeader,
        revision: i64,
        job: &ManagementJob,
    ) -> Result<(), String> {
        let raw_rows_compatible = self.analysis_index_report_for_startup(&header.id, revision)?;
        let raw_index_exists: bool = {
            let conn = self.management_connect()?;
            let tx = conn.unchecked_transaction().map_err(err)?;
            check_revision(&tx, revision)?;
            tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM report_row_index WHERE report_id=?1)",
                [&header.id],
                |row| row.get(0),
            ).map_err(err)?
        };
        if !raw_index_exists {
            if !raw_rows_compatible || !valid_date(&header.period_start) || !valid_date(&header.period_end) {
                return Ok(());
            }
            // A restored report with well-formed raw rows can have invalid
            // calendar bounds. Index its raw rows independently for management.
            self.index_report(&header.id, revision)?;
        }
        self.management_index_report(&header.id, revision, job)
    }

    pub fn management_index_report(
        &self,
        id: &str,
        rev: i64,
        job: &ManagementJob,
    ) -> Result<(), String> {
        label(id)?;
        job.check()?;
        let c = self.management_connect()?;
        c.execute_batch(
            "CREATE TEMP TABLE management_org_stage(ordinal INTEGER PRIMARY KEY,org TEXT);",
        )
        .map_err(err)?;
        job.run(&c,||{let tx=c.unchecked_transaction().map_err(err)?;check_revision(&tx,rev)?;check_metadata_bounds(&tx)?;let indexed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM report_row_index WHERE report_id=?1)",[id],|r|r.get(0)).map_err(err)?;if !indexed{return Err("Raw report index missing; run index_report before metadata indexing".into())}let rowid:i64=tx.query_row("SELECT rowid FROM reports WHERE id=?1",[id],|r|r.get(0)).map_err(err)?;let raw=tx.blob_open("main","reports","json",rowid,true).map_err(err)?;let bounded=read_model::TokenBounded::new(JobRead{inner:raw,job});let mut de=serde_json::Deserializer::from_reader(BufReader::with_capacity(8192,bounded));let(saved,warnings,shared)=Meta{c:&tx,job}.deserialize(&mut de).map_err(err)?;de.end().map_err(err)?;drop(de);let amount:Option<f64>=tx.query_row("SELECT CASE WHEN COUNT(*)=0 THEN 0 WHEN COUNT(amount)=COUNT(*) THEN SUM(amount) END FROM report_rows WHERE report_id=?1",[id],|r|r.get(0)).map_err(err)?;let amount=amount.map(num).transpose()?;tx.execute("DELETE FROM management_report_orgs WHERE report_id=?1",[id]).map_err(err)?;tx.execute("INSERT INTO management_report_orgs SELECT ?1,ordinal,org FROM temp.management_org_stage",[id]).map_err(err)?;tx.execute("INSERT INTO management_report_meta VALUES(?1,?2,?3,?4,?5) ON CONFLICT(report_id) DO UPDATE SET amount=excluded.amount,saved_amount=excluded.saved_amount,warnings=excluded.warnings,shared_source_artifact=excluded.shared_source_artifact",params![id,amount,saved,warnings,shared]).map_err(err)?;job.finish(tx)})
    }
    pub fn management_report_summary(
        &self,
        rev: i64,
        f: &ManagementReportFilter,
    ) -> Result<ManagementReportSummary, String> {
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        ready(&tx, rev, f)?;
        let sql=format!("SELECT COUNT(*),COALESCE(SUM(i.row_count),0),CASE WHEN COUNT(*)=0 THEN 0 WHEN COUNT(m.amount)=COUNT(*) THEN SUM(m.amount) END,COALESCE(SUM(m.warnings),0),MIN(r.period_start),MAX(r.period_start),COUNT(DISTINCT r.period_start) FROM reports r JOIN management_report_meta m ON m.report_id=r.id JOIN report_row_index i ON i.report_id=r.id WHERE {FILTER}");
        let out = tx
            .query_row(
                &sql,
                params![f.report_type, f.organization, f.from, f.to],
                |r| {
                    Ok(ManagementReportSummary {
                        reports: r.get(0)?,
                        rows: r.get(1)?,
                        amount: r.get(2)?,
                        warnings: r.get(3)?,
                        first: r.get(4)?,
                        latest: r.get(5)?,
                        periods: r.get(6)?,
                    })
                },
            )
            .map_err(err)?;
        out.amount.map(num).transpose()?;
        Ok(out)
    }
    pub fn management_report_page(
        &self,
        rev: i64,
        f: &ManagementReportFilter,
        after: Option<&ManagementReportCursor>,
        size: usize,
    ) -> Result<Vec<ManagementReport>, String> {
        batch(size)?;
        if let Some(a) = after {
            label(&a.id)?;
            if !valid_date(&a.start) {
                return Err("Invalid report cursor".into());
            }
        }
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        ready(&tx, rev, f)?;
        Self::report_rows(&tx, f, after, size, 0)
    }

    pub fn management_report_window(
        &self,
        rev: i64,
        f: &ManagementReportFilter,
        page_index: usize,
        size: usize,
    ) -> Result<SqlPage<ManagementReport>, String> {
        batch(size)?;
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        ready(&tx, rev, f)?;
        let count = format!("SELECT COUNT(*) {REPORT_TABLES} WHERE {FILTER}");
        let total_rows = tx.query_row(&count, params![f.report_type, f.organization, f.from, f.to], |r| r.get(0)).map_err(err)?;
        let offset = direct_page_offset(page_index, size, total_rows)?;
        let rows = Self::report_rows(&tx, f, None, size, offset)?;
        Ok(SqlPage { rows, total_rows })
    }

    fn report_rows(
        tx: &Transaction<'_>,
        f: &ManagementReportFilter,
        after: Option<&ManagementReportCursor>,
        size: usize,
        offset: i64,
    ) -> Result<Vec<ManagementReport>, String> {
        let sql=format!("SELECT r.id,r.report_type,r.period_start,r.period_end,r.source_name,i.row_count,m.amount,m.saved_amount,m.warnings,(SELECT COUNT(*) FROM management_report_orgs o WHERE o.report_id=r.id),m.shared_source_artifact {REPORT_TABLES} WHERE {FILTER} AND (?5 IS NULL OR r.period_start<?5 OR (r.period_start=?5 AND r.id>?6)) ORDER BY r.period_start DESC,r.id LIMIT ?7 OFFSET ?8");
        let mut stmt = tx.prepare(&sql).map_err(err)?;
        let out = stmt
            .query_map(
                params![
                    f.report_type,
                    f.organization,
                    f.from,
                    f.to,
                    after.map(|a| a.start.as_str()),
                    after.map(|a| a.id.as_str()),
                    size as i64,
                    offset
                ],
                |r| {
                    Ok(ManagementReport {
                        shared_source_artifact: r.get(10)?,
                        id: r.get(0)?,
                        report_type: r.get(1)?,
                        start: r.get(2)?,
                        end: r.get(3)?,
                        source_name: r.get(4)?,
                        rows: r.get(5)?,
                        amount: r.get(6)?,
                        saved_amount: r.get(7)?,
                        warnings: r.get(8)?,
                        organizations: r.get(9)?,
                    })
                },
            )
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(out)
    }
    pub fn management_period_page(
        &self,
        rev: i64,
        f: &ManagementReportFilter,
        after: Option<&str>,
        size: usize,
    ) -> Result<Vec<ManagementPeriod>, String> {
        batch(size)?;
        if f.report_type.is_none() {
            return Err("Period navigation requires one report type".into());
        }
        if after.is_some_and(|s| !valid_date(s)) {
            return Err("Invalid period cursor".into());
        }
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        ready(&tx, rev, f)?;
        let sql=format!("SELECT r.period_start,MAX(r.period_end),COUNT(*) FROM reports r WHERE {FILTER} AND (?5 IS NULL OR r.period_start<?5) GROUP BY r.period_start ORDER BY r.period_start DESC LIMIT ?6");
        let mut stmt = tx.prepare(&sql).map_err(err)?;
        let out = stmt
            .query_map(
                params![
                    f.report_type,
                    f.organization,
                    f.from,
                    f.to,
                    after,
                    size as i64
                ],
                |r| {
                    Ok(ManagementPeriod {
                        start: r.get(0)?,
                        end: r.get(1)?,
                        reports: r.get(2)?,
                    })
                },
            )
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(out)
    }
    pub fn management_report_org_page(
        &self,
        rev: i64,
        id: &str,
        after: Option<i64>,
        size: usize,
    ) -> Result<Vec<(i64, String)>, String> {
        batch(size)?;
        label(id)?;
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        check_revision(&tx, rev)?;
        Self::check_report_org_index(&tx, id)?;
        Self::report_org_rows(&tx, id, after, size, 0)
    }

    pub fn management_report_org_window(
        &self,
        rev: i64,
        id: &str,
        page_index: usize,
        size: usize,
    ) -> Result<SqlPage<(i64, String)>, String> {
        batch(size)?;
        label(id)?;
        let mut c = self.management_connect()?;
        let tx = c.transaction().map_err(err)?;
        check_revision(&tx, rev)?;
        Self::check_report_org_index(&tx, id)?;
        let total_rows = tx.query_row("SELECT COUNT(*) FROM management_report_orgs WHERE report_id=?1", [id], |r| r.get(0)).map_err(err)?;
        let offset = direct_page_offset(page_index, size, total_rows)?;
        let rows = Self::report_org_rows(&tx, id, None, size, offset)?;
        Ok(SqlPage { rows, total_rows })
    }

    fn check_report_org_index(tx: &Transaction<'_>, id: &str) -> Result<(), String> {
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM management_report_meta WHERE report_id=?1)",
                [id],
                |r| r.get(0),
            )
            .map_err(err)?;
        if !exists {
            return Err("Report organization index absent".into());
        }
        Ok(())
    }

    fn report_org_rows(tx: &Transaction<'_>, id: &str, after: Option<i64>, size: usize, offset: i64) -> Result<Vec<(i64, String)>, String> {
        let mut stmt=tx.prepare("SELECT ordinal,org FROM management_report_orgs WHERE report_id=?1 AND (?2 IS NULL OR ordinal>?2) ORDER BY ordinal LIMIT ?3 OFFSET ?4").map_err(err)?;
        let out = stmt
            .query_map(params![id, after, size as i64, offset], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(out)
    }
}

#[cfg(test)]
#[path = "unified_startup_tests.rs"]
mod unified_startup_tests;
