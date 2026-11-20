//! Original group-name audit over a bounded SQLite result page.
use super::*;
#[derive(Serialize)]
pub struct UnifiedGroupAuditRow {
    pub name: String,
    pub key: String,
    pub group: Option<String>,
    pub group_name: Option<String>,
    pub members: i64,
    pub active: Option<bool>,
}
#[derive(Serialize)]
pub struct UnifiedGroupAuditPage {
    pub rows: Vec<UnifiedGroupAuditRow>,
    pub total: i64,
    pub coverage: RangeSummary,
}
impl Storage {
    pub fn unified_group_audit_page(
        &self,
        revision: i64,
        scope: &AnalysisScope,
        query: &RangeQuery,
        mode: &str,
        search: &str,
        offset: i64,
        limit: usize,
    ) -> Result<UnifiedGroupAuditPage, String> {
        if offset<0 || offset>i64::from(i32::MAX) || !(1..=50).contains(&limit) {
            return Err("Invalid group audit query".into());
        }
        let mut rows=Vec::new();
        let (total,coverage)=self.visit_group_audit(revision,scope,query,mode,search,offset,limit as i64,|row|{rows.push(row);Ok(())})?;
        Ok(UnifiedGroupAuditPage{rows,total,coverage})
    }

    pub fn unified_group_audit_csv(&self,revision:i64,scope:&AnalysisScope,query:&RangeQuery,mode:&str,search:&str,destination:&Path)->Result<(),String>{
        if !destination.is_absolute() || destination.as_os_str().len()>32767 || !destination.extension().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("csv")) {return Err("Choose an absolute CSV file path".into());}
        export(destination,|w| {
            csv_record(w,&["Название","Группа","Названий работ в группе","Выполнения","Период с","Период до"].map(Cell::Text))?;
            self.visit_group_audit(revision,scope,query,mode,search,0,-1,|row|{
                let status=match row.active{Some(true)=>"Есть выполнения",Some(false)=>"Нет выполнений",None=>"Недостаточно данных о выполнениях"};
                csv_record(w,&[Cell::Text(&row.name),Cell::Text(row.group_name.as_deref().unwrap_or("")),Cell::Number((mode=="unused").then_some(row.members as f64)),Cell::Text(status),Cell::Text(&query.from),Cell::Text(&query.to)])
            })?;
            check_revision(&self.connect()?,revision)
        })
    }

    fn visit_group_audit(&self,revision:i64,scope:&AnalysisScope,query:&RangeQuery,mode:&str,search:&str,offset:i64,limit:i64,mut visit:impl FnMut(UnifiedGroupAuditRow)->Result<(),String>)->Result<(i64,RangeSummary),String>{
        if !matches!(mode,"all"|"ungrouped"|"unused")||search.len()>512 {return Err("Invalid group audit query".into());}
        // This audit always considers all works, independently of the chart's
        // name selection. Organization/range validation remains the same.
        let scope = AnalysisScope {
            report_type: scope.report_type.clone(),
            organizations: scope.organizations.clone(),
            names: None,
        };
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(err)?;
        workspace(&tx, revision, &scope)?;
        let indexed: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM management_settings)",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        if !indexed {
            return Err("Management settings are not indexed".into());
        }
        unified_analytics::presentation_functions(&tx)?;
        fill_range(&tx, &scope, query, 1)?;
        let coverage = range_summary(&tx, &scope, 1)?;
        let term = unified_analytics::normalized(search);
        let extras = term.chars().count() >= 3;
        if mode == "unused" {
            tx.execute_batch("CREATE TEMP TABLE audit_rows AS
                SELECT g.name name,'group:'||g.id key,g.handle group_handle,g.name group_name,g.members members,
                  EXISTS(SELECT 1 FROM an_combined a WHERE a.slot=1 AND a.key='group:'||g.id AND a.active<>0) observed
                FROM management_groups g;
                DELETE FROM audit_rows WHERE observed;").map_err(err)?;
            tx.execute("DELETE FROM audit_rows WHERE instr(classic_normalized(name),?1)=0 AND NOT (?2 AND EXISTS(SELECT 1 FROM management_members m WHERE m.group_handle=audit_rows.group_handle AND instr(classic_normalized(m.work),?1)>0))",params![term,extras]).map_err(err)?;
        } else {
            tx.execute_batch("CREATE TEMP TABLE audit_rows AS WITH works(work) AS (
                SELECT work FROM report_rows UNION SELECT work FROM management_catalog UNION SELECT work FROM management_members
            ) SELECT w.work name,'work:'||w.work key,
                (SELECT g.handle FROM management_members m JOIN management_groups g ON g.handle=m.group_handle WHERE m.work=w.work ORDER BY g.ordinal,g.handle LIMIT 1) group_handle,
                (SELECT g.name FROM management_members m JOIN management_groups g ON g.handle=m.group_handle WHERE m.work=w.work ORDER BY g.ordinal,g.handle LIMIT 1) group_name,
                0 members,
                EXISTS(SELECT 1 FROM an_valid v JOIN an_periods p ON p.slot=1 AND p.start=v.start JOIN report_rows r ON r.report_id=v.report_id AND r.organization=v.org WHERE r.work=w.work AND (r.count<>0 OR r.amount<>0)) observed
              FROM works w;").map_err(err)?;
            tx.execute("DELETE FROM audit_rows WHERE (?1='ungrouped' AND group_handle IS NOT NULL) OR (instr(classic_normalized(name),?2)=0 AND NOT (?3 AND instr(classic_normalized(coalesce(group_name,'Без группы')),?2)>0))",params![mode,term,extras]).map_err(err)?;
        }
        let total = count_sql(&tx, "SELECT count(*) FROM audit_rows")?;
        let mut stmt=tx.prepare("SELECT name,key,group_handle,group_name,members,observed FROM audit_rows ORDER BY (instr(classic_normalized(name),?1)<>1),name COLLATE classic_ru,key,group_handle LIMIT ?2 OFFSET ?3").map_err(err)?;
        let rows = stmt
            .query_map(params![term, limit, offset], |r| {
                let observed: bool = r.get(5)?;
                Ok(UnifiedGroupAuditRow {
                    name: r.get(0)?,
                    key: r.get(1)?,
                    group: r.get::<_, Option<i64>>(2)?.map(|handle| handle.to_string()),
                    group_name: r.get(3)?,
                    members: r.get(4)?,
                    active: if observed {
                        Some(true)
                    } else if coverage.complete {
                        Some(false)
                    } else {
                        None
                    },
                })
            })
            .map_err(err)?;
        for result in rows {visit(result.map_err(err)?)?;}
        Ok((total,coverage))
    }
}
