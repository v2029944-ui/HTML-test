//! Disk-backed SQL workspace shared by bounded analysis queries.
use super::*;

pub(in crate::storage) const CONFLICT_SQL:&str="INSERT INTO temp.an_issues SELECT a.report_id,a.org,CASE WHEN a.start=b.start AND a.end=b.end THEN 'duplicate' ELSE 'overlap' END,b.report_id FROM temp.an_candidates a JOIN temp.an_candidates b ON a.org=b.org AND b.start BETWEEN date(a.start,?1) AND a.end AND a.start<=b.end AND a.report_id<>b.report_id";

fn group_scope(c: &Connection, filter: Option<&str>) -> Result<(), String> {
    let hierarchy = tree_indexed(c)?;
    if !hierarchy {
        return c.execute_batch("CREATE TEMP VIEW an_scope_groups AS
            SELECT work,key,name,NULL selected FROM analysis_groups").map_err(err);
    }
    // Map a direct assignment to its highest selected ancestor. Each raw
    // work has one assignment, so overlaps never multiply report facts.
    c.execute_batch("CREATE TEMP TABLE an_scope_nodes(
        descendant TEXT PRIMARY KEY,key TEXT,name TEXT
    ) WITHOUT ROWID;
    CREATE TEMP TABLE an_scope_groups(
        work TEXT PRIMARY KEY,key TEXT,name TEXT,selected INTEGER NOT NULL
    ) WITHOUT ROWID;").map_err(err)?;
    c.execute("INSERT INTO an_scope_nodes
        SELECT descendant_key,key,name FROM (
            SELECT a.descendant_key,n.key,n.name,
                row_number() OVER(PARTITION BY a.descendant_key ORDER BY a.distance DESC) rank
            FROM analysis_group_ancestors a JOIN analysis_group_nodes n ON n.key=a.ancestor_key
            WHERE ?1 IS NULL OR EXISTS(
                SELECT 1 FROM analysis_filter_items f WHERE f.id=?1 AND f.item=n.name
            )
        ) WHERE rank=1", [filter]).map_err(err)?;
    c.execute("INSERT INTO an_scope_groups
        SELECT g.work,n.key,n.name,n.key IS NOT NULL OR EXISTS(
            SELECT 1 FROM analysis_filter_items f WHERE f.id=?1 AND f.item=g.work
            AND NOT EXISTS(SELECT 1 FROM analysis_group_names group_name WHERE group_name.name=f.item)
        ) FROM analysis_groups g LEFT JOIN an_scope_nodes n ON n.descendant=g.key",
        [filter]).map_err(err)?;
    // The exact-name filter gives a group priority over an equally named raw work.
    // Block the fallback match for an unassigned work with that name, too.
    c.execute("INSERT OR IGNORE INTO an_scope_groups
        SELECT f.item,NULL,NULL,0 FROM analysis_filter_items f
        JOIN analysis_group_names g ON g.name=f.item WHERE f.id=?1",
        [filter]).map_err(err)?;
    c.execute("CREATE TEMP TABLE an_scope_selections AS
        SELECT COALESCE(n.key,'work:'||f.item) key,f.item name,
            CASE WHEN n.key IS NULL THEN 'work' ELSE 'group' END kind,f.ordinal
        FROM analysis_filter_items f LEFT JOIN analysis_group_nodes n ON n.name=f.item
        LEFT JOIN an_scope_nodes scoped ON scoped.descendant=n.key
        LEFT JOIN an_scope_groups work ON work.work=f.item
        WHERE f.id=?1 AND ((n.key IS NULL AND work.key IS NULL) OR scoped.key=n.key)",
        [filter]).map_err(err)?;
    Ok(())
}
// Connection-local relational workspaces are deliberately on disk; no history Vec.
pub(super) fn workspace(c: &Connection, expected: i64, s: &AnalysisScope) -> Result<(), String> {
    check_revision(c, expected)?;
    check_metadata_bounds(c)?;
    if !REPORT_TYPES.contains(&s.report_type.as_str()) {
        return Err("Invalid analysis report type".into());
    }
    filter_check(c, &s.organizations, expected, "organizations")?;
    if let Some(id) = &s.names {
        filter_check(c, id, expected, "names")?;
    }
    let missing:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM reports r LEFT JOIN analysis_reports a ON a.report_id=r.id WHERE r.report_type=?1 AND a.report_id IS NULL)",[&s.report_type],|r|r.get(0)).map_err(err)?;
    if missing {
        return Err(
            "Analysis contains unindexed history; index every report of the selected type".into(),
        );
    }
    let settings: bool = c
        .query_row("SELECT EXISTS(SELECT 1 FROM analysis_settings)", [], |r| {
            r.get(0)
        })
        .map_err(err)?;
    if !settings {
        return Err("Analysis settings unindexed; backfill settings".into());
    }
    c.execute_batch("PRAGMA temp_store=FILE;PRAGMA temp.cache_size=-2048;").map_err(err)?;
    group_scope(c, s.names.as_deref())?;
    c.execute_batch("CREATE TEMP TABLE an_candidates(org TEXT,start TEXT,end TEXT,report_id TEXT,warned INTEGER,PRIMARY KEY(org,start,report_id)) WITHOUT ROWID;
 CREATE TEMP TABLE an_issues(report_id TEXT,org TEXT,kind TEXT,other_report_id TEXT,PRIMARY KEY(report_id,org,kind,other_report_id)) WITHOUT ROWID;
 CREATE TEMP TABLE an_valid(org TEXT,start TEXT,end TEXT,report_id TEXT,warned INTEGER,PRIMARY KEY(org,start)) WITHOUT ROWID;
 CREATE TEMP TABLE an_periods(slot INTEGER,start TEXT,PRIMARY KEY(slot,start)) WITHOUT ROWID;
 CREATE TEMP TABLE an_values(slot INTEGER,org TEXT,key TEXT,name TEXT,kind TEXT,count REAL,amount REAL,active INTEGER,known_amount REAL,paired_count REAL,source_work TEXT,report_rows INTEGER,priced_rows INTEGER,PRIMARY KEY(slot,org,key)) WITHOUT ROWID;
 CREATE TEMP TABLE an_missing(slot INTEGER,org TEXT,start TEXT,PRIMARY KEY(slot,org,start)) WITHOUT ROWID;
 CREATE TEMP TABLE an_combined(slot INTEGER,key TEXT,name TEXT,kind TEXT,count REAL,amount REAL,active INTEGER,known_amount REAL,paired_count REAL,source_work TEXT,report_rows INTEGER,priced_rows INTEGER,PRIMARY KEY(slot,key)) WITHOUT ROWID;").map_err(err)?;
    c.execute("INSERT INTO temp.an_candidates SELECT o.org,r.period_start,r.period_end,r.id,a.warned FROM reports r JOIN analysis_reports a ON a.report_id=r.id AND a.valid=1 JOIN analysis_orgs o ON o.report_id=r.id JOIN analysis_filter_items f ON f.id=?1 AND f.item=o.org WHERE r.report_type=?2",params![s.organizations,s.report_type]).map_err(err)?;
    c.execute("INSERT INTO temp.an_issues SELECT r.id,'','invalid','' FROM reports r JOIN analysis_reports a ON a.report_id=r.id WHERE r.report_type=?1 AND a.valid=0",[&s.report_type]).map_err(err)?;
    // A valid day/month can overlap only the same start; a seven-day window
    // can start at most six days earlier. Constrain BOTH B-tree range ends.
    c.execute(
        CONFLICT_SQL,
        [if s.report_type == "week" {
            "-6 day"
        } else {
            "0 day"
        }],
    )
    .map_err(err)?;
    c.execute("INSERT INTO temp.an_valid SELECT * FROM temp.an_candidates a WHERE NOT EXISTS(SELECT 1 FROM temp.an_issues i WHERE i.report_id=a.report_id AND i.org=a.org)",[]).map_err(err)?;
    Ok(())
}
// Limit the report IDs before reading report_rows. A report can cover several
// organizations, so look up its rows once and then match each eligible org.
const RANGE_VALUES_SQL: &str = "
    INSERT INTO temp.an_values
    SELECT ?1,v.org,COALESCE(g.key,'work:'||x.work),COALESCE(g.name,x.work),
           CASE WHEN g.key IS NULL THEN 'work' ELSE 'group' END,
           SUM(x.count),CASE WHEN COUNT(x.amount)=COUNT(*) THEN SUM(x.amount) END,
           MAX(x.count<>0 OR x.amount IS NULL OR x.amount<>0),
           SUM(x.amount),SUM(x.count) FILTER (WHERE x.amount IS NOT NULL),
           CASE WHEN MIN(x.work) FILTER (WHERE x.amount IS NOT NULL)
                     =MAX(x.work) FILTER (WHERE x.amount IS NOT NULL)
                THEN MIN(x.work) FILTER (WHERE x.amount IS NOT NULL) END,
           COUNT(*),COUNT(x.amount)
    FROM (
        SELECT DISTINCT v.report_id,v.start
        FROM temp.an_valid v
        JOIN temp.an_periods p ON p.slot=?1 AND p.start=v.start
    ) selected
    CROSS JOIN report_rows x ON x.report_id=selected.report_id
    CROSS JOIN temp.an_valid v ON v.org=x.organization AND v.start=selected.start
                               AND v.report_id=selected.report_id
    LEFT JOIN an_scope_groups g ON g.work=x.work
    WHERE (?2 IS NULL OR COALESCE(g.selected, EXISTS (
        SELECT 1 FROM analysis_filter_items f
        WHERE f.id=?2 AND f.item=COALESCE(g.name,x.work)
    )))
    GROUP BY v.org,COALESCE(g.key,'work:'||x.work)";

pub(super) fn fill_periods(c: &Connection, s: &AnalysisScope, q: &RangeQuery, slot: i64) -> Result<(), String> {
    let (mut d, b) = range_bounds(&s.report_type, q)?;
    let mut insert = c
        .prepare("INSERT INTO temp.an_periods VALUES(?1,?2)")
        .map_err(err)?;
    loop {
        insert.execute(params![slot, d.to_string()]).map_err(err)?;
        if d == b {
            break;
        }
        d = shift(&s.report_type, d, 1)?;
    }
    drop(insert);
    c.execute("INSERT INTO temp.an_missing SELECT ?1,f.item,p.start FROM analysis_filter_items f CROSS JOIN temp.an_periods p LEFT JOIN temp.an_valid v ON v.org=f.item AND v.start=p.start WHERE f.id=?2 AND p.slot=?1 AND v.report_id IS NULL",params![slot,s.organizations]).map_err(err)?;
    Ok(())
}

pub(super) fn fill_range(c: &Connection, s: &AnalysisScope, q: &RangeQuery, slot: i64) -> Result<(), String> {
    fill_periods(c, s, q, slot)?;
    fill_range_values(c, s, slot)
}

pub(super) fn fill_range_values(c: &Connection, s: &AnalysisScope, slot: i64) -> Result<(), String> {
    c.execute(RANGE_VALUES_SQL, params![slot, s.names]).map_err(err)?;
    c.execute("INSERT INTO temp.an_combined SELECT slot,key,name,kind,SUM(count),CASE WHEN COUNT(amount)=COUNT(*) THEN SUM(amount) END,MAX(active),SUM(known_amount),SUM(paired_count),CASE WHEN COUNT(source_work)=COUNT(paired_count) AND MIN(source_work)=MAX(source_work) THEN MIN(source_work) END,SUM(report_rows),SUM(priced_rows) FROM temp.an_values WHERE slot=?1 GROUP BY key",[slot]).map_err(err)?;
    validate_numbers(c, "SELECT count,amount FROM temp.an_combined")?;
    validate_numbers(c, "SELECT count,amount FROM temp.an_values")?;
    validate_numbers(
        c,
        "SELECT SUM(count),SUM(amount) FROM temp.an_values GROUP BY slot,org",
    )?;
    validate_numbers(
        c,
        "SELECT SUM(count),SUM(amount) FROM temp.an_combined GROUP BY slot",
    )?;
    Ok(())
}
pub(super) fn validate_numbers(c: &Connection, sql: &str) -> Result<(), String> {
    let mut stmt = c.prepare(sql).map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    while let Some(r) = rows.next().map_err(err)? {
        finite(r.get(0).map_err(err)?)?;
        r.get::<_, Option<f64>>(1).map_err(err)?.map(finite).transpose()?;
    }
    Ok(())
}
pub(super) fn count_sql(c: &Connection, sql: &str) -> Result<i64, String> {
    c.query_row(sql, [], |r| r.get(0)).map_err(err)
}
pub(super) fn selected_organization_count(c: &Connection, s: &AnalysisScope) -> Result<i64, String> {
    c.query_row("SELECT COUNT(*) FROM analysis_filter_items WHERE id=?1",
        [&s.organizations], |r| r.get(0)).map_err(err)
}
pub(super) fn observed_reports(c: &Connection, slot: i64, org: Option<&str>) -> Result<bool, String> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM temp.an_valid v
        JOIN temp.an_periods p ON p.slot=?1 AND p.start=v.start WHERE ?2 IS NULL OR v.org=?2)",
        params![slot,org], |r| r.get(0)).map_err(err)
}
pub(super) fn complete(c: &Connection, slot: i64, org: Option<&str>) -> Result<bool, String> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM temp.an_periods WHERE slot=?1) AND NOT EXISTS(SELECT 1 FROM temp.an_missing WHERE slot=?1 AND (?2 IS NULL OR org=?2)) AND EXISTS(SELECT 1 FROM temp.an_valid WHERE ?2 IS NULL OR org=?2)",params![slot,org],|r|r.get(0)).map_err(err)
}

