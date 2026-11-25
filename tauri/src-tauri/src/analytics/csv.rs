//! Bounded writers for analysis CSV exports.
use super::*;
use crate::storage::csv_export::export_before;

pub(super) fn changes_csv_header(w: &mut impl Write, metrics: &[&str]) -> Result<(), String> {
    let prefix = ["key", "name", "kind", "org", "baseStart", "currentStart", "status"];
    let mut headers: Vec<_> = prefix.iter().map(|name| (*name).to_owned()).collect();
    for metric in metrics {
        for field in ["base", "current", "delta", "percent"] {
            headers.push(format!("{metric}.{field}"));
        }
    }
    csv_record(w, &headers.iter().map(|h| Cell::Text(h)).collect::<Vec<_>>())
}

pub(super) fn change_csv(w: &mut impl Write, v: &ChangeValue, metrics: &[&str]) -> Result<(), String> {
    use Cell::*;
    let mut row = vec![Text(&v.key), Text(&v.name), Text(&v.kind), Text(&v.org),
                       Text(&v.base_start), Text(&v.current_start), Text(&v.status)];
    for &metric in metrics {
        let values = match metric {
            "count" | "amount" => {
                let measure = if metric == "count" { Some(&v.count) } else { v.amount.as_ref() };
                measure.map_or([None; 4], |m| [Some(m.base), Some(m.current), Some(m.delta), m.percent])
            }
            "average" => [v.average.base, v.average.current, v.average.delta, v.average.percent],
            _ => return Err("Unknown change metric".into()),
        };
        row.extend(values.into_iter().map(Number));
    }
    csv_record(w, &row)
}
impl Analytics<'_> {
    pub(super) fn analysis_export_changes(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        destination: &Path,
    ) -> Result<(), String> {
        self.with_snapshot(expected, s, |tx| {
            fill_changes(tx, s, q)?;
            export_changes_table(tx, q, destination)
        })
    }
    pub(super) fn analysis_export_range(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        org: Option<&str>,
        destination: &Path,
    ) -> Result<(), String> {
        self.analysis_export_range_before_publish(expected, s, q, org, destination, || {})
    }
    pub(super) fn analysis_export_range_before_publish(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        org: Option<&str>,
        destination: &Path,
        before: impl FnOnce(),
    ) -> Result<(), String> {
        let mut c = self.database.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        org_check(&tx, s, org)?;
        fill_range(&tx, s, q, 1)?;
        let available = complete(&tx, 1, org)?;
        export_before(
            destination,
            |w| {
                use Cell::*;
                csv_record(
                    w,
                    &[
                        Text("key"),
                        Text("name"),
                        Text("kind"),
                        Text("count"),
                        Text("amount"),
                        Text("average"),
                        Text("active"),
                    ],
                )?;
                let sql = if org.is_some() {
                    "SELECT key,name,kind,count,amount,active FROM temp.an_values WHERE slot=1 AND org=?1 ORDER BY key"
                } else {
                    "SELECT key,name,kind,count,amount,active FROM temp.an_combined WHERE slot=1 AND ?1 IS NULL ORDER BY key"
                };
                let mut stmt = tx.prepare(sql).map_err(err)?;
                let mut rows = stmt.query([org]).map_err(err)?;
                while let Some(r) = rows.next().map_err(err)? {
                    let key: String = r.get(0).map_err(err)?;
                    let name: String = r.get(1).map_err(err)?;
                    let kind: String = r.get(2).map_err(err)?;
                    let t = totals(r.get(3).map_err(err)?, r.get(4).map_err(err)?, available)?;
                    let active: bool = r.get(5).map_err(err)?;
                    csv_record(
                        w,
                        &[
                            Text(&key),
                            Text(&name),
                            Text(&kind),
                            Number(t.count),
                            Number(t.amount),
                            Number(t.average),
                            Text(if active { "true" } else { "false" }),
                        ],
                    )?;
                }
                Ok(())
            },
            before,
        )
    }
    pub(super) fn analysis_export_comparison(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ComparisonQuery,
        org: Option<&str>,
        destination: &Path,
    ) -> Result<(), String> {
        let mut c = self.database.connect()?;
        let tx = c.transaction().map_err(err)?;
        workspace(&tx, expected, s)?;
        let (b, a) = comparison(&tx, s, q, org)?;
        export(destination, |w| {
            use Cell::*;
            csv_record(
                w,
                &[
                    Text("key"),
                    Text("name"),
                    Text("kind"),
                    Text("status"),
                    Text("count.base"),
                    Text("count.current"),
                    Text("count.delta"),
                    Text("count.percent"),
                    Text("amount.base"),
                    Text("amount.current"),
                    Text("amount.delta"),
                    Text("amount.percent"),
                    Text("average.base"),
                    Text("average.current"),
                    Text("average.delta"),
                    Text("average.percent"),
                ],
            )?;
            let mut stmt = tx
                .prepare("SELECT key,name,kind,bc,ba,cc,ca FROM temp.an_comparison ORDER BY key")
                .map_err(err)?;
            let mut rows = stmt.query([]).map_err(err)?;
            while let Some(r) = rows.next().map_err(err)? {
                let v = comparison_value(r, q, b, a)?;
                csv_record(
                    w,
                    &[
                        Text(&v.key),
                        Text(&v.name),
                        Text(&v.kind),
                        Text(v.status.as_deref().unwrap_or("")),
                        Number(if b {
                            Some(r.get(3).map_err(err)?)
                        } else {
                            None
                        }),
                        Number(if a {
                            Some(r.get(5).map_err(err)?)
                        } else {
                            None
                        }),
                        Number(v.count.as_ref().map(|m| m.delta)),
                        Number(v.count.as_ref().and_then(|m| m.percent)),
                        Number(if b {
                            Some(r.get(4).map_err(err)?)
                        } else {
                            None
                        }),
                        Number(if a {
                            Some(r.get(6).map_err(err)?)
                        } else {
                            None
                        }),
                        Number(v.amount.as_ref().map(|m| m.delta)),
                        Number(v.amount.as_ref().and_then(|m| m.percent)),
                        Number(v.average.base),
                        Number(v.average.current),
                        Number(v.average.delta),
                        Number(v.average.percent),
                    ],
                )?;
            }
            Ok(())
        })
    }
}

