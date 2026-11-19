//! Full-fidelity disk JSON tree. No whole-object/array Value is constructed.
use super::*;
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::io::{BufReader, Read, Write};
pub(super) fn schema(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS management_nodes(handle INTEGER PRIMARY KEY,parent INTEGER,ordinal INTEGER NOT NULL,field TEXT,kind TEXT NOT NULL,value TEXT);
CREATE INDEX IF NOT EXISTS management_children ON management_nodes(parent,ordinal,handle);
CREATE TABLE IF NOT EXISTS management_settings(root INTEGER,groups_node INTEGER,catalog_node INTEGER,orgs_node INTEGER);
CREATE TABLE IF NOT EXISTS management_groups(handle INTEGER PRIMARY KEY,ordinal INTEGER,id TEXT,name TEXT,works_node INTEGER,members INTEGER,unique_members INTEGER);
CREATE INDEX IF NOT EXISTS management_group_names ON management_groups(name,handle);
CREATE TABLE IF NOT EXISTS management_members(handle INTEGER PRIMARY KEY,group_handle INTEGER,ordinal INTEGER,work TEXT);
CREATE INDEX IF NOT EXISTS management_members_order ON management_members(group_handle,handle);
CREATE INDEX IF NOT EXISTS management_members_work ON management_members(work,group_handle);
CREATE TABLE IF NOT EXISTS management_catalog(handle INTEGER PRIMARY KEY,ordinal INTEGER,work TEXT,organization TEXT);
CREATE INDEX IF NOT EXISTS management_catalog_work ON management_catalog(work,organization,handle);
CREATE TABLE IF NOT EXISTS management_orgs(ordinal INTEGER PRIMARY KEY,name TEXT);
").map_err(err)
}
fn insert(
    c: &Connection,
    parent: Option<i64>,
    ordinal: i64,
    field: Option<&str>,
    kind: &str,
    value: Option<&str>,
) -> Result<i64, String> {
    c.execute(
        "INSERT INTO management_nodes(parent,ordinal,field,kind,value) VALUES(?1,?2,?3,?4,?5)",
        params![parent, ordinal, field, kind, value],
    )
    .map_err(err)?;
    Ok(c.last_insert_rowid())
}
struct Seed<'a> {
    c: &'a Connection,
    job: &'a ManagementJob,
    parent: Option<i64>,
    ordinal: i64,
    field: Option<&'a str>,
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = i64;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<i64, D::Error> {
        if self.depth > 120 {
            return Err(serde::de::Error::custom("Settings nesting exceeds120"));
        }
        self.job.check().map_err(serde::de::Error::custom)?;
        self.job.record();
        struct V<'a>(Seed<'a>);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = i64;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON value")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<i64, A::Error> {
                use serde::de::Error;
                let s = self.0;
                let id = insert(s.c, s.parent, s.ordinal, s.field, "object", None)
                    .map_err(A::Error::custom)?;
                let mut n = 0i64;
                while let Some(k) = a.next_key::<String>()? {
                    label(&k).map_err(A::Error::custom)?;
                    a.next_value_seed(Seed {
                        c: s.c,
                        job: s.job,
                        parent: Some(id),
                        ordinal: n,
                        field: Some(&k),
                        depth: s.depth + 1,
                    })?;
                    n = n
                        .checked_add(1)
                        .ok_or_else(|| A::Error::custom("Too many settings fields"))?;
                }
                Ok(id)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<i64, A::Error> {
                use serde::de::Error;
                let s = self.0;
                let id = insert(s.c, s.parent, s.ordinal, s.field, "array", None)
                    .map_err(A::Error::custom)?;
                let mut n = 0i64;
                while a
                    .next_element_seed(Seed {
                        c: s.c,
                        job: s.job,
                        parent: Some(id),
                        ordinal: n,
                        field: None,
                        depth: s.depth + 1,
                    })?
                    .is_some()
                {
                    n = n
                        .checked_add(1)
                        .ok_or_else(|| A::Error::custom("Too many settings elements"))?;
                }
                Ok(id)
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<i64, E> {
                label(v).map_err(E::custom)?;
                insert(
                    self.0.c,
                    self.0.parent,
                    self.0.ordinal,
                    self.0.field,
                    "string",
                    Some(v),
                )
                .map_err(E::custom)
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<i64, E> {
                self.visit_str(&v)
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<i64, E> {
                insert(
                    self.0.c,
                    self.0.parent,
                    self.0.ordinal,
                    self.0.field,
                    "number",
                    Some(&v.to_string()),
                )
                .map_err(E::custom)
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<i64, E> {
                insert(
                    self.0.c,
                    self.0.parent,
                    self.0.ordinal,
                    self.0.field,
                    "number",
                    Some(&v.to_string()),
                )
                .map_err(E::custom)
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<i64, E> {
                let raw = serde_json::to_string(&v).map_err(E::custom)?;
                if !v.is_finite() {
                    return Err(E::custom("Non-finite settings number"));
                }
                insert(
                    self.0.c,
                    self.0.parent,
                    self.0.ordinal,
                    self.0.field,
                    "number",
                    Some(&raw),
                )
                .map_err(E::custom)
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<i64, E> {
                insert(
                    self.0.c,
                    self.0.parent,
                    self.0.ordinal,
                    self.0.field,
                    "bool",
                    Some(if v { "true" } else { "false" }),
                )
                .map_err(E::custom)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<i64, E> {
                insert(
                    self.0.c,
                    self.0.parent,
                    self.0.ordinal,
                    self.0.field,
                    "null",
                    None,
                )
                .map_err(E::custom)
            }
        }
        d.deserialize_any(V(self))
    }
}
pub(super) fn parse(c: &Connection, r: impl Read, job: &ManagementJob) -> Result<i64, String> {
    let bounded = super::read_model::TokenBounded::new(JobRead { inner: r, job });
    let mut de = serde_json::Deserializer::from_reader(BufReader::with_capacity(8192, bounded));
    let id = Seed {
        c,
        job,
        parent: None,
        ordinal: 0,
        field: None,
        depth: 0,
    }
    .deserialize(&mut de)
    .map_err(err)?;
    de.end().map_err(err)?;
    Ok(id)
}
pub(super) fn child(
    c: &Connection,
    parent: i64,
    field: &str,
) -> Result<Option<(i64, String, Option<String>)>, String> {
    let mut stmt=c.prepare("SELECT handle,kind,value FROM management_nodes WHERE parent=?1 AND field=?2 ORDER BY ordinal").map_err(err)?;
    let mut r = stmt.query(params![parent, field]).map_err(err)?;
    let value = r
        .next()
        .map_err(err)?
        .map(|r| {
            Ok::<_, String>((
                r.get(0).map_err(err)?,
                r.get(1).map_err(err)?,
                r.get(2).map_err(err)?,
            ))
        })
        .transpose()?;
    if r.next().map_err(err)?.is_some() {
        return Err(format!("Duplicate recognized settings field:{field}"));
    }
    Ok(value)
}
fn required(
    c: &Connection,
    p: i64,
    key: &str,
    kind: &str,
) -> Result<(i64, Option<String>), String> {
    let (id, k, v) = child(c, p, key)?.ok_or_else(|| format!("Missing settings field:{key}"))?;
    if k != kind {
        return Err(format!("Unsupported settings field type:{key}"));
    }
    Ok((id, v))
}
pub(super) fn derive(c: &Connection, root: i64, job: &ManagementJob) -> Result<(), String> {
    let kind: String = c
        .query_row(
            "SELECT kind FROM management_nodes WHERE handle=?1",
            [root],
            |r| r.get(0),
        )
        .map_err(err)?;
    if kind != "object" {
        return Err("Settings root must be an object".into());
    }
    let groups = required(c, root, "groups", "array")?.0;
    let catalog = required(c, root, "catalog", "array")?.0;
    let orgs = required(c, root, "orgNames", "array")?.0;
    c.execute_batch("DELETE FROM management_settings;DELETE FROM management_groups;DELETE FROM management_members;DELETE FROM management_catalog;DELETE FROM management_orgs;").map_err(err)?;
    let invalid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM management_nodes WHERE parent=?1 AND kind<>'string') OR (SELECT COUNT(*) FROM management_nodes WHERE parent=?1)>100",[orgs],|r|r.get(0)).map_err(err)?;
    if invalid {
        return Err("Organization names require at most100 bounded strings".into());
    }
    c.execute(
        "INSERT INTO management_orgs SELECT ordinal,value FROM management_nodes WHERE parent=?1",
        [orgs],
    )
    .map_err(err)?;
    let mut stmt=c.prepare("SELECT handle,ordinal,kind FROM management_nodes WHERE parent=?1 ORDER BY ordinal,handle").map_err(err)?;
    let mut rows = stmt.query([groups]).map_err(err)?;
    while let Some(r) = rows.next().map_err(err)? {
        job.check()?;
        let id: i64 = r.get(0).map_err(err)?;
        let ordinal: i64 = r.get(1).map_err(err)?;
        if r.get::<_, String>(2).map_err(err)? != "object" {
            return Err("Group must be an object".into());
        }
        let name = required(c, id, "name", "string")?.1.unwrap();
        let groupid = required(c, id, "id", "string")?.1.unwrap();
        let works = required(c, id, "works", "array")?.0;
        let bad: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM management_nodes WHERE parent=?1 AND kind<>'string')",
                [works],
                |r| r.get(0),
            )
            .map_err(err)?;
        if bad {
            return Err("Group works must be strings".into());
        }
        c.execute("INSERT INTO management_members SELECT handle,?1,ordinal,value FROM management_nodes WHERE parent=?2",params![id,works]).map_err(err)?;
        c.execute("INSERT INTO management_groups VALUES(?1,?2,?3,?4,?5,(SELECT COUNT(*) FROM management_members WHERE group_handle=?1),(SELECT COUNT(DISTINCT work) FROM management_members WHERE group_handle=?1))",params![id,ordinal,groupid,name,works]).map_err(err)?;
    }
    drop(rows);
    let mut rows = stmt.query([catalog]).map_err(err)?;
    while let Some(r) = rows.next().map_err(err)? {
        job.check()?;
        let id: i64 = r.get(0).map_err(err)?;
        let ordinal: i64 = r.get(1).map_err(err)?;
        if r.get::<_, String>(2).map_err(err)? != "object" {
            return Err("Catalog entry must be an object".into());
        }
        let work = required(c, id, "work", "string")?.1.unwrap();
        let org = match child(c, id, "organization")? {
            None => None,
            Some((_, k, v)) if k == "string" => v,
            _ => return Err("Unsupported catalog organization".into()),
        };
        c.execute(
            "INSERT INTO management_catalog VALUES(?1,?2,?3,?4)",
            params![id, ordinal, work, org],
        )
        .map_err(err)?;
    }
    c.execute(
        "INSERT INTO management_settings VALUES(?1,?2,?3,?4)",
        params![root, groups, catalog, orgs],
    )
    .map_err(err)?;
    job.check()
}
pub(super) fn write(
    c: &Connection,
    id: i64,
    w: &mut impl Write,
    job: &ManagementJob,
    depth: usize,
) -> Result<(), String> {
    if depth > 120 {
        return Err("Settings nesting exceeds120".into());
    }
    job.check()?;
    let (kind, value): (String, Option<String>) = c
        .query_row(
            "SELECT kind,value FROM management_nodes WHERE handle=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(err)?;
    match kind.as_str() {
        "object" | "array" => {
            let object = kind == "object";
            w.write_all(if object { b"{" } else { b"[" }).map_err(err)?;
            let mut stmt=c.prepare("SELECT handle,field FROM management_nodes WHERE parent=?1 ORDER BY ordinal,handle").map_err(err)?;
            let mut rows = stmt.query([id]).map_err(err)?;
            let mut first = true;
            while let Some(r) = rows.next().map_err(err)? {
                if !first {
                    w.write_all(b",").map_err(err)?;
                }
                first = false;
                if object {
                    let field: String = r.get(1).map_err(err)?;
                    serde_json::to_writer(&mut *w, &field).map_err(err)?;
                    w.write_all(b":").map_err(err)?;
                }
                write(c, r.get(0).map_err(err)?, w, job, depth + 1)?;
            }
            w.write_all(if object { b"}" } else { b"]" }).map_err(err)?;
        }
        "string" => serde_json::to_writer(w, &value.ok_or("Missing string node")?).map_err(err)?,
        "number" | "bool" => w
            .write_all(value.as_deref().ok_or("Missing scalar node")?.as_bytes())
            .map_err(err)?,
        "null" => w.write_all(b"null").map_err(err)?,
        _ => return Err("Unknown JSON node type".into()),
    }
    Ok(())
}
