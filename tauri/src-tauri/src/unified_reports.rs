//! Bounded original-interface report quality and calendar read models.
use super::{err, read_model, QualityScope, Storage};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnifiedQualityFilter {
    pub from: Option<String>, pub to: Option<String>, pub organization: Option<String>,
    pub kind: Option<String>, pub search: Option<String>, pub source: Option<String>,
    pub report_type: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct UnifiedQualityPage {
    pub revision: i64, pub issues: Vec<Value>, pub next_offset: Option<u32>,
    pub total: u64, pub counts: BTreeMap<String,u64>,
}
#[derive(Debug, Serialize)]
pub struct UnifiedCalendar { pub revision: i64, pub year: i32, pub days: Vec<Value> }
fn checked_filter(f: &UnifiedQualityFilter) -> Result<(),String> {
    for text in [&f.from,&f.to,&f.organization,&f.kind,&f.search,&f.source,&f.report_type].into_iter().flatten() {
        if text.len()>1024 || text.contains('\0') { return Err("Invalid quality filter".into()); }
    }
    for date in [&f.from,&f.to].into_iter().flatten() {
        if chrono::NaiveDate::parse_from_str(date,"%Y-%m-%d").is_err() {return Err("Invalid quality date".into());}
    }
    if matches!((&f.from,&f.to),(Some(a),Some(b)) if a>b) {return Err("Invalid quality range".into());}
    if f.report_type.as_deref().is_some_and(|v| !["day","week","month"].contains(&v)) {return Err("Invalid quality report type".into());}
    Ok(())
}
fn matches_issue(v:&Value,f:&UnifiedQualityFilter)->bool {
    f.organization.as_deref().is_none_or(|o|v["organization"].as_str()==Some(o)) &&
    f.kind.as_deref().is_none_or(|k|v["kind"].as_str()==Some(k)) &&
    f.search.as_deref().is_none_or(|s|v.to_string().to_lowercase().contains(&s.to_lowercase()))
}
impl Storage {
    /// Presence is deliberately independent of completeness; analytics daily
    /// rollups are the only source of confirmed daily/monthly/yearly totals.
    pub fn unified_calendar(&self, revision:i64, year:i32)->Result<UnifiedCalendar,String> {
        if !(1..=9998).contains(&year) {return Err("Invalid calendar year".into());}
        let c=self.connect()?; let tx=c.unchecked_transaction().map_err(err)?;
        read_model::check_revision(&tx,revision)?;read_model::check_metadata_bounds(&tx)?;
        let invalid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM reports WHERE octet_length(period_start)<>10 OR report_type NOT IN ('day','week','month') OR period_start NOT GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' OR substr(period_start,1,4)='0000' OR date(period_start,'+0 days') IS NOT period_start)",[],|r|r.get(0)).map_err(err)?;
        if invalid{return Err("Invalid calendar report headers".into());}
        let mut s=tx.prepare("SELECT r.period_start, SUM(r.report_type='day'),SUM(r.report_type='week'),SUM(r.report_type='month'),COALESCE(SUM(m.warnings),0),(SELECT COUNT(DISTINCT o.org) FROM management_report_orgs o JOIN reports r2 ON r2.id=o.report_id WHERE r2.period_start=r.period_start) FROM reports r LEFT JOIN management_report_meta m ON m.report_id=r.id WHERE r.period_start BETWEEN ?1 AND ?2 GROUP BY r.period_start ORDER BY r.period_start LIMIT 366").map_err(err)?;
        let days=s.query_map(params![format!("{year:04}-01-01"),format!("{year:04}-12-31")],|r|Ok(json!({"date":r.get::<_,String>(0)?,"day_reports":r.get::<_,i64>(1)?,"week_reports":r.get::<_,i64>(2)?,"month_reports":r.get::<_,i64>(3)?,"warnings":r.get::<_,i64>(4)?,"organizations":r.get::<_,i64>(5)?,"complete":null}))).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
        Ok(UnifiedCalendar{revision,year,days})
    }
    /// Filtering and pagination are applied after streaming every selected saved
    /// diagnostic, never to the currently visible report page. Only 50 issues
    /// and a fixed number of counters are retained in memory.
    pub fn unified_quality_page(&self, revision:i64, filter:&UnifiedQualityFilter, offset:u32, limit:usize)->Result<UnifiedQualityPage,String> {
        checked_filter(filter)?;
        if !(1..=50).contains(&limit) {return Err("Quality page must contain 1..=50 rows".into());}
        let c=self.connect()?; read_model::check_revision(&c,revision)?;read_model::check_metadata_bounds(&c)?;
        let mut result=UnifiedQualityPage{revision,issues:Vec::new(),next_offset:None,total:0,counts:BTreeMap::new()};
        let mut accept=|mut issue:Value,id:&str,start:&str,end:&str,source:&Option<String>,anchor:Value| {
            issue["report_id"]=json!(id);issue["period_start"]=json!(start);issue["period_end"]=json!(end);issue["source_name"]=json!(source);issue["source_anchor"]=anchor;
            if !matches_issue(&issue,filter) {return;}
            let original_kind=issue["kind"].as_str().unwrap_or("warning");
            let counter_kind=if ["invalid","skipped","mismatch","warning","missing","duplicate","overlap","empty-name","import"].contains(&original_kind){original_kind}else{"other"};
            *result.counts.entry(counter_kind.to_owned()).or_default()+=1;
            if result.total>=u64::from(offset) && result.issues.len()<limit {result.issues.push(issue);}
            result.total+=1;
        };
        let mut cursor:Option<(String,String)>=None;
        loop {
            let headers={let mut s=c.prepare("SELECT id,period_start,period_end,source_name FROM reports r WHERE (?1 IS NULL OR period_start>=?1) AND (?2 IS NULL OR period_start<=?2) AND (?3 IS NULL OR report_type=?3) AND (?4 IS NULL OR instr(lower(COALESCE(source_name,'')),lower(?4))>0) AND (?5 IS NULL OR period_start<?5 OR (period_start=?5 AND id>?6)) ORDER BY period_start DESC,id LIMIT 50").map_err(err)?;
                let rows=s.query_map(params![filter.from,filter.to,filter.report_type,filter.source,cursor.as_ref().map(|x|&x.0),cursor.as_ref().map(|x|&x.1)],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?))).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?; rows};
            if headers.is_empty(){break;}
            for (id,start,end,source) in &headers {
                let mut page_offset=0;
                loop {
                    let page=serde_json::to_value(self.report_quality_page(revision,id,QualityScope::Report,page_offset,50)?).map_err(err)?;
                    for issue in page["issues"].as_array().ok_or("Invalid diagnostic page")? {accept(issue.clone(),id,start,end,source,page["source_anchor"].clone());}
                    if page_offset==0 && page["invalid_entries"].as_u64().unwrap_or(0)>0 {accept(json!({"kind":"invalid","reason":format!("Сохранены замечания неподдерживаемого формата: {}. Полнота не подтверждена.",page["invalid_entries"])}),id,start,end,source,page["source_anchor"].clone());}
                    let source_anchor=page["source_anchor"].as_str();
                    let source_first=if page_offset==0 {if let Some(anchor)=source_anchor {
                        !c.query_row("SELECT EXISTS(SELECT 1 FROM reports r WHERE (r.id=?1 OR EXISTS(SELECT 1 FROM report_source_diagnostic_links l WHERE l.report_id=r.id AND l.anchor_id=?1)) AND (?2 IS NULL OR r.period_start>=?2) AND (?3 IS NULL OR r.period_start<=?3) AND (?4 IS NULL OR r.report_type=?4) AND (?5 IS NULL OR instr(lower(COALESCE(r.source_name,'')),lower(?5))>0) AND (r.period_start>?6 OR (r.period_start=?6 AND r.id<?7)))",params![anchor,filter.from,filter.to,filter.report_type,filter.source,start,id],|r|r.get::<_,bool>(0)).map_err(err)?
                    }else{false}}else{false};
                    if source_first {
                        let mut source_offset=0;
                        loop {let source_page=serde_json::to_value(self.report_quality_page(revision,id,QualityScope::Source,source_offset,50)?).map_err(err)?;
                            for issue in source_page["issues"].as_array().ok_or("Invalid source diagnostic page")? {let mut issue=issue.clone();issue["diagnostic_scope"]=json!("source");accept(issue,id,start,end,source,json!(source_anchor));}
                            match source_page["next_offset"].as_u64(){Some(n)=>source_offset=u32::try_from(n).map_err(err)?,None=>break}
                        }
                    }
                    match page["next_offset"].as_u64() {Some(n)=>page_offset=u32::try_from(n).map_err(err)?,None=>break}
                }
                let invalid_reason:Option<String>=c.query_row("SELECT MAX(CASE WHEN valid=0 THEN COALESCE(issue,'Некорректный сохранённый отчёт') END) FROM analysis_reports WHERE report_id=?1",[id],|r|r.get(0)).map_err(err)?;
                if let Some(reason)=invalid_reason {accept(json!({"kind":"invalid","reason":reason}),id,start,end,source,Value::Null);}
                // Select either relevant member before canonical pair deduplication:
                // a lexically larger selected report still exposes its conflict
                // when its partner falls outside the period/source filters.
                // Partner metadata also makes global text search symmetric.
                // Conflicts come from the same SQL organization index as analytics.
                let mut conflicts=c.prepare("SELECT b.id,o.org,CASE WHEN a.period_start=b.period_start AND a.period_end=b.period_end THEN 'duplicate' ELSE 'overlap' END,b.source_name,b.period_start,b.period_end FROM reports a JOIN analysis_reports va ON va.report_id=a.id AND va.valid=1 JOIN analysis_orgs o ON o.report_id=a.id JOIN analysis_orgs p ON p.org=o.org JOIN reports b ON b.id=p.report_id AND b.report_type=a.report_type JOIN analysis_reports vb ON vb.report_id=b.id AND vb.valid=1 WHERE a.id=?1 AND a.id<>b.id AND a.period_start<=b.period_end AND a.period_end>=b.period_start AND (a.id<b.id OR NOT ((?2 IS NULL OR b.period_start>=?2) AND (?3 IS NULL OR b.period_start<=?3) AND (?4 IS NULL OR b.report_type=?4) AND (?5 IS NULL OR instr(lower(COALESCE(b.source_name,'')),lower(?5))>0))) ORDER BY b.id,o.org").map_err(err)?;
                let mut rows=conflicts.query(params![id,filter.from,filter.to,filter.report_type,filter.source]).map_err(err)?;
                while let Some(row)=rows.next().map_err(err)? {let kind:String=row.get(2).map_err(err)?;accept(json!({"kind":kind,"organization":row.get::<_,String>(1).map_err(err)?,"other_report_id":row.get::<_,String>(0).map_err(err)?,"other_source_name":row.get::<_,Option<String>>(3).map_err(err)?,"other_period_start":row.get::<_,String>(4).map_err(err)?,"other_period_end":row.get::<_,String>(5).map_err(err)?,"reason":if kind=="duplicate"{"Повторный период"}else{"Пересечение периодов"}}),id,start,end,source,Value::Null);}
            }
            cursor=headers.last().map(|h|(h.1.clone(),h.0.clone()));
        }
        // Infer gaps only inside one identifiable source/organization/type span.
        // Weekly and monthly totals are never expanded into daily observations.
        let mut gaps=c.prepare("WITH RECURSIVE spans(org,kind,source,first,last) AS (SELECT o.org,r.report_type,r.source_name,MIN(r.period_start),MAX(r.period_start) FROM reports r JOIN analysis_orgs o ON o.report_id=r.id JOIN analysis_reports a ON a.report_id=r.id AND a.valid=1 WHERE r.source_name IS NOT NULL AND r.source_name<>'' AND (?1 IS NULL OR r.report_type=?1) AND (?2 IS NULL OR instr(lower(r.source_name),lower(?2))>0) GROUP BY o.org,r.report_type,r.source_name), expected(org,kind,source,d,last) AS (SELECT org,kind,source,first,last FROM spans UNION ALL SELECT org,kind,source,date(d,CASE kind WHEN 'day' THEN '+1 day' WHEN 'week' THEN '+7 days' ELSE '+1 month' END),last FROM expected WHERE d<last) SELECT org,kind,source,d FROM expected e WHERE (?3 IS NULL OR d>=?3) AND (?4 IS NULL OR d<=?4) AND NOT EXISTS(SELECT 1 FROM reports r JOIN analysis_orgs o ON o.report_id=r.id WHERE o.org=e.org AND r.report_type=e.kind AND r.period_start=e.d AND r.source_name=e.source) ORDER BY d DESC,org,kind,source").map_err(err)?;
        let mut rows=gaps.query(params![filter.report_type,filter.source,filter.from,filter.to]).map_err(err)?;
        while let Some(row)=rows.next().map_err(err)? {let date:String=row.get(3).map_err(err)?;let source:Option<String>=row.get(2).map_err(err)?;accept(json!({"kind":"missing","organization":row.get::<_,String>(0).map_err(err)?,"report_type":row.get::<_,String>(1).map_err(err)?,"date":date,"reason":"Пропущен период внутри сохранённого источника. Это не нулевой показатель."}),"",&date,&date,&source,Value::Null);}
        drop(accept);
        // The revision recheck also rejects writes between independently opened
        // streaming report snapshots rather than presenting mixed results.
        read_model::check_revision(&c,revision)?;
        if result.total>u64::from(offset)+result.issues.len() as u64 {result.next_offset=Some(offset.checked_add(limit as u32).ok_or("Quality offset overflow")?);}
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
 use super::*;
 #[test] fn quality_filters_are_global_and_bounded(){let d=tempfile::tempdir().unwrap();let s=Storage::new(d.path().into()).unwrap();let c=s.connect().unwrap();for n in 0..65 {let id=format!("r{n:03}");let doc=json!({"issues":[{"type":"invalid","reason":format!("needle {n}"),"organization":"A"}]});c.execute("INSERT INTO reports VALUES(?1,'day','2024-01-01','2024-01-01','file.xlsx',?2)",params![id,serde_json::to_vec(&doc).unwrap()]).unwrap();}let rev=s.revision().unwrap();let f=UnifiedQualityFilter{search:Some("needle 64".into()),..Default::default()};let p=s.unified_quality_page(rev,&f,0,50).unwrap();assert_eq!(p.total,1);assert_eq!(p.issues[0]["report_id"],"r064");assert!(s.unified_quality_page(rev-1,&f,0,50).is_err());assert!(s.unified_quality_page(rev,&f,0,51).is_err());}
 #[test] fn calendar_presence_does_not_claim_completeness(){let d=tempfile::tempdir().unwrap();let s=Storage::new(d.path().into()).unwrap();let c=s.connect().unwrap();c.execute("INSERT INTO reports VALUES('a','day','2024-02-29','2024-02-29',NULL,x'FF')",[]).unwrap();let v=s.unified_calendar(s.revision().unwrap(),2024).unwrap();assert_eq!(v.days.len(),1);assert!(v.days[0]["complete"].is_null());}
 #[test] fn quality_reports_missing_duplicate_and_offset_across_history(){
  let d=tempfile::tempdir().unwrap();let s=Storage::new(d.path().into()).unwrap();let c=s.connect().unwrap();
  for (id,date) in [("a","2024-01-01"),("b","2024-01-03"),("c","2024-01-03")] {
   c.execute("INSERT INTO reports VALUES(?1,'day',?2,?2,'daily.xlsx',?3)",params![id,date,b"{}".as_slice()]).unwrap();
   c.execute("INSERT INTO analysis_reports VALUES(?1,1,NULL,0)",[id]).unwrap();c.execute("INSERT INTO analysis_orgs VALUES(?1,'A')",[id]).unwrap();
  }
  let rev=s.revision().unwrap();let all=s.unified_quality_page(rev,&UnifiedQualityFilter::default(),0,1).unwrap();assert_eq!(all.total,2);assert_eq!(all.next_offset,Some(1));assert_eq!(all.counts["missing"],1);assert_eq!(all.counts["duplicate"],1);
  let missing=s.unified_quality_page(rev,&UnifiedQualityFilter{kind:Some("missing".into()),organization:Some("A".into()),..Default::default()},0,50).unwrap();assert_eq!(missing.issues[0]["date"],"2024-01-02");
  assert!(s.unified_quality_page(rev,&UnifiedQualityFilter{organization:Some("B".into()),..Default::default()},0,50).unwrap().issues.is_empty());
 }


 #[test] fn conflict_pairs_survive_either_member_date_and_source_filters(){
  let d=tempfile::tempdir().unwrap();let s=Storage::new(d.path().into()).unwrap();let c=s.connect().unwrap();
  for (id,kind,start,end,source) in [("a","week","2024-11-01","2024-11-07","alpha.xlsx"),("b","week","2024-11-02","2024-11-08","beta.xlsx"),("c","month","2024-11-01","2024-11-30","gamma.xlsx"),("d","month","2024-11-01","2024-11-30","delta.xlsx")] {
   c.execute("INSERT INTO reports VALUES(?1,?2,?3,?4,?5,?6)",params![id,kind,start,end,source,b"{}".as_slice()]).unwrap();c.execute("INSERT INTO analysis_reports VALUES(?1,1,NULL,0)",[id]).unwrap();c.execute("INSERT INTO analysis_orgs VALUES(?1,'A')",[id]).unwrap();
  }
  let rev=s.revision().unwrap();
  for (kind,report_type,date,source,id) in [("overlap","week","2024-11-01","alpha.xlsx","a"),("overlap","week","2024-11-02","beta.xlsx","b"),("duplicate","month","2024-11-01","gamma.xlsx","c"),("duplicate","month","2024-11-01","delta.xlsx","d")] {
   for filtered_source in [None,Some(source.to_string())] {let f=UnifiedQualityFilter{from:Some(date.into()),to:Some(date.into()),source:filtered_source,report_type:Some(report_type.into()),kind:Some(kind.into()),..Default::default()};let p=s.unified_quality_page(rev,&f,0,50).unwrap();assert_eq!(p.total,1,"{id} {source}");if f.source.is_some()||report_type=="week" {assert_eq!(p.issues[0]["report_id"],id);}}
  }
  let f=UnifiedQualityFilter{kind:Some("overlap".into()),search:Some("beta.xlsx".into()),..Default::default()};let p=s.unified_quality_page(rev,&f,0,50).unwrap();assert_eq!(p.total,1);assert_eq!(p.issues[0]["other_source_name"],"beta.xlsx");
  let all=s.unified_quality_page(rev,&UnifiedQualityFilter::default(),0,50).unwrap();assert_eq!(all.total,2);assert_eq!(all.counts["overlap"],1);assert_eq!(all.counts["duplicate"],1);
 }
 #[test] fn arbitrary_saved_kinds_do_not_expand_summary_counter_memory(){
  let d=tempfile::tempdir().unwrap();let s=Storage::new(d.path().into()).unwrap();let c=s.connect().unwrap();let issues=(0..80).map(|i|json!({"type":format!("custom-{i}"),"reason":"Saved detail"})).collect::<Vec<_>>();let doc=serde_json::to_vec(&json!({"issues":issues})).unwrap();c.execute("INSERT INTO reports VALUES('a','day','2024-11-01','2024-11-01','source.xlsx',?1)",[doc]).unwrap();let p=s.unified_quality_page(s.revision().unwrap(),&UnifiedQualityFilter::default(),0,1).unwrap();assert_eq!(p.total,80);assert_eq!(p.issues.len(),1);assert_eq!(p.issues[0]["kind"],"custom-0");assert_eq!(p.counts.len(),1);assert_eq!(p.counts["other"],80);assert_eq!(p.next_offset,Some(1));
 }

}

