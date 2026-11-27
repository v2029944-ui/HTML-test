//! Index publication and revision-bound analysis filters.
use super::*;
use crate::storage::work_group_hierarchy::{validate_group_parents, GroupParent, GROUP_PARENTS_FIELD};
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::io::{BufReader, Read};

pub(in crate::storage) fn initialize(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS analysis_reports(report_id TEXT PRIMARY KEY, valid INTEGER NOT NULL, issue TEXT, warned INTEGER NOT NULL);
 CREATE TABLE IF NOT EXISTS analysis_orgs(report_id TEXT NOT NULL,org TEXT NOT NULL,PRIMARY KEY(report_id,org)) WITHOUT ROWID;
 CREATE INDEX IF NOT EXISTS analysis_org_lookup ON analysis_orgs(org,report_id);
 CREATE TABLE IF NOT EXISTS analysis_settings(singleton INTEGER PRIMARY KEY CHECK(singleton=1));
 CREATE TABLE IF NOT EXISTS analysis_group_names(name TEXT PRIMARY KEY,key TEXT) WITHOUT ROWID;
 CREATE TABLE IF NOT EXISTS analysis_groups(work TEXT PRIMARY KEY,key TEXT NOT NULL,name TEXT NOT NULL) WITHOUT ROWID;
 CREATE TABLE IF NOT EXISTS analysis_group_nodes(key TEXT PRIMARY KEY,name TEXT NOT NULL,parent_key TEXT) WITHOUT ROWID;
 CREATE INDEX IF NOT EXISTS analysis_group_nodes_by_parent ON analysis_group_nodes(parent_key);
 CREATE INDEX IF NOT EXISTS analysis_group_works_by_key ON analysis_groups(key);
 CREATE TABLE IF NOT EXISTS analysis_group_ancestors(descendant_key TEXT,ancestor_key TEXT,distance INTEGER NOT NULL,PRIMARY KEY(descendant_key,ancestor_key)) WITHOUT ROWID;
 CREATE INDEX IF NOT EXISTS analysis_group_descendants ON analysis_group_ancestors(ancestor_key,descendant_key);
 CREATE TRIGGER IF NOT EXISTS analysis_group_index_settings_insert AFTER INSERT ON kv WHEN NEW.name='settings' BEGIN
 DELETE FROM analysis_group_nodes; DELETE FROM analysis_group_ancestors; END;
 CREATE TRIGGER IF NOT EXISTS analysis_group_index_settings_update AFTER UPDATE ON kv WHEN OLD.name='settings' OR NEW.name='settings' BEGIN
 DELETE FROM analysis_group_nodes; DELETE FROM analysis_group_ancestors; END;
 CREATE TRIGGER IF NOT EXISTS analysis_group_index_settings_delete AFTER DELETE ON kv WHEN OLD.name='settings' BEGIN
 DELETE FROM analysis_group_nodes; DELETE FROM analysis_group_ancestors; END;
 CREATE TABLE IF NOT EXISTS analysis_filters(id TEXT PRIMARY KEY,revision INTEGER NOT NULL,kind TEXT NOT NULL,sealed INTEGER NOT NULL);
 CREATE TABLE IF NOT EXISTS analysis_filter_items(id TEXT NOT NULL,item TEXT NOT NULL,ordinal INTEGER NOT NULL,PRIMARY KEY(id,item)) WITHOUT ROWID;
 CREATE TRIGGER IF NOT EXISTS analysis_report_insert AFTER INSERT ON reports BEGIN
 DELETE FROM analysis_reports WHERE report_id=NEW.id;DELETE FROM analysis_orgs WHERE report_id=NEW.id;END;
 CREATE TRIGGER IF NOT EXISTS analysis_report_update AFTER UPDATE ON reports BEGIN
 DELETE FROM analysis_reports WHERE report_id IN(OLD.id,NEW.id);DELETE FROM analysis_orgs WHERE report_id IN(OLD.id,NEW.id);END;
 CREATE TRIGGER IF NOT EXISTS analysis_report_delete AFTER DELETE ON reports BEGIN
 DELETE FROM analysis_reports WHERE report_id=OLD.id;DELETE FROM analysis_orgs WHERE report_id=OLD.id;END;
 CREATE TRIGGER IF NOT EXISTS analysis_settings_insert AFTER INSERT ON kv WHEN NEW.name='settings' BEGIN DELETE FROM analysis_settings;DELETE FROM analysis_groups;END;
 CREATE TRIGGER IF NOT EXISTS analysis_settings_update AFTER UPDATE ON kv WHEN OLD.name='settings' OR NEW.name='settings' BEGIN DELETE FROM analysis_settings;DELETE FROM analysis_groups;END;
 CREATE TRIGGER IF NOT EXISTS analysis_settings_delete AFTER DELETE ON kv WHEN OLD.name='settings' BEGIN DELETE FROM analysis_settings;DELETE FROM analysis_groups;END;").map_err(err)
}

