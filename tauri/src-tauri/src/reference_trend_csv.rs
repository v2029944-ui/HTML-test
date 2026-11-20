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
            CREATE TEMP TABLE trend_series(member TEXT,label TEXT,qty REAL,money REAL,active INTEGER,PRIMARY KEY(member,label)) WITHOUT ROWID;").map_err(err)?;
        tx.execute(&format!("INSERT INTO trend_series SELECT {member},d.label,sum(x.count),sum(x.amount),max(x.count<>0 OR x.amount<>0)
            FROM trend_dates d JOIN an_valid v ON v.start=d.start JOIN report_rows x ON x.report_id=v.report_id AND x.organization=v.org
            LEFT JOIN analysis_groups g ON g.work=x.work WHERE ?1 IS NULL OR EXISTS(
                SELECT 1 FROM analysis_filter_items f WHERE f.id=?1 AND f.item=coalesce(g.name,x.work))
            GROUP BY {member},d.label"),[&scope.names]).map_err(err)?;
        tx.execute_batch("CREATE TEMP TABLE trend_buckets AS WITH
            sums AS (SELECT label,sum(qty) qty,sum(money) money,max(active) active FROM trend_series GROUP BY label),
            gaps AS (SELECT label,sum(missing) missing FROM trend_missing GROUP BY label)
            SELECT d.label,coalesce(s.qty,0.0) qty,coalesce(s.money,0.0) money,coalesce(s.active,0) active,
                   coalesce(g.missing,0) missing FROM (SELECT DISTINCT label FROM trend_dates) d
            LEFT JOIN sums s USING(label) LEFT JOIN gaps g USING(label);
            CREATE UNIQUE INDEX trend_buckets_by_label ON trend_buckets(label);").map_err(err)?;
        if count_sql(&tx,"SELECT count(*) FROM trend_buckets")?>124 {return Err("Analysis timeline exceeds 124 points".into());}
        if works {
            super::unified_analytics::presentation_functions(&tx)?;
            super::unified_analytics::create_name_catalog(&tx)?;
        }
        // Missing buckets remain visible; cancelling positive and negative entries
        // also remain visible even when both their aggregate measures equal zero.
        let visible="(?1=0 OR b.active<>0 OR b.missing<>0)";
        let hide=options.hide_empty&&options.table!=TrendTable::OrganizationTotals;
        let mut axis=tx.prepare(&format!("SELECT label FROM trend_buckets b WHERE {visible} ORDER BY label")).map_err(err)?;
        let labels=axis.query_map([hide],|row|row.get::<_,String>(0)).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
        let members=if works {"SELECT n.key,n.name FROM unified_names n
            LEFT JOIN analysis_filter_items f ON f.id=?1 AND f.item=n.name WHERE ?1 IS NULL OR f.id IS NOT NULL
            ORDER BY f.ordinal,n.name COLLATE classic_ru,n.key"}
            else {"SELECT item,item FROM analysis_filter_items WHERE id=?1 ORDER BY ordinal"};
        let mut names=tx.prepare(members).map_err(err)?;
        let mut names=names.query([if works {scope.names.as_deref()} else {Some(scope.organizations.as_str())}]).map_err(err)?;
        let mut points=tx.prepare(&format!("SELECT coalesce(s.qty,0.0),coalesce(s.money,0.0),
            CASE WHEN ?3 THEN b.missing ELSE coalesce(m.missing,0) END
            FROM trend_buckets b LEFT JOIN trend_series s ON s.member=?2 AND s.label=b.label
            LEFT JOIN trend_missing m ON m.org=?2 AND m.label=b.label WHERE {visible} ORDER BY b.label")).map_err(err)?;
        export(destination,|w|{
            let mut head=vec![Text(if works {"Работа / группа"}else{"Подразделение"})];
            if options.table==TrendTable::OrganizationTotals {
                head.extend(options.metrics.iter().map(|m|Text(m.label())));head.push(Text("Полнота данных"));
            } else {
                if !works {head.push(Text("Показатель"));}
                head.extend(labels.iter().map(|v|Text(v)));
            }
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
                let mut covered=0;let mut qty=0.0;let mut money=0.0;
                let mut rows=points.query(params![hide,key,works]).map_err(err)?;
                while let Some(r)=rows.next().map_err(err)? {
                    let count:f64=r.get(0).map_err(err)?;let amount:f64=r.get(1).map_err(err)?;
                    let missing:i64=r.get(2).map_err(err)?;
                    values.push(totals(count,amount,missing==0)?);
                    qty+=count;money+=amount;covered+=missing;
                }
                if options.table==TrendTable::OrganizationTotals {
                    let value=totals(qty,money,covered==0)?;
                    let coverage=if covered==0 {"Полные данные".to_owned()}else{format!("Пропусков: {covered}")};
                    let mut line=vec![Text(&name)];line.extend(options.metrics.iter().map(|m|Number(m.value(&value))));line.push(Text(&coverage));
                    csv_record(w,&line)?;
                } else {
                    for metric in &options.metrics {
                        let mut line=vec![Text(&name)];if !works {line.push(Text(metric.label()));}
                        line.extend(values.iter().map(|value|Number(metric.value(value))));csv_record(w,&line)?;
                    }
                }
            }
            Ok(())
        })
    }
}
