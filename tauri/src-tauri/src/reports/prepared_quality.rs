//! Read-only diagnostics from a reviewed Excel stage. Shared source issues
//! are emitted once, using their actual date and organization when known.
use super::*;
use analiz_import_core::PreparedImport;

fn visit(
    prepared: &PreparedImport<'_>,
    filter: &UnifiedQualityFilter,
    mut consume: impl FnMut(Value) -> Result<(), String>,
) -> Result<(), String> {
    checked_filter(filter)?;
    let source = prepared.source_name();
    if filter.source.as_ref().is_some_and(|s| !source.to_lowercase().contains(&s.to_lowercase())) {
        return Ok(());
    }
    let mut accept = |mut issue: Value, kind: &str, period: Option<(&str, &str)>| {
        issue["source_name"] = json!(source);
        if let Some((start,end)) = period {
            issue["period_start"] = json!(start);
            issue["period_end"] = json!(end);
        }
        let date = issue["date"].as_str().or_else(||issue["period_start"].as_str());
        if filter.report_type.as_deref().is_some_and(|t|t!=kind)
            || filter.from.as_deref().is_some_and(|lo|date.is_none_or(|d|d<lo))
            || filter.to.as_deref().is_some_and(|hi|date.is_none_or(|d|d>hi))
            || !matches_issue(&issue,filter) { return Ok(()); }
        consume(issue)
    };
    let mut after=0;
    while let Some(report)=prepared.next_report(after).map_err(err)? {
        after=report.ordinal;
        if report.quality.issues==0 { continue; }
        let mut seq=0;
        loop {
            let page=prepared.issues(&report,seq,50).map_err(err)?;
            if page.is_empty(){break;}
            for issue in page {
                seq=issue.sequence;
                if issue.source_issue_sequence.is_some(){continue;}
                let mut value=serde_json::to_value(issue).map_err(err)?;
                value["sheet"]=json!(report.info.sheet);
                accept(value,&report.kind,Some((&report.start,&report.end)))?;
            }
        }
    }
    let mut seq=0;
    loop {
        let page=prepared.source_issues(seq,50).map_err(err)?;
        if page.is_empty(){break;}
        for issue in page {
            seq=issue.sequence;
            accept(serde_json::to_value(issue).map_err(err)?,"day",None)?;
        }
    }
    Ok(())
}

impl Storage {
    pub fn prepared_quality_page(&self,revision:i64,prepared:&PreparedImport<'_>,filter:&UnifiedQualityFilter,offset:u32)->Result<UnifiedQualityPage,String>{
        let conn=self.connect()?;
        read_model::check_revision(&conn,revision)?;
        let mut result=UnifiedQualityPage{revision,total:0,counts:BTreeMap::new(),next_offset:None,issues:Vec::new()};
        visit(prepared,filter,|issue|result.collect(issue,offset,50))?;
        read_model::check_revision(&conn,revision)?;
        result.finish(offset,50)?;
        Ok(result)
    }
    pub fn prepared_quality_csv(&self,revision:i64,prepared:&PreparedImport<'_>,filter:&UnifiedQualityFilter,destination:&Path)->Result<(),String>{
        let conn=self.connect()?;
        read_model::check_revision(&conn,revision)?;
        write_quality_export(destination,|writer|{
            visit(prepared,filter,|issue|write_quality_row(writer,&issue))?;
            read_model::check_revision(&conn,revision)
        })
    }
}
