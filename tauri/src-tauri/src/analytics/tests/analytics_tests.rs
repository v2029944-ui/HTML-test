#[path = "indexing.rs"]
mod indexing;
#[path = "trends.rs"]
mod trends;
#[path = "advanced.rs"]
mod advanced;
#[path = "exports.rs"]
mod exports;
#[path = "ranges_and_changes.rs"]
mod ranges_and_changes;

use super::*;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::tempdir;
fn fixture() -> Value {
    let row = |org: &str, work: &str, count: f64, amount: f64| json!({"organization":org,"work":work,"count":count,"amount":amount});
    let reports = vec![
        json!({"id":"a","type":"day","start":"2024-02-28","end":"2024-02-28","organizations":["A","B"],"rows":[row("A","one",2.,20.),row("A","two",-3.,30.),row("A","zero",0.,9.),row("A","gone",1.,10.)]}),
        json!({"id":"b","type":"day","start":"2024-02-29","end":"2024-02-29","organizations":["A","B"],"rows":[row("A","one",4.,80.),row("A","two",-2.,40.),row("A","zero",0.,0.),row("A","new",2.,50.)]}),
        json!({"id":"c","type":"day","start":"2024-03-01","end":"2024-03-01","organizations":["A"],"rows":[row("A","one",1.,10.)]}),
    ];
    json!({"reports":reports,"settings":{"orgNames":["A","B","C","D"],"catalog":[],"groups":[{"id":"g","name":"Group","works":["one"]},{"id":"later","name":"Ignored","works":["one","two"]}]}})
}
fn normalized(v: Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.into_iter().map(normalized).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, normalized(v))).collect()),
        v => v,
    }
}
fn golden(
    data: &Value,
    kind: &str,
    orgs: &[&str],
    names: Option<&[&str]>,
    ranges: &[(&str, &str)],
    changes: &ChangeQuery,
) -> Value {
    let input = json!({"advanced":data["advanced"],"reports":data["reports"],"settings":data["settings"],"scope":{"type":kind,"organizations":orgs,"names":names},"ranges":ranges.iter().map(|(from,to)|json!({"from":from,"to":to})).collect::<Vec<_>>(),"changes":{"method":changes.method,"from":changes.from,"to":changes.to,"percent":changes.percent,"minCount":changes.min_count,"minAmount":changes.min_amount}});
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/native_analytics_golden.cjs");
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    normalized(serde_json::from_slice(&result.stdout).unwrap())
}
fn scope(
    store: &Storage,
    rev: i64,
    kind: &str,
    orgs: &[&str],
    names: Option<&[&str]>,
) -> AnalysisScope {
    fn filter(s: &Storage, r: i64, kind: &str, items: &[&str]) -> String {
        let id = s.analysis_filter_create(r, kind).unwrap();
        s.analysis_filter_append(
            r,
            &id,
            &items.iter().map(|x| x.to_string()).collect::<Vec<_>>(),
        )
        .unwrap();
        s.analysis_filter_seal(r, &id).unwrap();
        id
    }
    AnalysisScope {
        report_type: kind.into(),
        organizations: filter(store, rev, "organizations", orgs),
        names: names.map(|n| filter(store, rev, "names", n)),
    }
}
fn prepare(store: &Storage, data: &Value) -> i64 {
    store.save(data.clone()).unwrap();
    let rev = store.revision().unwrap();
    for report in data["reports"].as_array().unwrap() {
        store
            .analysis_index_report(report["id"].as_str().unwrap(), rev)
            .unwrap();
    }
    store.analysis_index_settings(rev).unwrap();
    rev
}

fn query(from: &str, to: &str) -> RangeQuery {
    RangeQuery {
        from: from.into(),
        to: to.into(),
    }
}
fn changes(from: &str, to: &str, method: &str) -> ChangeQuery {
    ChangeQuery {
        method: method.into(),
        from: from.into(),
        to: to.into(),
        percent: 25.,
        min_count: 3.,
        min_amount: 20.,
    }
}
fn check_old(
    store: &Storage,
    rev: i64,
    data: &Value,
    kind: &str,
    orgs: &[&str],
    ranges: &[(&str, &str)],
    q: &ChangeQuery,
) {
    let s = scope(store, rev, kind, orgs, None);
    let old = golden(data, kind, orgs, None, ranges, q);
    for (i, (a, b)) in ranges.iter().enumerate() {
        let q = query(a, b);
        let sum = store.analysis_range_summary(rev, &s, &q).unwrap();
        assert_eq!(json!(sum.total), old["ranges"][i]["total"]);
        assert_eq!(json!(sum.complete), old["ranges"][i]["complete"]);
        assert_eq!(
            json!(store
                .analysis_range_page(rev, &s, &q, None, None, 500)
                .unwrap()),
            old["ranges"][i]["values"]
        );
    }
    let mut kinds = store
        .analysis_issue_page(rev, &s, None, 500)
        .unwrap()
        .into_iter()
        .map(|i| i.kind)
        .collect::<Vec<_>>();
    kinds.sort();
    assert_eq!(json!(kinds), old["issues"]);
    assert_eq!(
        json!(store.analysis_changes_page(rev, &s, q, None, 500).unwrap()),
        old["changes"]
    );
}