pub(super) fn tree_indexed(c: &Connection) -> Result<bool, String> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM analysis_group_nodes)", [], |r| r.get(0)).map_err(err)
}

// The catalog includes assigned works even if they are absent from the
// reporting history. In analytical series they appear only below their group.
pub(super) fn create_tree_catalog(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TEMP TABLE an_tree_catalog(
        key TEXT PRIMARY KEY,name TEXT,kind TEXT,parent_key TEXT,work TEXT
    ) WITHOUT ROWID;
    INSERT INTO an_tree_catalog
        SELECT key,name,'group',parent_key,NULL FROM analysis_group_nodes;
    INSERT INTO an_tree_catalog SELECT 'work:'||w.work,w.work,'work',g.key,w.work
        FROM (SELECT work FROM report_rows UNION SELECT work FROM analysis_groups) w
        LEFT JOIN analysis_groups g ON g.work=w.work WHERE w.work<>'';").map_err(err)
}

pub(super) fn create_name_catalog(tx: &Connection, all_series: bool) -> Result<bool, String> {
    if tree_indexed(tx)? {
        create_tree_catalog(tx)?;
        if all_series {
            tx.execute_batch("CREATE TEMP TABLE unified_names AS
                SELECT key,name,kind,CASE WHEN COUNT(*) OVER(PARTITION BY name)>1
                    THEN name || CASE kind WHEN 'group' THEN ' (группа)' ELSE ' (работа)' END
                    ELSE name END label FROM an_tree_catalog WHERE parent_key IS NULL")
                .map_err(err)?;
            return Ok(false);
        }
        tx.execute_batch("CREATE TEMP TABLE unified_names AS
            SELECT key,name,kind,name label,COUNT(*) OVER(PARTITION BY name,kind) copies FROM an_tree_catalog")
            .map_err(err)?;
        return tx.execute("DELETE FROM unified_names WHERE copies>1 OR length(CAST(name AS BLOB))>512
            OR (kind='work' AND EXISTS(SELECT 1 FROM analysis_group_names g WHERE g.name=unified_names.name))", [])
            .map(|count| count > 0).map_err(err);
    }

    // Name-based selection must be unambiguous. The all-work chart uses identity
    // keys and can safely display both members of a matching work/group pair.
    let labels = if all_series {
        "SELECT key,name,kind,
                CASE WHEN count(*) OVER(PARTITION BY name)>1
                     THEN name || CASE kind WHEN 'group' THEN ' (группа)' ELSE ' (работа)' END
                     ELSE name END label FROM candidates WHERE name<>''"
    } else {
        "SELECT key,name,min(kind) kind,name label,count(*) copies FROM candidates
         WHERE name<>'' GROUP BY name"
    };
    tx.execute_batch(&format!("CREATE TEMP TABLE unified_names AS WITH candidates AS (
            SELECT g.key,g.name,'group' kind FROM analysis_groups g GROUP BY g.key,g.name
            UNION ALL SELECT 'work:'||x.work,x.work,'work' FROM report_rows x
            WHERE NOT EXISTS(SELECT 1 FROM analysis_groups g WHERE g.work=x.work) GROUP BY x.work
        ) {labels}")).map_err(err)?;
    if all_series { return Ok(false); }
    let omitted = tx.execute(
        "DELETE FROM unified_names WHERE copies>1 OR length(CAST(name AS BLOB))>512", []
    ).map_err(err)?;
    Ok(omitted > 0)
}