#[derive(Debug, Serialize)]
pub struct AnalysisNameOption {
    pub name: String,
    pub kind: String,
}
#[derive(Debug, Serialize)]
pub struct AnalysisNameOptions {
    pub rows: Vec<AnalysisNameOption>,
    pub has_more: bool,
}

// Scalars only: unknown composite fields are consumed without allocating a Value.
#[derive(Default)]
enum Scalar {
    Text(String),
    Num(f64),
    Bool(bool),
    #[default]
    Other,
    Null,
    Composite,
}
impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Scalar;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON value")
            }
            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Scalar, E> {
                Ok(Scalar::Text(s.into()))
            }
            fn visit_string<E: serde::de::Error>(self, s: String) -> Result<Scalar, E> {
                Ok(Scalar::Text(s))
            }
            fn visit_f64<E: serde::de::Error>(self, n: f64) -> Result<Scalar, E> {
                Ok(Scalar::Num(n))
            }
            fn visit_i64<E: serde::de::Error>(self, n: i64) -> Result<Scalar, E> {
                Ok(Scalar::Num(n as f64))
            }
            fn visit_u64<E: serde::de::Error>(self, n: u64) -> Result<Scalar, E> {
                Ok(Scalar::Num(n as f64))
            }
            fn visit_bool<E: serde::de::Error>(self, b: bool) -> Result<Scalar, E> {
                Ok(Scalar::Bool(b))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Scalar, E> {
                Ok(Scalar::Null)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Scalar, A::Error> {
                while a.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Scalar::Composite)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Scalar, A::Error> {
                while a.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Scalar::Composite)
            }
        }
        d.deserialize_any(V)
    }
}
impl Scalar {
    fn text(self) -> Option<String> {
        if let Self::Text(s) = self {
            Some(s)
        } else {
            None
        }
    }
    fn number(self) -> Option<f64> {
        if let Self::Num(n) = self {
            Some(n)
        } else {
            None
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Text(s) => !s.is_empty(),
            Self::Num(n) => *n != 0.,
            Self::Composite => true,
            _ => false,
        }
    }
}
#[derive(Default)]
struct ParsedRow {
    org: Option<String>,
    work: Option<String>,
    count: Option<f64>,
    amount: Option<Option<f64>>,
}
impl<'de> Deserialize<'de> for ParsedRow {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = ParsedRow;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("report row")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<ParsedRow, A::Error> {
                let mut r = ParsedRow::default();
                let mut seen = [false; 4];
                while let Some(k) = a.next_key::<String>()? {
                    if let Some(i) = ["organization", "work", "count", "amount"]
                        .iter()
                        .position(|x| *x == k)
                    {
                        if seen[i] {
                            return Err(serde::de::Error::custom("Duplicate row field"));
                        }
                        seen[i] = true;
                        let v = a.next_value::<Scalar>()?;
                        match i {
                            0 => r.org = v.text(),
                            1 => r.work = v.text(),
                            2 => r.count = v.number(),
                            _ => r.amount = match v { Scalar::Null => Some(None), _ => v.number().map(Some) },
                        }
                    } else {
                        a.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(r)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<ParsedRow, E> {
                Ok(ParsedRow::default())
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<ParsedRow, E> {
                Ok(ParsedRow::default())
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<ParsedRow, E> {
                Ok(ParsedRow::default())
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<ParsedRow, E> {
                Ok(ParsedRow::default())
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<ParsedRow, E> {
                Ok(ParsedRow::default())
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<ParsedRow, E> {
                Ok(ParsedRow::default())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<ParsedRow, A::Error> {
                while a.next_element::<IgnoredAny>()?.is_some() {}
                Ok(ParsedRow::default())
            }
        }
        d.deserialize_any(V)
    }
}
struct Rows<'a>(&'a Connection);
impl<'de> DeserializeSeed<'de> for Rows<'_> {
    type Value = bool;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<bool, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Rows<'_> {
    type Value = bool;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("rows array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<bool, A::Error> {
        use serde::de::Error;
        let mut n = 0i64;
        let mut valid = true;
        let mut stmt = self
            .0
            .prepare("INSERT INTO temp.an_row VALUES(?1,?2,?3,?4,?5)")
            .map_err(A::Error::custom)?;
        while let Some(r) = a.next_element::<ParsedRow>()? {
            match (r.work, r.count, r.amount) {
                (Some(w), Some(c), Some(m)) if c.is_finite() && m.is_none_or(f64::is_finite) => {
                    if let Some(o) = r.org {
                        bounded(&w)
                            .and_then(|_| bounded(&o))
                            .map_err(A::Error::custom)?;
                        self.0
                            .execute("INSERT OR IGNORE INTO temp.an_org VALUES(?1)", [&o])
                            .map_err(A::Error::custom)?;
                        stmt.execute(params![n, o, w, c, m])
                            .map_err(A::Error::custom)?;
                    } else {
                        // Old JS ignores these rows for string organization selections.
                        // Do not certify the reduced rows as a complete D1 raw index.
                        self.0
                            .execute("INSERT OR IGNORE INTO temp.an_omitted VALUES(1)", [])
                            .map_err(A::Error::custom)?;
                    }
                }
                _ => valid = false,
            }
            n = n
                .checked_add(1)
                .ok_or_else(|| A::Error::custom("Too many rows"))?;
        }
        Ok(valid)
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<bool, E> {
        Ok(false)
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<bool, E> {
        Ok(false)
    }
    fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<bool, E> {
        Ok(false)
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<bool, E> {
        Ok(false)
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<bool, E> {
        Ok(false)
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<bool, E> {
        Ok(false)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<bool, A::Error> {
        while a.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(false)
    }
}
struct Orgs<'a>(&'a Connection);
impl<'de> DeserializeSeed<'de> for Orgs<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Orgs<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("organization array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        use serde::de::Error;
        while let Some(v) = a.next_element::<Scalar>()? {
            if let Some(s) = v.text() {
                bounded(&s).map_err(A::Error::custom)?;
                self.0
                    .execute("INSERT OR IGNORE INTO temp.an_org VALUES(?1)", [s])
                    .map_err(A::Error::custom)?;
            }
        }
        Ok(())
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
        Ok(())
    }
}
struct Warnings;
impl<'de> DeserializeSeed<'de> for Warnings {
    type Value = bool;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<bool, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Warnings {
    type Value = bool;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("warning array or null")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<bool, A::Error> {
        let mut any = false;
        while a.next_element::<IgnoredAny>()?.is_some() {
            any = true
        }
        Ok(any)
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<bool, E> {
        Ok(false)
    }
    fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<bool, E> {
        Ok(!s.is_empty())
    }
}
struct Report<'a> {
    c: &'a Connection,
    metadata: [&'a str; 4],
}
impl<'de> DeserializeSeed<'de> for Report<'_> {
    type Value = (bool, bool);
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(bool, bool), D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for Report<'_> {
    type Value = (bool, bool);
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("report object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<(bool, bool), A::Error> {
        use serde::de::Error;
        let mut seen = [false; 8];
        let mut valid = false;
        let mut warned = false;
        while let Some(k) = a.next_key::<String>()? {
            if let Some(i) = [
                "id",
                "type",
                "start",
                "end",
                "rows",
                "organizations",
                "warnings",
                "hasMismatch",
            ]
            .iter()
            .position(|x| *x == k)
            {
                if seen[i] {
                    return Err(A::Error::custom("Duplicate report field"));
                }
                seen[i] = true;
                match i {
                    0..=3 => {
                        let s = a.next_value::<String>()?;
                        if s != self.metadata[i] {
                            return Err(A::Error::custom(
                                "JSON metadata differs from columns",
                            ));
                        }
                    }
                    4 => valid = a.next_value_seed(Rows(self.c))?,
                    5 => a.next_value_seed(Orgs(self.c))?,
                    6 => warned |= a.next_value_seed(Warnings)?,
                    _ => warned |= a.next_value::<Scalar>()?.truthy(),
                }
            } else {
                a.next_value::<IgnoredAny>()?;
            }
        }
        if seen[..4].iter().any(|x| !*x) {
            return Err(A::Error::custom("Missing report metadata"));
        }
        Ok((valid, warned))
    }
}
fn parse<R: Read, S, T>(reader: R, seed: S) -> Result<T, String>
where
    S: for<'de> DeserializeSeed<'de, Value = T>,
{
    let bounded = crate::storage::read_model::TokenBounded::new(reader);
    let mut de = serde_json::Deserializer::from_reader(BufReader::with_capacity(8192, bounded));
    let result = seed.deserialize(&mut de).map_err(err)?;
    de.end().map_err(err)?;
    Ok(result)
}
// Group work lists stage on disk even when they precede id/name in JSON.
struct Works<'a> {
    c: &'a Connection,
    n: i64,
}
impl<'de> DeserializeSeed<'de> for Works<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for Works<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("group work array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        use serde::de::Error;
        while let Some(s) = a.next_element::<String>()? {
            bounded(&s).map_err(A::Error::custom)?;
            self
                .c
                .execute(
                    "INSERT OR IGNORE INTO temp.an_members VALUES(?1,?2)",
                    params![self.n, s],
                )
                .map_err(A::Error::custom)?;
        }
        Ok(())
    }
}
struct Group<'a> {
    c: &'a Connection,
    n: i64,
}
impl<'de> DeserializeSeed<'de> for Group<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for Group<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("group object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        use serde::de::Error;
        let mut id = None;
        let mut name = None;
        let mut works = false;
        while let Some(k) = a.next_key::<String>()? {
            match k.as_str() {
                "id" if id.is_none() => id = Some(a.next_value::<String>()?),
                "name" if name.is_none() => name = Some(a.next_value::<String>()?),
                "works" if !works => {
                    a.next_value_seed(Works {
                        c: self.c,
                        n: self.n,
                    })?;
                    works = true
                }
                "id" | "name" | "works" => {
                    return Err(A::Error::custom("Duplicate group field"))
                }
                _ => {
                    a.next_value::<IgnoredAny>()?;
                }
            }
        }
        let id = id.ok_or_else(|| A::Error::custom("Missing group id"))?;
        let name = name.ok_or_else(|| A::Error::custom("Missing group name"))?;
        bounded(&id)
            .and_then(|_| bounded(&name))
            .map_err(A::Error::custom)?;
        if !works {
            return Err(A::Error::custom("Missing group works"));
        }
        self
            .c
            .execute(
                "INSERT INTO temp.an_descriptors VALUES(?1,?2,?3)",
                params![self.n, id, name],
            )
            .map_err(A::Error::custom)?;
        Ok(())
    }
}
struct Groups<'a>(&'a Connection);
impl<'de> DeserializeSeed<'de> for Groups<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for Groups<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("groups array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        use serde::de::Error;
        let mut n = 0i64;
        while a.next_element_seed(Group { c: self.0, n })?.is_some() {
            n = n
                .checked_add(1)
                .ok_or_else(|| A::Error::custom("Too many groups"))?;
        }
        Ok(())
    }
}
struct Settings<'a>(&'a Connection);
impl<'de> DeserializeSeed<'de> for Settings<'_> {
    type Value = Option<Vec<GroupParent>>;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for Settings<'_> {
    type Value = Option<Vec<GroupParent>>;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("settings object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
        let mut seen = false;
        let mut parents = None;
        while let Some(k) = a.next_key::<String>()? {
            match k.as_str() {
                "groups" => {
                    if seen {
                        return Err(serde::de::Error::custom("Duplicate groups field"));
                    }
                    seen = true;
                    a.next_value_seed(Groups(self.0))?;
                }
                GROUP_PARENTS_FIELD => {
                    if parents.is_some() {
                        return Err(serde::de::Error::custom("Duplicate work group hierarchy field"));
                    }
                    parents = Some(a.next_value::<Vec<GroupParent>>()?);
                }
                _ => { a.next_value::<IgnoredAny>()?; }
            }
        }
        if !seen {
            return Err(serde::de::Error::custom("Missing settings groups"));
        }
        Ok(parents)
    }
}

fn publish_group_hierarchy(c: &Connection, parents: Option<Vec<GroupParent>>) -> Result<(), String> {
    c.execute_batch("DELETE FROM analysis_group_nodes; DELETE FROM analysis_group_ancestors;").map_err(err)?;
    let Some(parents) = parents else { return Ok(()); };
    let mut ids = c.prepare("SELECT id FROM temp.an_descriptors LIMIT 10001").map_err(err)?;
    let ids = ids.query_map([], |row| row.get::<_, String>(0)).map_err(err)?
        .collect::<Result<Vec<_>,_>>().map_err(err)?;
    if ids.len() > 10_000 || parents.len() > ids.len() {
        return Err("Превышен лимит групп или связей дерева.".into());
    }
    validate_group_parents(ids.iter().map(String::as_str), &parents)?;
    let conflicting_assignments: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM temp.an_members GROUP BY work HAVING COUNT(*) > 1)",
        [], |row| row.get(0),
    ).map_err(err)?;
    if conflicting_assignments {
        return Err("В дереве работа может входить только в одну группу напрямую.".into());
    }
    c.execute_batch(
        "INSERT INTO analysis_group_nodes(key,name,parent_key)
         SELECT 'group:'||id,name,NULL FROM temp.an_descriptors;",
    ).map_err(err)?;
    let mut link = c.prepare(
        "UPDATE analysis_group_nodes SET parent_key='group:'||?2 WHERE key='group:'||?1",
    ).map_err(err)?;
    for parent in parents {
        link.execute(params![parent.child,parent.parent]).map_err(err)?;
    }
    c.execute_batch(
        "WITH RECURSIVE ancestors(descendant_key,ancestor_key,distance) AS (
             SELECT key,key,0 FROM analysis_group_nodes
             UNION ALL
             SELECT a.descendant_key,n.parent_key,a.distance+1
             FROM ancestors a JOIN analysis_group_nodes n ON n.key=a.ancestor_key
             WHERE n.parent_key IS NOT NULL
         ) INSERT INTO analysis_group_ancestors SELECT * FROM ancestors;",
    ).map_err(err)
}

