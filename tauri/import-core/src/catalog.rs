//! Catalog extraction from the existing disk-backed worksheet cells.
use super::*;
use std::collections::BTreeMap;

fn normalized(s: &str) -> String {
    s.to_lowercase()
        .trim_end_matches(['.', ',', ';', ':', '：'])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn organization<'a>(value: &str, names: &'a [String]) -> Option<&'a str> {
    let value = normalized(value);
    names
        .iter()
        .find(|s| normalized(s) == value)
        .map(String::as_str)
}
fn work(value: &str) -> bool {
    let value = value.trim();
    let n = normalized(value);
    (3..=240).contains(&value.chars().count())
        && !(n == "итого" || n.starts_with("итого "))
        && !(n == "всего" || n.starts_with("всего "))
        && !n.starts_with("работа № по каталогу")
        && !matches!(
            n.as_str(),
            "подразделение"
                | "организация"
                | "работа"
                | "услуга"
                | "услуги"
                | "список услуг"
                | "наименование"
                | "наименование работы"
                | "наименование услуг"
                | "название услуги"
                | "количество"
                | "на сумму"
                | "код"
                | "артикул"
                | "№"
                | "номер"
        )
}
fn add(c: &Connection, org: &str, value: &str) -> Result<()> {
    if work(value) {
        c.execute(
            "INSERT OR IGNORE INTO catalog_output VALUES(?1,?2,?3,?4)",
            params![normalized(org), normalized(value), org, value.trim()],
        )?;
    }
    Ok(())
}
impl Stage<'_> {
    /// Detect all four classic catalog layouts. Stores results on disk and visits
    /// one row at a time; skips report sheets so report titles never become services.
    pub fn visit_catalog(
        &self,
        names: &[String],
        cancel: &AtomicBool,
        mut accept: impl FnMut(&str, &str) -> Result<()>,
    ) -> Result<u64> {
        if names.len() != 4 || names.iter().any(|s| s.trim().is_empty()) {
            return Err(fail("catalog requires four organizations"));
        }
        self.db.execute_batch("PRAGMA temp_store=FILE; PRAGMA temp.cache_size=-2048; PRAGMA temp.max_page_count=65536; CREATE TEMP TABLE IF NOT EXISTS catalog_output(org_key TEXT,work_key TEXT,organization TEXT,work TEXT,PRIMARY KEY(org_key,work_key)) WITHOUT ROWID;DELETE FROM catalog_output;")?;
        for (sheet_name, sheet) in self.sheet_names()? {
            check_cancel(cancel)?;
            let mut header: BTreeMap<u32, BTreeMap<u32, String>> = BTreeMap::new();
            let mut st=self.db.prepare("SELECT c.row_num,c.col_num,CASE WHEN c.kind IN ('s','inlineStr','str') THEN COALESCE(s.value,c.value,'') ELSE '' END FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.row_num<=20 AND c.col_num<=80 ORDER BY c.row_num,c.col_num")?;
            let mut rows = st.query([&sheet])?;
            while let Some(row) = rows.next()? {
                let value: String = row.get(2)?;
                if value.len() <= 4096 {
                    header
                        .entry(row.get(0)?)
                        .or_default()
                        .insert(row.get(1)?, value);
                }
            }
            drop(rows);
            drop(st);
            let report = header.values().any(|row| {
                row.iter().any(|(col, v)| {
                    let n = normalized(v);
                    (n.contains("работ") || n.contains("услуг") || n.contains("наименован"))
                        && row
                            .get(&(col + 1))
                            .is_some_and(|s| normalized(s).contains("количеств"))
                        && row
                            .get(&(col + 2))
                            .is_some_and(|s| normalized(s).contains("сумм"))
                })
            });
            if report {
                continue;
            }
            let mut table = None;
            let mut columns: Vec<(u32, String)> = Vec::new();
            let mut header_row = 0;
            for (r, row) in &header {
                if *r <= 16 && table.is_none() {
                    let org = row
                        .iter()
                        .find(|(_, v)| {
                            let n = normalized(v);
                            n.contains("подразделени") || n.contains("организаци") || n == "отдел"
                        })
                        .map(|(c, _)| *c);
                    let job = row
                        .iter()
                        .find(|(_, v)| {
                            let n = normalized(v);
                            matches!(
                                n.as_str(),
                                "услуга"
                                    | "услуги"
                                    | "работа"
                                    | "вид работ"
                                    | "наименование"
                                    | "название услуги"
                                    | "наименование услуг"
                            )
                        })
                        .map(|(c, _)| *c);
                    if let (Some(o), Some(w)) = (org, job) {
                        if o != w {
                            table = Some((*r, o, w));
                        }
                    }
                }
                let hits: Vec<_> = row
                    .iter()
                    .filter_map(|(col, value)| {
                        organization(value, names).map(|org| (*col, org.to_string()))
                    })
                    .collect();
                if hits.len() >= 2 && hits.len() > columns.len() {
                    columns = hits;
                    header_row = *r;
                }
            }
            if let Some((r, org_col, work_col)) = table {
                let mut stmt=self.db.prepare("SELECT c.row_num,c.col_num,CASE WHEN c.kind IN ('s','inlineStr','str') THEN COALESCE(s.value,c.value,'') ELSE '' END FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.row_num>?2 AND c.col_num IN (?3,?4) ORDER BY c.row_num,c.col_num")?;
                let mut rows = stmt.query(params![sheet, r, org_col, work_col])?;
                let mut active = None::<String>;
                let mut rownum = 0;
                let mut row_org = None::<String>;
                let mut row_work = None::<String>;
                let start: i64 =
                    self.db
                        .query_row("SELECT COUNT(*) FROM catalog_output", [], |r| r.get(0))?;
                while let Some(row) = rows.next()? {
                    check_cancel(cancel)?;
                    let number: u32 = row.get(0)?;
                    if number != rownum {
                        if let Some(org) = row_org.take() {
                            active = Some(org);
                        }
                        if let (Some(org), Some(value)) = (&active, row_work.take()) {
                            add(&self.db, org, &value)?;
                        }
                        rownum = number;
                    }
                    let col: u32 = row.get(1)?;
                    let value: String = row.get(2)?;
                    if col == org_col {
                        row_org = organization(&value, names).map(str::to_string);
                    } else {
                        row_work = Some(value);
                    }
                }
                if let Some(org) = row_org {
                    active = Some(org);
                }
                if let (Some(org), Some(value)) = (&active, row_work) {
                    add(&self.db, org, &value)?;
                }
                let end: i64 =
                    self.db
                        .query_row("SELECT COUNT(*) FROM catalog_output", [], |r| r.get(0))?;
                if end > start {
                    continue;
                }
            }
            if columns.len() >= 2 {
                let mut active: BTreeMap<u32, String> = columns.into_iter().collect();
                let mut stmt=self.db.prepare("SELECT c.col_num,CASE WHEN c.kind IN ('s','inlineStr','str') THEN COALESCE(s.value,c.value,'') ELSE '' END FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.row_num>?2 AND c.col_num<=80 ORDER BY c.row_num,c.col_num")?;
                let mut rows = stmt.query(params![sheet, header_row])?;
                while let Some(row) = rows.next()? {
                    check_cancel(cancel)?;
                    let col: u32 = row.get(0)?;
                    if let Some(org) = active.get_mut(&col) {
                        let value: String = row.get(1)?;
                        if let Some(next) = organization(&value, names) {
                            *org = next.into();
                        } else {
                            add(&self.db, org, &value)?;
                        }
                    }
                }
                continue;
            }
            self.db.execute_batch("CREATE TEMP TABLE IF NOT EXISTS catalog_candidates(col INTEGER,seq INTEGER,organization TEXT,work TEXT);DELETE FROM catalog_candidates;")?;
            let mut active: BTreeMap<u32, String> = BTreeMap::new();
            let mut heads: BTreeMap<u32, u64> = BTreeMap::new();
            let mut counts: BTreeMap<u32, u64> = BTreeMap::new();
            let mut stmt=self.db.prepare("SELECT c.row_num,c.col_num,CASE WHEN c.kind IN ('s','inlineStr','str') THEN COALESCE(s.value,c.value,'') ELSE '' END FROM cells c LEFT JOIN strings s ON c.kind='s' AND s.id=CAST(c.value AS INTEGER) WHERE c.sheet=?1 AND c.col_num<=80 ORDER BY c.row_num,c.col_num")?;
            let mut rows = stmt.query([&sheet])?;
            let sheet_org = organization(&sheet_name, names);
            while let Some(row) = rows.next()? {
                check_cancel(cancel)?;
                let seq: u32 = row.get(0)?;
                let col: u32 = row.get(1)?;
                let value: String = row.get(2)?;
                if let Some(org) = organization(&value, names) {
                    active.insert(col, org.into());
                    *heads.entry(col).or_default() += 1;
                } else if work(&value) {
                    if let Some(org) = active.get(&col).map(String::as_str).or(sheet_org) {
                        self.db.execute(
                            "INSERT INTO catalog_candidates VALUES(?1,?2,?3,?4)",
                            params![col, seq, org, value.trim()],
                        )?;
                        *counts.entry(col).or_default() += 1;
                    }
                }
            }
            let best = counts
                .iter()
                .filter(|(col, count)| {
                    sheet_org.is_some() || (**count >= 2 && heads.contains_key(col))
                })
                .max_by_key(|(col, count)| (heads.get(col).copied().unwrap_or(0), **count));
            if let Some((col, _)) = best {
                let mut st = self.db.prepare(
                    "SELECT organization,work FROM catalog_candidates WHERE col=?1 ORDER BY seq",
                )?;
                let mut r = st.query([col])?;
                while let Some(row) = r.next()? {
                    add(
                        &self.db,
                        &row.get::<_, String>(0)?,
                        &row.get::<_, String>(1)?,
                    )?;
                }
            }
        }
        let count: u64 = self
            .db
            .query_row("SELECT COUNT(*) FROM catalog_output", [], |r| r.get(0))?;
        let mut stmt = self
            .db
            .prepare("SELECT organization,work FROM catalog_output ORDER BY org_key,work_key")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            check_cancel(cancel)?;
            accept(&row.get::<_, String>(0)?, &row.get::<_, String>(1)?)?;
        }
        Ok(count)
    }
}
