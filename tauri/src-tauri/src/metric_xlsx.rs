//! XLSX -> normalized metric rows using the existing disk-backed, bounded XLSX parser.
//! No arbitrary formulas, scripts or user-supplied SQL; profiles are reviewed at build time.
use super::*;
use super::metric_reports::{
    self, MetricDocument, MetricKind, MetricReceipt, MetricReview, MetricRow,
};
use analiz_import_core::{Limits, Stage, StageArea};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricXlsxProfile {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub sheet: String,
    pub header_row: u32,
    pub first_data_row: u32,
    #[serde(default)]
    pub last_data_row: Option<u32>,
    #[serde(default)]
    pub organization_column: Option<String>,
    pub dimension_column: String,
    pub values: BTreeMap<String,String>,
    pub headers: BTreeMap<String,String>,
}

fn column(s: &str) -> Result<u32,String> {
    if s.is_empty() || s.len()>3 || !s.bytes().all(|x|x.is_ascii_uppercase()) {
        return Err(format!("Invalid Excel column: {s}"));
    }
    let mut n=0u32;
    for byte in s.bytes(){
        n=n.checked_mul(26).and_then(|x|x.checked_add((byte-b'A'+1) as u32))
          .ok_or("Excel column overflow")?;
    }
    if n>2048 {return Err("Excel column exceeds XLSX stage limit".into());}
    Ok(n)
}
fn position(address: &str) -> Result<(u32,u32),String> {
    let split=address.bytes().position(|v|v.is_ascii_digit())
        .ok_or("Excel cell lacks row number")?;
    let c=column(&address[..split])?;
    let r=address[split..].parse::<u32>().map_err(err)?;
    if r==0 || r>60000{return Err("Excel row out of range".into());}
    Ok((r,c))
}
fn validate(profile: &MetricXlsxProfile, kind: &MetricKind) -> Result<(),String> {
    let id=&profile.id;
    if id.is_empty() || id.len()>64 || !id.bytes().all(|c|
        c.is_ascii_lowercase() || c.is_ascii_digit() || c==b'_')
        || !metric_reports::label(&profile.name)
        || !metric_reports::label(&profile.sheet)
        || profile.header_row==0 || profile.first_data_row<=profile.header_row
        || profile.first_data_row>60000
        || profile.last_data_row.is_some_and(|n|n<profile.first_data_row||n>60000) {
        return Err("Invalid XLSX mapping metadata".into());
    }
    let required:HashSet<_>=kind.metrics.iter().filter(|m|m.operation=="sum")
        .map(|m|m.id.as_str()).collect();
    if profile.values.len()!=required.len() ||
        profile.values.keys().any(|name|!required.contains(name.as_str())) {
        return Err("XLSX mapping must cover every additive metric exactly once".into());
    }
    let mut used=HashSet::new();
    let dimension=column(&profile.dimension_column)?;
    used.insert(dimension);
    if let Some(org)=&profile.organization_column {
        if !used.insert(column(org)?) {return Err("Duplicate XLSX column".into());}
    }
    for col in profile.values.values() {
        if !used.insert(column(col)?) {return Err("Duplicate XLSX column".into());}
    }
    let headers:HashSet<_>=profile.headers.keys().map(|s|column(s)).collect::<Result<_,_>>()?;
    if headers!=used || profile.headers.values().any(|text|!metric_reports::label(text)) {
        return Err("XLSX mapping must verify the header of every used column".into());
    }
    Ok(())
}
pub fn profiles() -> Result<Vec<MetricXlsxProfile>,String> {
    let list:Vec<MetricXlsxProfile>=serde_json::from_str(include_str!("metric-xlsx-profiles.json")).map_err(err)?;
    if list.is_empty() || list.len()>64 {return Err("Invalid XLSX profile catalog size".into());}
    let mut ids=HashSet::new();
    for p in &list {
        if !ids.insert(p.id.clone()){return Err("Duplicate XLSX profile ID".into());}
        validate(p,&metric_reports::kind_by_id(&p.kind)?)?;
    }
    Ok(list)
}
fn profile_by_id(id: &str) -> Result<MetricXlsxProfile,String> {
    profiles()?.into_iter().find(|p|p.id==id).ok_or_else(||"Unknown XLSX profile".into())
}
#[derive(Default)]
struct RawCell { value:String, kind:String, formula:bool }
fn get<'a>(data:&'a BTreeMap<u32,RawCell>,col:u32) -> &'a str {
    data.get(&col).map(|x|x.value.as_str()).unwrap_or("")
}
fn checked_cell(data:&BTreeMap<u32,RawCell>,col:u32,row:u32) -> Result<&str,String> {
    if data.get(&col).is_some_and(|c|c.formula || c.kind=="e" || c.kind=="b") {
        return Err(format!("Formula/error/boolean not permitted at Excel row {row}, column {col}"));
    }
    Ok(get(data,col).trim())
}
fn number(input:&str,row:u32,col:u32) -> Result<f64,String> {
    let normalized:String=input.chars()
        .filter(|&c| c!=' '&&c!='\u{00a0}'&&c!='\u{202f}')
        .collect::<String>().replace(',','.');
    let value=normalized.parse::<f64>().map_err(|_|
        format!("Invalid numeric metric at Excel row {row}, column {col}"))?;
    if !value.is_finite() || value.abs()>1e12 {
        return Err(format!("Metric out of bounds at Excel row {row}, column {col}"));
    }
    Ok(value)
}
fn finish_row(
    row:u32,
    data:&BTreeMap<u32,RawCell>,
    profile:&MetricXlsxProfile,
    out:&mut Vec<MetricRow>
) -> Result<(),String> {
    if row==profile.header_row {
        for (col,expected) in &profile.headers {
            let ix=column(col)?;
            if checked_cell(data,ix,row)?.to_lowercase()!=expected.trim().to_lowercase() {
                return Err(format!("XLSX header mismatch at {col}{row}: expected {expected}"));
            }
        }
        return Ok(());
    }
    if row<profile.first_data_row || profile.last_data_row.is_some_and(|last|row>last) {
        return Ok(());
    }
    let dim_col=column(&profile.dimension_column)?;
    let dim=checked_cell(data,dim_col,row)?;
    let organization=profile.organization_column.as_ref().map(|col|
        checked_cell(data,column(col)?,row)
    ).transpose()?;
    let mut any=!dim.is_empty() || organization.is_some_and(|v|!v.is_empty());
    for col in profile.values.values() {
        if !checked_cell(data,column(col)?,row)?.is_empty(){any=true;}
    }
    if !any {return Ok(());}
    if dim.is_empty() {return Err(format!("Missing dimension at Excel row {row}"));}
    if out.len()>=metric_reports::MAX_ROWS {
        return Err("XLSX metric row count exceeds 10 000".into());
    }
    let mut values=BTreeMap::new();
    for (id,col) in &profile.values {
        let ix=column(col)?;
        values.insert(id.clone(),number(checked_cell(data,ix,row)?,row,ix)?);
    }
    out.push(MetricRow{
        organization:organization.and_then(|v|(!v.is_empty()).then(||v.to_owned())),
        dimension:dim.to_owned(),values,
    });
    Ok(())
}
fn decode_stage(
    stage:&Stage<'_>,sheet:&str,profile:&MetricXlsxProfile
) -> Result<Vec<MetricRow>,String> {
    let mut all=Vec::new();
    let mut after:i64=0;
    let mut current_row=0u32;
    let mut cells=BTreeMap::new();
    let mut saw_header=false;
    loop {
        let page=stage.page(sheet,after,100).map_err(err)?;
        if page.is_empty(){break;}
        let count=page.len();
        for cell in page {
            let (row,col)=position(&cell.address)?;
            if profile.last_data_row.is_some_and(|last|row>last) {
                if current_row!=0 {
                    if current_row==profile.header_row{saw_header=true;}
                    finish_row(current_row,&cells,profile,&mut all)?;
                }
                if !saw_header {return Err("XLSX header row is missing".into());}
                return Ok(all);
            }
            if current_row!=0 && current_row!=row {
                if current_row==profile.header_row{saw_header=true;}
                finish_row(current_row,&cells,profile,&mut all)?;
                cells.clear();
            }
            current_row=row;
            // Only mapped columns and headers are kept; never a full worksheet.
            if col==column(&profile.dimension_column)?
                || profile.organization_column.as_deref().is_some_and(|s|column(s).ok()==Some(col))
                || profile.values.values().any(|s|column(s).ok()==Some(col))
                || (row==profile.header_row
                    && profile.headers.keys().any(|s|column(s).ok()==Some(col))) {
                cells.insert(col,RawCell{value:cell.value,kind:cell.kind,formula:cell.formula});
            }
            after=((row as i64-1)*2048)+(col as i64);
        }
        if count<100 {break;}
    }
    if current_row!=0 {
        if current_row==profile.header_row{saw_header=true;}
        finish_row(current_row,&cells,profile,&mut all)?;
    }
    if !saw_header {return Err("XLSX header row is missing".into());}
    if all.is_empty(){return Err("XLSX has no mapped metric data rows".into());}
    Ok(all)
}
fn from_excel(
    store:&Storage,source:&str,profile_id:&str,start:&str,end:&str
) -> Result<(MetricDocument,Vec<u8>),String> {
    let profile=profile_by_id(profile_id)?;
    if !valid_date(start)||!valid_date(end)||start>end {
        return Err("Select valid XLSX report start and end dates".into());
    }
    let path=Path::new(source);
    if !path.is_absolute() || !path.extension().and_then(|x|x.to_str())
        .is_some_and(|x|x.eq_ignore_ascii_case("xlsx")) {
        return Err("Select an absolute .xlsx file".into());
    }
    let metadata=fs::symlink_metadata(path).map_err(err)?;
    if !metadata.file_type().is_file() || metadata.len()>Limits::default().archive_bytes {
        return Err("A regular XLSX file <=30 MiB is required".into());
    }
    let source_name=path.file_name().and_then(|n|n.to_str())
        .ok_or("XLSX file name is not valid UTF-8")?.to_owned();
    if !metric_reports::label(&source_name) {
        return Err("XLSX file name too long or empty".into());
    }
    let area=StageArea::open(store.root.join("metric-xlsx-staging")).map_err(err)?;
    let cancel=AtomicBool::new(false);
    let stage=area.ingest(path,Limits::default(),&cancel,|_|{}).map_err(err)?;
    let sheets=stage.sheet_names().map_err(err)?;
    let sheet=sheets.iter().find(|(name,_)|name==&profile.sheet)
        .ok_or("XLSX worksheet named in the profile was not found")?.1.as_str();
    let rows=decode_stage(&stage,sheet,&profile)?;
    let mut hasher=Sha256::new();
    hasher.update(b"metric-xlsx-v1\0");
    hasher.update(stage.source_hash.as_bytes());
    hasher.update(serde_json::to_vec(&profile).map_err(err)?);
    for item in [start,end,&source_name] {
        hasher.update((item.len() as u32).to_le_bytes());
        hasher.update(item.as_bytes());
    }
    let digest=hasher.finalize().to_vec();
    let key=format!("{:x}",Sha256::digest(&digest));
    let report_id=format!("xlsx-{}-{}",&profile.kind,&key[..24]);
    Ok((MetricDocument{
        kind:profile.kind,id:report_id,start:start.into(),end:end.into(),
        source_name,rows,
    },digest))
}
impl Storage {
    pub fn metric_xlsx_preview(
        &self,source:&str,profile:&str,start:&str,end:&str,
    ) -> Result<MetricReview,String> {
        let (document,digest)=from_excel(self,source,profile,start,end)?;
        self.metric_preview_decoded(document,&digest)
    }
    pub fn metric_xlsx_apply(
        &self,source:&str,profile:&str,start:&str,end:&str,token:&str,
    ) -> Result<MetricReceipt,String> {
        let (document,digest)=from_excel(self,source,profile,start,end)?;
        self.metric_apply_decoded(document,&digest,token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::io::Write;
    use zip::{ZipWriter,write::SimpleFileOptions};

    fn book(path:&Path, header:&str, hours:&str, formula:bool) {
        let rows=[
            [("A1","Подразделение"),("B1","Работа"),("C1",header),("D1","Объём")],
            [("A2","СМУ16"),("B2","Монтаж"),("C2",hours),("D2","16")],
            [("A3","АВТО"),("B3","Ремонт"),("C3","4"),("D3","3")],
        ];
        let mut xml=String::from("<worksheet><sheetData>");
        for row in rows {
            xml.push_str("<row>");
            for (address,value) in row {
                if formula && address=="C2" {
                    xml.push_str(&format!("<c r=\"{address}\"><f>2+6</f><v>8</v></c>"));
                } else {
                    xml.push_str(&format!(
                        "<c r=\"{address}\" t=\"inlineStr\"><is><t>{value}</t></is></c>"
                    ));
                }
            }
            xml.push_str("</row>");
        }
        xml.push_str("</sheetData></worksheet>");
        let mut zip=ZipWriter::new(fs::File::create(path).unwrap());
        for (name,content) in [
            ("xl/worksheets/sheet1.xml",xml),
            ("xl/workbook.xml","<workbook xmlns:r=\"r\"><sheets><sheet name=\"Data\" r:id=\"r1\"/></sheets></workbook>".to_string()),
            ("xl/_rels/workbook.xml.rels","<Relationships><Relationship Id=\"r1\" Target=\"/xl/worksheets/sheet1.xml\" Type=\"worksheet\"/></Relationships>".to_string()),
        ] {
            zip.start_file(name,SimpleFileOptions::default()).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    #[test]
    fn profiles_are_valid_and_reject_unmapped_derived_values() {
        let all=profiles().unwrap();
        assert_eq!(all.len(),2);
        assert_eq!(all[0].id,"labor_simple");
        let mut bad=all[0].clone();
        bad.values.insert("units_per_hour".into(),"E".into());
        assert!(validate(&bad,&metric_reports::kind_by_id("labor").unwrap()).is_err());
        let mut bad_header=all[0].clone();
        bad_header.headers.remove("D");
        assert!(validate(&bad_header,&metric_reports::kind_by_id("labor").unwrap()).is_err());
    }
    #[test]
    fn mapped_xlsx_preview_and_save_keep_classic_reports_untouched() {
        let t=tempdir().unwrap();
        let source=t.path().join("worked.xlsx");
        book(&source,"Часы","8",false);
        let store=Storage::new(t.path().join("data")).unwrap();
        let source=source.to_str().unwrap();
        let preview=store.metric_xlsx_preview(source,"labor_simple","2026-11-18","2026-11-18").unwrap();
        assert_eq!(preview.kind,"labor");
        assert_eq!(preview.row_count,2);
        assert_eq!(preview.values["hours"],Some(12.0));
        assert_eq!(preview.values["units"],Some(19.0));
        assert_eq!(preview.values["units_per_hour"],Some(19.0/12.0));
        let receipt=store.metric_xlsx_apply(source,"labor_simple","2026-11-18","2026-11-18",&preview.review_token).unwrap();
        assert_eq!(receipt.row_count,2);
        let summary=store.metric_summary("labor","2026-11-18","2026-11-18").unwrap();
        assert_eq!(summary.report_count,1);
        assert_eq!(summary.row_count,2);
        assert_eq!(summary.values["units_per_hour"],Some(19.0/12.0));
        assert!(store.load().unwrap()["reports"].as_array().unwrap().is_empty());
        assert!(store.metric_xlsx_apply(source,"labor_simple","2026-11-18","2026-11-18",&preview.review_token).is_err());
    }
    #[test]
    fn wrong_column_name_and_formula_are_rejected_before_sql_write() {
        let t=tempdir().unwrap();
        let source=t.path().join("checked.xlsx");
        let store=Storage::new(t.path().join("data")).unwrap();
        book(&source,"Затраты","8",false);
        let input=source.to_str().unwrap();
        assert!(store.metric_xlsx_preview(input,"labor_simple","2026-11-18","2026-11-18")
            .unwrap_err().contains("header mismatch"));
        book(&source,"Часы","8",true);
        assert!(store.metric_xlsx_preview(input,"labor_simple","2026-11-18","2026-11-18")
            .unwrap_err().contains("Formula"));
        book(&source,"Часы","not a number",false);
        assert!(store.metric_xlsx_preview(input,"labor_simple","2026-11-18","2026-11-18")
            .unwrap_err().contains("Invalid numeric"));
        let result=store.metric_summary("labor","2026-11-18","2026-11-18").unwrap();
        assert_eq!(result.report_count,0);
    }
    #[test]
    fn source_and_requested_dates_are_bound_to_review_token() {
        let t=tempdir().unwrap();
        let source=t.path().join("changed.xlsx");
        let store=Storage::new(t.path().join("data")).unwrap();
        book(&source,"Часы","8",false);
        let input=source.to_str().unwrap();
        let preview=store.metric_xlsx_preview(input,"labor_simple","2026-11-18","2026-11-18").unwrap();
        assert!(store.metric_xlsx_apply(input,"labor_simple","2026-11-19","2026-11-19",&preview.review_token).is_err());
        book(&source,"Часы","9",false);
        assert!(store.metric_xlsx_apply(input,"labor_simple","2026-11-18","2026-11-18",&preview.review_token).is_err());
        assert_eq!(store.metric_summary("labor","2026-11-18","2026-11-18").unwrap().report_count,0);
    }
}