impl Storage {
    /// One report per call. Parsing stages in FILE temp tables, publishing by revision CAS.
    pub fn analysis_index_report(&self, id: &str, expected: i64) -> Result<(), String> {
        self.analysis_index_report_before_publish(id, expected, || {})
    }
    /// Startup needs to know whether the same streamed report is eligible for
    /// a complete raw index. Invalid period bounds remain quality findings,
    /// while otherwise valid rows can still be indexed for bounded browsing.
    pub fn analysis_index_report_for_startup(&self,id:&str,expected:i64)->Result<bool,String> {
        self.analysis_index_report_with_raw_eligibility(id,expected,||{})
    }
    pub(in crate::storage) fn analysis_index_report_before_publish(
        &self,
        id: &str,
        expected: i64,
        before: impl FnOnce(),
    ) -> Result<(), String> {
        self.analysis_index_report_with_raw_eligibility(id,expected,before).map(|_|())
    }
    fn analysis_index_report_with_raw_eligibility(
        &self,id:&str,expected:i64,before:impl FnOnce(),
    )->Result<bool,String> {
        bounded(id)?;
        let mut c = self.connect()?;
        c.execute_batch("PRAGMA temp_store=FILE;PRAGMA temp.cache_size=-2048;CREATE TEMP TABLE an_row(n INTEGER PRIMARY KEY,org TEXT,work TEXT,count REAL,amount REAL);CREATE TEMP TABLE an_org(org TEXT PRIMARY KEY) WITHOUT ROWID;CREATE TEMP TABLE an_omitted(singleton INTEGER PRIMARY KEY);").map_err(err)?;
        let tx = c.transaction().map_err(err)?;
        check_revision(&tx, expected)?;
        check_metadata_bounds(&tx)?;
        let (rowid, kind, start, stop): (i64, String, String, String) = tx
            .query_row(
                "SELECT rowid,report_type,period_start,period_end FROM reports WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(err)?
            .ok_or("Report not found")?;
        if !REPORT_TYPES.contains(&kind.as_str()) {
            return Err("Unsupported report type".into());
        }
        let (rows_valid, warned) = parse(
            tx.blob_open("main", "reports", "json", rowid, true)
                .map_err(err)?,
            Report {
                c: &tx,
                metadata: [id, &kind, &start, &stop],
            },
        )?;
        let valid = valid_bounds(&kind, &start, &stop) && rows_valid;
        let raw_eligible=rows_valid&&!tx.query_row("SELECT EXISTS(SELECT 1 FROM temp.an_omitted)",[],|r|r.get::<_,bool>(0)).map_err(err)?;
        before();
        check_revision(&tx, expected)?;
        tx.execute("DELETE FROM analysis_reports WHERE report_id=?1", [id])
            .map_err(err)?;
        tx.execute("DELETE FROM analysis_orgs WHERE report_id=?1", [id])
            .map_err(err)?;
        tx.execute(
            "INSERT INTO analysis_orgs SELECT ?1,org FROM temp.an_org",
            [id],
        )
        .map_err(err)?;
        if valid {
            tx.execute("DELETE FROM report_rows WHERE report_id=?1", [id])
                .map_err(err)?;
            tx.execute(
                "INSERT INTO report_rows SELECT ?1,n,org,work,count,amount FROM temp.an_row",
                [id],
            )
            .map_err(err)?;
            tx.execute("DELETE FROM report_row_index WHERE report_id=?1", [id])
                .map_err(err)?;
            tx.execute("INSERT INTO report_row_index SELECT ?1,(SELECT COUNT(*) FROM temp.an_row) WHERE NOT EXISTS(SELECT 1 FROM temp.an_omitted)",[id]).map_err(err)?;
        }
        tx.execute(
            "INSERT INTO analysis_reports VALUES(?1,?2,?3,?4)",
            params![
                id,
                valid,
                if valid {
                    None
                } else if !rows_valid {
                    Some("invalid_rows")
                } else {
                    Some("invalid_bounds")
                },
                warned
            ],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(raw_eligible)
    }
    pub fn analysis_index_settings(&self, expected: i64) -> Result<(), String> {
        let mut c = self.connect()?;
        c.execute_batch("PRAGMA temp_store=FILE;PRAGMA temp.cache_size=-2048;CREATE TEMP TABLE an_members(n INTEGER,work TEXT,PRIMARY KEY(n,work)) WITHOUT ROWID;CREATE TEMP TABLE an_descriptors(n INTEGER PRIMARY KEY,id TEXT,name TEXT);").map_err(err)?;
        let tx = c.transaction().map_err(err)?;
        check_revision(&tx, expected)?;
        let rowid: Option<i64> = tx
            .query_row("SELECT rowid FROM kv WHERE name='settings'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)?;
        let parents = if let Some(rowid) = rowid {
            parse(
                tx.blob_open("main", "kv", "json", rowid, true)
                    .map_err(err)?,
                Settings(&tx),
            )?
        } else {
            None
        };
        publish_group_hierarchy(&tx, parents)?;
        let ambiguous:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM temp.an_descriptors GROUP BY id HAVING COUNT(DISTINCT name)>1)",[],|r|r.get(0)).map_err(err)?;
        if ambiguous {
            return Err(
                "Unsupported settings: duplicate group id with different display names".into(),
            );
        }
        tx.execute("DELETE FROM analysis_groups", []).map_err(err)?;
        tx.execute_batch("DELETE FROM analysis_group_names;INSERT OR IGNORE INTO analysis_group_names SELECT name,'group:'||id FROM temp.an_descriptors ORDER BY n;INSERT OR IGNORE INTO analysis_groups SELECT m.work,'group:'||d.id,d.name FROM temp.an_members m JOIN temp.an_descriptors d ON d.n=m.n ORDER BY m.n;INSERT OR REPLACE INTO analysis_settings VALUES(1);").map_err(err)?;
        tx.commit().map_err(err)
    }
    /// Discover bounded exact display names. Colliding labels are hidden
    /// because the legacy analysis names-filter cannot tell them apart.
    /// The work scan uses the existing work index, never historical JSON.
    pub fn analysis_name_options(
        &self,
        expected: i64,
        search: &str,
        limit: usize,
    ) -> Result<AnalysisNameOptions, String> {
        if search.len()>128 {
            return Err("Work/group search must be at most 128 bytes".into());
        }
        if !(1..=80).contains(&limit) {
            return Err("Work/group options must contain 1..=80 names".into());
        }
        let c=self.connect()?;
        check_revision(&c,expected)?;
        check_metadata_bounds(&c)?;
        let indexed:bool=c.query_row(
            "SELECT EXISTS(SELECT 1 FROM analysis_settings)",[],|r|r.get(0)
        ).map_err(err)?;
        if !indexed { return Err("Analysis groups are not indexed".into()); }
        // A grouped work is represented only by its group display name.
        // One group may have many works: collapse by stable group key first.
        // Ambiguous equal labels spanning different identities are omitted.
        let mut stmt=c.prepare(
            "WITH candidates AS (
               SELECT g.name AS name,'group' AS kind
               FROM analysis_groups g GROUP BY g.key,g.name
               UNION ALL
               SELECT x.work AS name,'work' AS kind
               FROM report_rows x
               WHERE NOT EXISTS(
                  SELECT 1 FROM analysis_groups g WHERE g.work=x.work
               ) GROUP BY x.work
            )
            SELECT name,MIN(kind) FROM candidates
            WHERE name<>'' AND length(CAST(name AS BLOB))<=512
              AND (?1='' OR instr(name,?1)>0)
            GROUP BY name HAVING COUNT(*)=1
            ORDER BY name COLLATE BINARY
            LIMIT ?2"
        ).map_err(err)?;
        let mut rows=stmt.query(params![search,(limit+1) as i64]).map_err(err)?;
        let mut out=Vec::with_capacity(limit+1);
        while let Some(row)=rows.next().map_err(err)? {
            if out.len()>limit {return Err("Work/group options exceed result bound".into());}
            out.push(AnalysisNameOption {
                name:row.get(0).map_err(err)?,
                kind:row.get(1).map_err(err)?,
            });
        }
        let has_more=out.len()>limit;
        if has_more {out.pop();}
        Ok(AnalysisNameOptions{rows:out,has_more})
    }
    pub fn analysis_filter_create(&self, expected: i64, kind: &str) -> Result<String, String> {
        if !matches!(kind, "organizations" | "names") {
            return Err("Unknown analysis filter kind".into());
        }
        let _filter_writer = self.analysis_filter_gate.lock().map_err(err)?;
        let started = std::time::Instant::now();
        let stage_error = |stage: &str, detail: String| format!("{detail} [analysis filter {stage}; elapsed {} ms]", started.elapsed().as_millis());
        let sql_error = |stage: &str, failure: rusqlite::Error| {
            let code = match &failure { rusqlite::Error::SqliteFailure(code, _) => code.extended_code, _ => 0 };
            stage_error(stage, format!("{} [SQLite {code}]", failure))
        };
        let mut c = self.connect().map_err(|e| stage_error("connect", e))?;
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e| sql_error("begin", e))?;
        check_revision(&tx, expected).map_err(|e| stage_error("revision", e))?;
        let id: String = tx
            .query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))
            .map_err(|e| sql_error("query/write", e))?;
        tx.execute(
            "INSERT INTO analysis_filters VALUES(?1,?2,?3,0)",
            params![id, expected, kind],
        )
        .map_err(|e| sql_error("query/write", e))?;
        tx.commit().map_err(|e| sql_error("commit", e))?;
        Ok(id)
    }
    pub fn analysis_filter_append(
        &self,
        expected: i64,
        id: &str,
        items: &[String],
    ) -> Result<(), String> {
        bounded(id)?;
        cap(items.len())?;
        for s in items {
            bounded(s)?;
        }
        let _filter_writer = self.analysis_filter_gate.lock().map_err(err)?;
        let mut c = self.connect()?;
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
        check_revision(&tx, expected)?;
        let open:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM analysis_filters WHERE id=?1 AND revision=?2 AND sealed=0)",params![id,expected],|r|r.get(0)).map_err(err)?;
        if !open {
            return Err("Analysis filter absent, stale, or sealed".into());
        }
        let mut ordinal: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(ordinal)+1,0) FROM analysis_filter_items WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .map_err(err)?;
        for item in items {
            tx.execute(
                "INSERT OR IGNORE INTO analysis_filter_items VALUES(?1,?2,?3)",
                params![id, item, ordinal],
            )
            .map_err(err)?;
            ordinal = ordinal.checked_add(1).ok_or("Filter ordinal overflow")?;
        }
        tx.commit().map_err(err)
    }
    pub fn analysis_filter_seal(&self, expected: i64, id: &str) -> Result<(), String> {
        bounded(id)?;
        let _filter_writer = self.analysis_filter_gate.lock().map_err(err)?;
        let mut c = self.connect()?;
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
        check_revision(&tx, expected)?;
        if tx
            .execute(
                "UPDATE analysis_filters SET sealed=1 WHERE id=?1 AND revision=?2",
                params![id, expected],
            )
            .map_err(err)?
            != 1
        {
            return Err("Unknown or stale analysis filter".into());
        }
        tx.commit().map_err(err)
    }
    pub fn analysis_filter_delete(&self, id: &str) -> Result<(), String> {
        bounded(id)?;
        let _filter_writer = self.analysis_filter_gate.lock().map_err(err)?;
        let mut c = self.connect()?;
        let tx = c.transaction().map_err(err)?;
        tx.execute("DELETE FROM analysis_filter_items WHERE id=?1", [id])
            .map_err(err)?;
        tx.execute("DELETE FROM analysis_filters WHERE id=?1", [id])
            .map_err(err)?;
        tx.commit().map_err(err)
    }
}
pub(super) fn filter_check(c: &Connection, id: &str, rev: i64, kind: &str) -> Result<(), String> {
    bounded(id)?;
    let ok:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM analysis_filters WHERE id=?1 AND revision=?2 AND kind=?3 AND sealed=1)",params![id,rev,kind],|r|r.get(0)).map_err(err)?;
    if ok {
        Ok(())
    } else {
        Err("Analysis filter absent, stale, unsealed, or wrong kind".into())
    }
}