impl Storage {
    pub fn analysis_export_changes(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ChangeQuery,
        destination: &Path,
    ) -> Result<(), String> {
        self.analysis().analysis_export_changes(expected, s, q, destination)
    }

    pub fn analysis_export_range(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        org: Option<&str>,
        destination: &Path,
    ) -> Result<(), String> {
        self.analysis().analysis_export_range(expected, s, q, org, destination)
    }

    #[cfg(test)]
    pub(in crate::storage) fn analysis_export_range_before_publish(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &RangeQuery,
        org: Option<&str>,
        destination: &Path,
        before: impl FnOnce(),
    ) -> Result<(), String> {
        self.analysis().analysis_export_range_before_publish(expected, s, q, org, destination, before)
    }

    pub fn analysis_export_comparison(
        &self,
        expected: i64,
        s: &AnalysisScope,
        q: &ComparisonQuery,
        org: Option<&str>,
        destination: &Path,
    ) -> Result<(), String> {
        self.analysis().analysis_export_comparison(expected, s, q, org, destination)
    }
}

fn export_changes_table(
    tx: &Connection,
    q: &ChangeQuery,
    destination: &Path,
) -> Result<(), String> {
    export(destination, |w| write_changes_rows(tx, q, w))
}

pub(super) fn write_changes_rows(tx: &Connection, q: &ChangeQuery, w: &mut impl Write) -> Result<(), String> {
    let metrics = ["count", "amount", "average"];
    changes_csv_header(w, &metrics)?;
    let mut stmt=tx.prepare("SELECT start,base,org,key,name,kind,bc,ba,cc,ca FROM temp.an_changes ORDER BY start,org,key").map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    while let Some(r) = rows.next().map_err(err)? {
        change_csv(w, &change_value(r, q)?, &metrics)?;
    }
    Ok(())
}
