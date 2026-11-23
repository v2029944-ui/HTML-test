//! Complete chart tables from the same revision, date buckets and work identities
//! as the paged renderer. Only a single output row is held in Rust.
use super::*;
use Cell::{Number, Text};

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrendTable { Works, Organizations, OrganizationTotals }
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrendMetric { Count, Amount, Average }
impl TrendMetric {
    fn label(self) -> &'static str {
        match self { Self::Count => "Количество / объём", Self::Amount => "Сумма", Self::Average => "Сумма / количество (объём)" }
    }
    fn value(self, value: &AnalysisTotals) -> Option<f64> {
        match self { Self::Count => value.count, Self::Amount => value.amount, Self::Average => value.average }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrendCsvOptions {
    pub table: TrendTable,
    pub metrics: Vec<TrendMetric>,
    pub hide_empty: bool,
}
// Use the same range aggregates as the organization page; summing a second
// time after grouping into buckets can change the displayed rounding.
fn organization_totals_csv(
    tx: &Connection,
    scope: &AnalysisScope,
    metrics: &[TrendMetric],
    destination: &Path,
) -> Result<(), String> {
    let mut stmt = tx.prepare(
        "SELECT f.item,(SELECT count(*) FROM an_missing m WHERE slot=1 AND m.org=f.item),
         coalesce(sum(v.count),0),CASE WHEN COUNT(v.key)=0 THEN 0 WHEN COUNT(v.amount)=COUNT(v.key) THEN SUM(v.amount) END FROM analysis_filter_items f
         LEFT JOIN an_values v ON v.slot=1 AND v.org=f.item WHERE f.id=?1 GROUP BY f.item ORDER BY f.item"
    ).map_err(err)?;
    let mut rows = stmt.query([&scope.organizations]).map_err(err)?;
    export(destination, |w| {
        let mut head = vec![Text("Подразделение")];
        head.extend(metrics.iter().map(|m| Text(m.label())));
        head.push(Text("Полнота данных"));
        csv_record(w, &head)?;
        while let Some(row) = rows.next().map_err(err)? {
            let name: String = row.get(0).map_err(err)?;
            let missing: i64 = row.get(1).map_err(err)?;
            let value = totals(row.get(2).map_err(err)?, row.get(3).map_err(err)?, missing == 0)?;
            let coverage = if missing == 0 { "Полные данные".to_owned() } else { format!("Пропусков: {missing}") };
            let mut line = vec![Text(&name)];
            line.extend(metrics.iter().map(|m| Number(m.value(&value))));
            line.push(Text(&coverage));
            csv_record(w, &line)?;
        }
        Ok(())
    })
}

impl Storage {
    pub fn reference_trend_csv(&self, revision: i64, scope: &AnalysisScope, range: &RangeQuery,
        grouping: Option<&str>, options: &TrendCsvOptions, destination: &Path) -> Result<(), String> {
        if !destination.is_absolute() || destination.as_os_str().len()>32767 ||
            !destination.extension().and_then(|v|v.to_str()).is_some_and(|v|v.eq_ignore_ascii_case("csv")) {
            return Err("Choose an absolute CSV file path".into());
        }
        if options.metrics.is_empty() || options.metrics.len()>3 ||
            (options.table==TrendTable::Works && options.metrics.len()!=1) ||
            options.metrics.iter().enumerate().any(|(i,m)|options.metrics[..i].contains(m)) {
            return Err("Select valid chart metrics".into());
        }
        let (lo,hi)=range_bounds(&scope.report_type,range)?;
        if (hi-lo).num_days()>3660 { return Err("Select up to ten years".into()); }
        let mut c=self.connect()?;
        let tx=c.transaction().map_err(err)?;
        workspace(&tx,revision,scope)?;
        fill_range(&tx,scope,range,1)?;
        if options.table == TrendTable::OrganizationTotals {
            return organization_totals_csv(&tx, scope, &options.metrics, destination);
        }
        let periods=count_sql(&tx,"SELECT count(*) FROM an_periods WHERE slot=1")?;
        let (mode,_)=timeline_grouping(scope,range,grouping,periods)?;
        let works=options.table==TrendTable::Works;
        let member=if works {"coalesce(g.key,'work:'||x.work)"} else {"v.org"};
        tx.execute("CREATE TEMP TABLE trend_dates AS SELECT start,
            CASE ?1 WHEN 1 THEN substr(start,1,7) WHEN 2 THEN date(start,'-'||((cast(strftime('%w',start) AS integer)+6)%7)||' days')
                    WHEN 3 THEN substr(start,1,4)||'-01-01' ELSE start END label FROM an_periods WHERE slot=1",[mode]).map_err(err)?;
        tx.execute_batch("CREATE UNIQUE INDEX trend_date_start ON trend_dates(start);
            CREATE TEMP TABLE trend_missing AS SELECT m.org,d.label,count(*) missing
            FROM an_missing m JOIN trend_dates d USING(start) WHERE m.slot=1 GROUP BY m.org,d.label;
            CREATE UNIQUE INDEX trend_org_missing ON trend_missing(org,label);
            CREATE TEMP TABLE trend_series(member TEXT,label TEXT,qty REAL,money REAL,PRIMARY KEY(member,label)) WITHOUT ROWID;").map_err(err)?;
        tx.execute(&format!("INSERT INTO trend_series SELECT {member},d.label,sum(x.count),CASE WHEN COUNT(x.amount)=COUNT(*) THEN SUM(x.amount) END
            FROM trend_dates d JOIN an_valid v ON v.start=d.start JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org
            LEFT JOIN analysis_groups g ON g.work=x.work WHERE ?1 IS NULL OR EXISTS(
                SELECT 1 FROM analysis_filter_items f WHERE f.id=?1 AND f.item=coalesce(g.name,x.work))
            GROUP BY {member},d.label"),[&scope.names]).map_err(err)?;
        tx.execute_batch("CREATE TEMP TABLE trend_buckets(
            label TEXT PRIMARY KEY,periods INTEGER,missing INTEGER,qty REAL,money REAL,active INTEGER,first_start TEXT,last_start TEXT
        ) WITHOUT ROWID;").map_err(err)?;
        // The screen and export must aggregate the original rows in the same order.
        // Adding per-member floating-point sums can change the last displayed cent.
        tx.execute(&format!("INSERT INTO trend_buckets {TIMELINE_BUCKET_TOTALS}"),
            params![mode, &scope.organizations, &scope.names]).map_err(err)?;
        if count_sql(&tx,"SELECT count(*) FROM trend_buckets")?>124 {return Err("Analysis timeline exceeds 124 points".into());}
        if works {
            super::unified_analytics::presentation_functions(&tx)?;
            super::unified_analytics::create_name_catalog(&tx,scope.names.is_none())?;
        }
        // Missing buckets remain visible; cancelling positive and negative entries
        // also remain visible even when both their aggregate measures equal zero.
        let visible="(?1=0 OR b.active<>0 OR b.missing<>0)";
        let hide=options.hide_empty;
        let mut axis = tx.prepare(&format!(
            "SELECT label, first_start, last_start FROM trend_buckets b WHERE {visible} ORDER BY label"
        )).map_err(err)?;
        let mut dates = axis.query([hide]).map_err(err)?;
        let mut labels = Vec::new();
        while let Some(row) = dates.next().map_err(err)? {
            let label: String = row.get(0).map_err(err)?;
            let first: String = row.get(1).map_err(err)?;
            let last: String = row.get(2).map_err(err)?;
            let through = end(&scope.report_type, date(&last)?)?.to_string();
            let heading = if first == through {
                first
            } else if mode == 1 && first == format!("{label}-01") && end("month", date(&first)?)?.to_string() == through {
                label
            } else {
                format!("{first} — {through}")
            };
            labels.push(heading);
        }
        let members=if works {"SELECT n.key,n.label FROM unified_names n
            LEFT JOIN analysis_filter_items f ON f.id=?1 AND f.item=n.name WHERE ?1 IS NULL OR f.id IS NOT NULL
            ORDER BY f.ordinal,n.name COLLATE classic_ru,n.key"}
            else {"SELECT item,item FROM analysis_filter_items WHERE id=?1 ORDER BY ordinal"};
        let mut names=tx.prepare(members).map_err(err)?;
        let mut names=names.query([if works {scope.names.as_deref()} else {Some(scope.organizations.as_str())}]).map_err(err)?;
        let mut points=tx.prepare(&format!("SELECT coalesce(s.qty,0.0),CASE WHEN s.member IS NULL THEN 0.0 ELSE s.money END,
            CASE WHEN ?3 THEN b.missing ELSE coalesce(m.missing,0) END
            FROM trend_buckets b LEFT JOIN trend_series s ON s.member=?2 AND s.label=b.label
            LEFT JOIN trend_missing m ON m.org=?2 AND m.label=b.label WHERE {visible} ORDER BY b.label")).map_err(err)?;
        export(destination,|w|{
            let mut head=vec![Text(if works {"Работа / группа"}else{"Подразделение"})];
            if !works {head.push(Text("Показатель"));}
            head.extend(labels.iter().map(|v|Text(v)));
            csv_record(w,&head)?;
            if works {
                let metric=options.metrics[0];let mut line=vec![Text("Итого по выбранным работам")];
                let mut stmt=tx.prepare(&format!("SELECT qty,money,missing FROM trend_buckets b WHERE {visible} ORDER BY label")).map_err(err)?;
                let mut rows=stmt.query([hide]).map_err(err)?;
                while let Some(r)=rows.next().map_err(err)?{
                    line.push(Number(metric.value(&totals(r.get(0).map_err(err)?,r.get(1).map_err(err)?,r.get::<_,i64>(2).map_err(err)?==0)?)));
                }
                csv_record(w,&line)?;
            }
            while let Some(row)=names.next().map_err(err)? {
                let key:String=row.get(0).map_err(err)?;let name:String=row.get(1).map_err(err)?;
                let mut values=Vec::with_capacity(labels.len());
                let mut rows=points.query(params![hide,key,works]).map_err(err)?;
                while let Some(r)=rows.next().map_err(err)? {
                    let count:f64=r.get(0).map_err(err)?;let amount:Option<f64>=r.get(1).map_err(err)?;
                    let missing:i64=r.get(2).map_err(err)?;
                    values.push(totals(count,amount,missing==0)?);
                }
                for metric in &options.metrics {
                    let mut line=vec![Text(&name)];if !works {line.push(Text(metric.label()));}
                    line.extend(values.iter().map(|value|Number(metric.value(value))));csv_record(w,&line)?;
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trend_exports_all_names_including_groups_preserves_cancellation_missing_and_observed_zero() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::new(dir.path().join("data")).unwrap();
        let mut works:Vec<_>=(1..=52).map(|n|json!({"organization":"A","work":format!("Work {n:02}"),"count":1,"amount":n})).collect();
        works.push(json!({"organization":"A","work":"=Formula","count":1,"amount":10}));
        let data = json!({"reports":[
            {"id":"one","type":"day","start":"2024-01-01","end":"2024-01-01","organizations":["A","B"],"rows":works},
            {"id":"zero","type":"day","start":"2024-01-02","end":"2024-01-02","organizations":["A","B"],"rows":[]},
            {"id":"partial","type":"day","start":"2024-01-03","end":"2024-01-03","organizations":["A"],"rows":[{"organization":"A","work":"Work 52","count":1,"amount":5}]},
            {"id":"cancel","type":"day","start":"2024-01-04","end":"2024-01-04","organizations":["A","B"],"rows":[
                {"organization":"A","work":"Work 52","count":4,"amount":30},{"organization":"A","work":"Work 52","count":-4,"amount":-30}]}
        ],"settings":{"orgNames":["A","B","C","D"],"groups":[{"id":"g","name":"Grouped","works":["Work 01","Work 02"]}],"catalog":[]}});
        store.save(data).unwrap();
        let rev=store.revision().unwrap();
        for id in ["one","zero","partial","cancel"]{store.analysis_index_report(id,rev).unwrap();}
        store.analysis_index_settings(rev).unwrap();
        let orgs=store.analysis_filter_create(rev,"organizations").unwrap();
        store.analysis_filter_append(rev,&orgs,&["A".into(),"B".into()]).unwrap();
        store.analysis_filter_seal(rev,&orgs).unwrap();
        let q=RangeQuery{from:"2024-01-01".into(),to:"2024-01-04".into()};
        let scope=AnalysisScope{report_type:"day".into(),organizations:orgs,names:None};
        let csv=dir.path().join("works.csv");
        let options=TrendCsvOptions{table:TrendTable::Works,metrics:vec![TrendMetric::Amount],hide_empty:true};
        store.reference_trend_csv(rev,&scope,&q,Some("day"),&options,&csv).unwrap();
        let content=fs::read_to_string(&csv).unwrap();
        assert_eq!(content.lines().count(),54); // Header, total and 52 distinct series.
        assert!(content.starts_with("\u{feff}\"Работа / группа\";\"2024-01-01\";\"2024-01-03\";\"2024-01-04\""));
        assert!(content.contains("\"Grouped\";\"3\";\"\";\"0\""));
        assert!(content.contains("\"Work 52\";\"52\";\"\";\"0\""));
        assert!(content.contains("\"'=Formula\";\"10\";\"\";\"0\""));
        assert!(!content.contains("\"Work 01\""));
        assert!(store.reference_trend_csv(rev,&scope,&q,None,&options,&csv).is_err());
        assert_eq!(content,fs::read_to_string(csv).unwrap());
        let points=store.analysis_timeline_grouped(rev,&scope,&q,Some("day")).unwrap().points;
        assert!(!points[1].has_activity && points[3].has_activity);
        let orgcsv=dir.path().join("orgs.csv");
        let options=TrendCsvOptions{table:TrendTable::Organizations,metrics:vec![TrendMetric::Amount,TrendMetric::Count],hide_empty:false};
        store.reference_trend_csv(rev,&scope,&q,Some("day"),&options,&orgcsv).unwrap();
        let content=fs::read_to_string(orgcsv).unwrap();
        assert!(content.contains("\"B\";\"Сумма\";\"0\";\"0\";\"\";\"0\""));
        assert!(content.contains("\"A\";\"Количество / объём\";\"53\";\"0\";\"1\";\"0\""));
        let dest=dir.path().join("org-totals.csv");
        let options=TrendCsvOptions{table:TrendTable::OrganizationTotals,..options};
        store.reference_trend_csv(rev,&scope,&q,Some("day"),&options,&dest).unwrap();
        let content=fs::read_to_string(dest).unwrap();
        assert!(content.contains("\"A\";\"1393\";\"54\";\"Полные данные\""));
        assert!(content.contains("\"B\";\"\";\"\";\"Пропусков: 1\""));
        assert_eq!(store.revision().unwrap(),rev);
    }
    #[test]
    fn monthly_chart_csv_uses_the_same_source_period_boundaries_as_all_three_timelines() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::new(dir.path().join("data")).unwrap();
        store.save(json!({"reports":[{"id":"sample","type":"day","start":"2024-01-28",
            "end":"2024-01-28","organizations":["A"],"rows":[{"organization":"A","work":"X","count":1,"amount":10}]}],
            "settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}})).unwrap();
        let rev = store.revision().unwrap();
        store.analysis_index_report("sample",rev).unwrap();
        store.analysis_index_settings(rev).unwrap();
        let orgs = store.analysis_filter_create(rev,"organizations").unwrap();
        store.analysis_filter_append(rev,&orgs,&["A".into()]).unwrap();
        store.analysis_filter_seal(rev,&orgs).unwrap();
        for (kind,from,to,first,last,heading) in [
            ("day","2024-01-28","2024-06-03","2024-01-28","2024-01-31","2024-01-28 — 2024-01-31"),
            ("week","2023-12-27","2026-05-20","2023-12-27","2023-12-27","2023-12-27 — 2024-01-02"),
        ] {
            let scope = AnalysisScope {report_type:kind.into(),organizations:orgs.clone(),names:None};
            let range = RangeQuery {from:from.into(),to:to.into()};
            let timeline = store.analysis_timeline(rev,&scope,&range).unwrap();
            let trends = store.analysis_organization_trends(rev,&scope,&range).unwrap();
            let work = store.analysis_work_timeline(rev,&scope,&range,None,"work:X").unwrap();
            for (start,stop) in [
                (&timeline.points[0].first_start,&timeline.points[0].last_start),
                (&trends.series[0].points[0].first_start,&trends.series[0].points[0].last_start),
                (&work.points[0].first_start,&work.points[0].last_start),
            ] { assert_eq!((start.as_str(),stop.as_str()),(first,last)); }
            let path = dir.path().join(format!("{kind}.csv"));
            let options = TrendCsvOptions {table:TrendTable::Organizations,metrics:vec![TrendMetric::Amount],hide_empty:false};
            store.reference_trend_csv(rev,&scope,&range,None,&options,&path).unwrap();
            let csv = fs::read_to_string(path).unwrap();
            let header = csv.lines().next().unwrap();
            assert!(header.contains(heading),"{header}");
            if kind=="day" {
                assert!(header.contains(r#""2024-02""#) && header.contains("2024-06-01 — 2024-06-03"),"{header}");
            } else {
                assert!(header.contains("2024-01-03 — 2024-02-06"),"{header}");
            }
        }
    }

    #[test]
    fn all_work_series_keep_namesakes_and_match_the_unrounded_screen_total() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage::new(dir.path().join("data")).unwrap();
        let row = |work: &str, amount: f64| json!({"organization":"A","work":work,"count":1,"amount":amount});
        let report = |day: &str, rows: Vec<Value>| json!({"id":day,"type":"day","start":day,"end":day,"organizations":["A"],"rows":rows});
        let first = "2025-03-01";
        let second = "2025-03-02";
        store.save(json!({
            "reports":[report(first,vec![row("Сварка",100.),row("Ремонт",40.),row("Уборка",10.)]),
                       report(second,vec![row("Сварка",1.453),row("Монтаж",0.605),row("Ремонт",1.167)])],
            "settings":{"orgNames":["A","B","C","D"],"groups":[{"id":"g","name":"Ремонт","works":["Сварка","Монтаж"]}],"catalog":[]}
        })).unwrap();
        let rev = store.revision().unwrap();
        for id in [first,second] {store.analysis_index_report(id,rev).unwrap();}
        store.analysis_index_settings(rev).unwrap();
        let orgs = store.analysis_filter_create(rev,"organizations").unwrap();
        store.analysis_filter_append(rev,&orgs,&["A".into()]).unwrap();
        store.analysis_filter_seal(rev,&orgs).unwrap();
        let mut scope = AnalysisScope {report_type:"day".into(),organizations:orgs,names:None};
        let range = RangeQuery {from:first.into(),to:second.into()};
        let all = store.unified_work_series_at_page(rev,0,50).unwrap();
        let keys: Vec<_> = all.rows.iter().map(|row|(row.key.as_str(),row.name.as_str())).collect();
        assert_eq!(keys.len(),3);
        assert!(keys.contains(&("group:g","Ремонт (группа)")));
        assert!(keys.contains(&("work:Ремонт","Ремонт (работа)")));
        let selectable = store.unified_name_options_at_page(rev,"",0,50).unwrap();
        assert!(selectable.has_omitted_names);
        assert_eq!(selectable.rows.len(),1);
        assert_eq!(selectable.rows[0].name,"Уборка");
        let options = TrendCsvOptions {table:TrendTable::Works,metrics:vec![TrendMetric::Amount],hide_empty:false};
        let destination = dir.path().join("all.csv");
        store.reference_trend_csv(rev,&scope,&range,Some("day"),&options,&destination).unwrap();
        let csv = fs::read_to_string(destination).unwrap();
        let timeline = store.analysis_timeline_grouped(rev,&scope,&range,Some("day")).unwrap();
        let amount = timeline.points[1].total.amount.unwrap();
        assert!((amount-3.225).abs()<1e-12);
        assert!(csv.contains(&format!(r#""Итого по выбранным работам";"150";"{}""#,number_text(amount))),"{csv}");
        for (name,amount) in [("Ремонт (группа)",100),("Ремонт (работа)",40),("Уборка",10)]{
            assert!(csv.contains(&format!(r#""{name}";"{amount}";"#)),"{csv}");
        }
        let selected = store.analysis_filter_create(rev,"names").unwrap();
        store.analysis_filter_append(rev,&selected,&["Уборка".into()]).unwrap();
        store.analysis_filter_seal(rev,&selected).unwrap();
        scope.names=Some(selected);
        let destination = dir.path().join("selected.csv");
        store.reference_trend_csv(rev,&scope,&range,Some("day"),&options,&destination).unwrap();
        let csv = fs::read_to_string(destination).unwrap();
        assert!(csv.contains(r#""Итого по выбранным работам";"10";"0""#));
        assert!(csv.contains(r#""Уборка";"10";"0""#));
        assert!(!csv.contains("Ремонт"));
    }

}