impl super::Rule {
    /// Persisted browser coordinates are zero based. Native XLSX conversion
    /// accepts one based coordinates; names remain workbook display names.
    pub fn native_wide_options(&self, organizations:[String;4])->Result<analiz_import_core::WideOptions,String> {
        super::validate_rule(self)?;
        let mut mappings=BTreeMap::new();
        for (name,m) in self.mappings.as_ref().into_iter().flatten() {
            if mappings.len()>=12 {return Err("Не более 12 ручных листов за импорт.".into());}
            let inc=|v:u32|v.checked_add(1).ok_or_else(||"Excel coordinate overflow".to_string());
            mappings.insert(name.clone(),analiz_import_core::WideMapping{work_col:inc(m.work_col)?,header_row:inc(m.header_row)?,first_day_col:inc(m.first_day_col)?,total_count_col:m.total_count_col.map(inc).transpose()?,total_amount_col:m.total_amount_col.map(inc).transpose()?});
        }
        Ok(analiz_import_core::WideOptions{year:self.year.map(i32::from),sheets:self.sheets.clone().unwrap_or_default(),mappings,organizations,sequential_periods:BTreeMap::new()})
    }
    pub fn native_legacy_sheet(&self,book:&[(String,String)])->Result<String,String> {
        if !self.sheet.is_empty(){return book.iter().find(|(name,_)|name==&self.sheet).map(|(_,component)|component.clone()).ok_or_else(||"Выбранный лист Excel не найден.".into());}
        match book { [(_,component)]=>Ok(component.clone()),_=>Err("Несколько листов Excel требуют ручного выбора.".into()) }
    }
}
#[cfg(test)]
mod folder_rule_tests {
 use super::*;
 #[test] fn advanced_rule_coordinates_translate_once_and_preserve_names(){
  let rule:super::super::Rule=serde_json::from_value(json!({"format":"daily-wide","type":"day","start":"","sheet":"","year":2024,"sheets":["Дневные"],"mappings":{"Дневные":{"workCol":0,"headerRow":1,"firstDayCol":1,"totalCountCol":5,"totalAmountCol":6}}})).unwrap();
  let options=rule.native_wide_options(["A".into(),"B".into(),"C".into(),"D".into()]).unwrap();let m=&options.mappings["Дневные"];
  assert_eq!(options.sheets,vec!["Дневные"]);assert_eq!(m.work_col,1);assert_eq!(m.header_row,2);assert_eq!(m.first_day_col,2);assert_eq!(m.total_count_col,Some(6));assert_eq!(m.total_amount_col,Some(7));assert_eq!(options.year,Some(2024));
 }
 #[test] fn folder_legacy_resolves_actual_relationship_and_rejects_ambiguity(){
  let mut rule=super::super::Rule{kind:"day".into(),start:"2024-01-01".into(),..Default::default()};let book=vec![("Другой".into(),"xl/worksheets/custom.xml".into()),("Отчёт".into(),"xl/worksheets/sheet7.xml".into())];
  assert!(rule.native_legacy_sheet(&book).is_err());rule.sheet="Отчёт".into();assert_eq!(rule.native_legacy_sheet(&book).unwrap(),"xl/worksheets/sheet7.xml");rule.sheet="Нет".into();assert!(rule.native_legacy_sheet(&book).is_err());
 }
 #[test] fn malformed_advanced_coordinates_are_rejected(){let mut rule:super::super::Rule=serde_json::from_value(json!({"format":"daily-wide","type":"day","start":"","sheet":"","mappings":{"Data":{"workCol":2048,"headerRow":0,"firstDayCol":1}}})).unwrap();assert!(rule.native_wide_options(["A".into(),"B".into(),"C".into(),"D".into()]).is_err());rule.mappings=None;let options=rule.native_wide_options(["A".into(),"B".into(),"C".into(),"D".into()]).unwrap();assert!(options.mappings.is_empty());}
 #[test] fn translated_saved_mapping_converts_real_xlsx_with_display_sheet_name(){
  use std::{io::Write,sync::{Arc,atomic::AtomicBool}};
  let d=tempfile::tempdir().unwrap();let source=d.path().join("advanced.xlsx");let mut zip=zip::ZipWriter::new(std::fs::File::create(&source).unwrap());
  let worksheet=r#"<worksheet><sheetData><row r="1"><c r="B1" t="inlineStr"><is><t>2024-01-01</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Работа</t></is></c><c r="B2" t="inlineStr"><is><t>Количество</t></is></c><c r="C2" t="inlineStr"><is><t>Сумма</t></is></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>A</t></is></c></row><row r="4"><c r="A4" t="inlineStr"><is><t>Service</t></is></c><c r="B4"><v>2</v></c><c r="C4"><v>20</v></c></row></sheetData></worksheet>"#;
  for (name,xml) in [("xl/worksheets/custom.xml",worksheet),("xl/workbook.xml",r#"<workbook xmlns:r="r"><sheets><sheet name="Отчёт" r:id="r7"/></sheets></workbook>"#),("xl/_rels/workbook.xml.rels",r#"<Relationships><Relationship Id="r7" Target="/xl/worksheets/custom.xml" Type="worksheet"/></Relationships>"#)] {zip.start_file(name,zip::write::SimpleFileOptions::default()).unwrap();zip.write_all(xml.as_bytes()).unwrap();}zip.finish().unwrap();
  let rule:super::super::Rule=serde_json::from_value(json!({"format":"daily-wide","type":"day","start":"","sheet":"","year":2024,"sheets":["Отчёт"],"mappings":{"Отчёт":{"workCol":0,"headerRow":1,"firstDayCol":1}}})).unwrap();
  let options=rule.native_wide_options(["A".into(),"B".into(),"C".into(),"D".into()]).unwrap();let area=analiz_import_core::StageArea::open(d.path().join("stage")).unwrap();let cancel=Arc::new(AtomicBool::new(false));let mut stage=area.ingest(&source,analiz_import_core::Limits::default(),&cancel,|_|{}).unwrap();
  assert_eq!(rule.native_legacy_sheet(&stage.sheet_names().unwrap()).unwrap(),"xl/worksheets/custom.xml");
  let detected=stage.detect_wide(&options,cancel.clone()).unwrap();assert_eq!(detected[0].name,"Отчёт");stage.convert_wide(&options,cancel,|_|{}).unwrap();let reports=stage.report_page(0,50).unwrap();assert_eq!(reports.len(),1);assert_eq!(reports[0].start,"2024-01-01");assert_eq!(reports[0].count,2.);assert_eq!(reports[0].amount,20.);
 }

}
