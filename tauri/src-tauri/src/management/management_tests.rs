use super::*;
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn unified_group_edits_move_exclusively_and_publish_once() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let rev = management_setup(&s, &management_data());
    let groups = s.management_group_page(rev, "", None, 50).unwrap();
    let target = groups[2].handle;
    let receipt = s.unified_save_group(rev, Some(target), "ignored", "Changed", &["Shared".into(), "one".into()]).unwrap();
    let data = s.load().unwrap();
    let groups = data["settings"]["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0],management_data()["settings"]["groups"][0],"Unrelated empty group and extension fields must survive a group edit");
    assert_eq!(groups[1]["id"], "other");
    assert_eq!(groups[1]["name"], "Changed");
    assert_eq!(groups[1]["works"], json!(["Shared", "one"]));
    assert!(s.unified_assign_work(rev, "Shared", None).is_err());
    s.unified_assign_work(receipt.revision, "Shared", None).unwrap();
    assert_eq!(s.load().unwrap()["settings"]["groups"][1]["works"], json!(["one"]));
    assert_eq!(data["reports"], management_data()["reports"]);
}

#[test]
fn unified_assignment_only_removes_sources_emptied_by_the_move() {
    let dir=tempdir().unwrap();let s=Storage::new(dir.path().into()).unwrap();
    let original=json!({"reports":[],"settings":{"orgNames":["A","B","C","D"],"catalog":[],"groups":[
        {"id":"source","name":"Source","works":["alpha"]},
        {"id":"untouched","name":"Unrelated empty B","works":[],"unknown":{"nested":[true,"preserve"]}},
        {"id":"target","name":"Target","works":["beta"],"color":"blue"}
    ]}});
    let rev=management_setup(&s,&original);let target=s.management_group_page(rev,"",None,50).unwrap().into_iter().find(|g|g.id=="target").unwrap();
    s.unified_assign_work(rev,"alpha",Some(target.handle)).unwrap();
    let after=s.load().unwrap();let groups=after["settings"]["groups"].as_array().unwrap();
    assert_eq!(groups.len(),2);assert_eq!(groups[0],original["settings"]["groups"][1]);assert_eq!(groups[1]["works"],json!(["beta","alpha"]));assert_eq!(groups[1]["color"],"blue");
    assert_eq!(after["reports"],original["reports"]);
}

#[test]
fn unified_group_json_round_trips_empty_groups_and_extension_metadata() {
    let dir=tempdir().unwrap();let s=Storage::new(dir.path().join("data")).unwrap();
    let original=json!({"reports":[],"settings":{"orgNames":["A","B","C","D"],"catalog":[],"unknown":{"keep":true},"groups":[
        {"id":"empty","name":"Empty","works":[],"unknown":{"nested":[true,{"text":"preserve"}]},"color":"red"},
        {"id":"active","name":"Active","works":["alpha","beta"],"extra":[null,2,false],"visible":true}
    ]}});
    let rev=management_setup(&s,&original);let destination=dir.path().join("groups.json");s.unified_groups_export(rev,&destination).unwrap();
    let exported:Value=serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();assert_eq!(exported["groups"],original["settings"]["groups"]);
    let review=s.unified_groups_preview(rev,&destination).unwrap();assert_eq!(review.groups,2);assert_eq!(review.works,2);
    assert!(review.requires_backup);
    let backup = dir.path().join("before-import.sqlite3");
    s.unified_groups_import_with_backup(rev,&destination,&review.hash,true,Some(&backup)).unwrap();
    assert!(backup.is_file());
    assert_eq!(s.load().unwrap(), original);
}

#[test]
fn unified_search_is_bounded_russian_case_insensitive_prefix_first() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let rev = management_setup(&s, &json!({"reports":[],"settings":{"orgNames":["А","Б","В","Г"],"groups":[],"catalog":[{"work":"Я ремонт","organization":"А"},{"work":"РЕМОНТ двигателя","organization":"Б"},{"work":"Ремонт блока","organization":"А"}]}}));
    let page = s.unified_management_page(rev, "works", "ремонт", None, 0, 1).unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].name, "Ремонт блока");
    let page = s.unified_management_page(rev, "works", "РЕМОНТ", None, 2, 1).unwrap();
    assert_eq!(page.rows[0].name, "Я ремонт");
}

#[test]
fn unified_group_json_requires_review_confirmation_revision_and_keeps_reports() {
    let dir=tempdir().unwrap();let s=Storage::new(dir.path().join("data")).unwrap();let rev=management_setup(&s,&management_data());
    let source=dir.path().join("groups.json");std::fs::write(&source,r#"{"format":"work-groups","version":1,"groups":[{"id":"a","name":"Новая","works":["one","Shared"]}]}"#).unwrap();
    let review=s.unified_groups_preview(rev,&source).unwrap();assert_eq!(review.groups,1);assert_eq!(review.works,2);
    assert!(s.unified_groups_import(rev,&source,&review.hash,false).is_err());
    assert_eq!(s.revision().unwrap(),rev);
    let destination=dir.path().join("export.json");s.unified_groups_export(rev,&destination).unwrap();assert!(s.unified_groups_export(rev,&destination).is_err());
    assert!(review.requires_backup);
    assert!(s.unified_groups_import(rev,&source,&review.hash,true).is_err());
    let backup = dir.path().join("before-import.sqlite3");
    let receipt=s.unified_groups_import_with_backup(rev,&source,&review.hash,true,Some(&backup)).unwrap();
    assert!(backup.is_file());
    assert!(s.unified_groups_import(rev,&source,&review.hash,true).is_err());
    assert_eq!(s.load().unwrap()["reports"],management_data()["reports"]);
    let before=s.load().unwrap();std::fs::write(&source,r#"{"format":"work-groups","version":1,"groups":[]}"#).unwrap();
    assert!(s.unified_groups_import(receipt.revision,&source,&review.hash,true).is_err());assert_eq!(s.load().unwrap(),before);
}

#[test]
fn unified_report_and_second_sheet_catalog_publish_or_roll_back_together() {
    use std::io::Write;
    let dir=tempdir().unwrap();let source=dir.path().join("report.xlsx");
    let mut zip=zip::ZipWriter::new(std::fs::File::create(&source).unwrap());
    for (name,xml) in [
        ("xl/workbook.xml",r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Отчет" r:id="r1"/><sheet name="Каталог" r:id="r2"/></sheets></workbook>"#),
        ("xl/_rels/workbook.xml.rels",r#"<Relationships><Relationship Id="r1" Target="worksheets/sheet1.xml"/><Relationship Id="r2" Target="worksheets/sheet2.xml"/></Relationships>"#),
        ("xl/worksheets/sheet1.xml",r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Работа</t></is></c><c r="B1" t="inlineStr"><is><t>Количество</t></is></c><c r="C1" t="inlineStr"><is><t>Сумма</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>A</t></is></c><c r="B2"><v>1</v></c><c r="C2"><v>10</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Новая работа</t></is></c><c r="B3"><v>1</v></c><c r="C3"><v>10</v></c></row><row r="4"><c r="A4" t="inlineStr"><is><t>Итого</t></is></c><c r="B4"><v>1</v></c><c r="C4"><v>10</v></c></row></sheetData></worksheet>"#),
        ("xl/worksheets/sheet2.xml",r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Подразделение</t></is></c><c r="B1" t="inlineStr"><is><t>Услуга</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>A</t></is></c><c r="B2" t="inlineStr"><is><t>Справочная услуга</t></is></c></row></sheetData></worksheet>"#),
    ] {zip.start_file(name,zip::write::SimpleFileOptions::default()).unwrap();zip.write_all(xml.as_bytes()).unwrap();}zip.finish().unwrap();
    let area=analiz_import_core::StageArea::open(dir.path().join("xlsx-stage")).unwrap();let cancel=std::sync::atomic::AtomicBool::new(false);
    let mut stage=area.ingest(&source,analiz_import_core::Limits::default(),&cancel,|_|{}).unwrap();
    let conversion=stage.convert_legacy(&analiz_import_core::LegacyOptions{period_dates:None,sheet:"xl/worksheets/sheet1.xml".into(),kind:"day".into(),start:"2026-11-02".into(),organizations:["A".into(),"B".into(),"C".into(),"D".into()],mappings:None},&cancel,|_|{}).unwrap();
    assert_eq!(conversion.rows,1,"Fixture must contain a recognized organization total and one real work row");
    let prepared=stage.prepare(&cancel,|_|{}).unwrap();let s=Storage::new(dir.path().join("live")).unwrap();let rev=management_setup(&s,&management_data());let before=s.load().unwrap();let preview=s.preview_import(&prepared).unwrap();
    let conn=s.connect().unwrap();conn.execute_batch("CREATE TRIGGER reject_catalog_publish BEFORE INSERT ON kv WHEN NEW.name='settings' BEGIN SELECT RAISE(ABORT,'catalog publication rejected');END;").unwrap();drop(conn);
    let confirmation=ImportConfirmation{accept_quality:true,..Default::default()};
    let error=s.apply_import(&prepared,&preview,confirmation,&ImportJob::new(),|_|{}).unwrap_err();assert!(error.contains("catalog publication rejected"),"{error}");assert_eq!(s.load().unwrap(),before);assert_eq!(s.revision().unwrap(),rev);assert!(s.committed_import(prepared.operation_id()).unwrap().is_none());
    s.connect().unwrap().execute_batch("DROP TRIGGER reject_catalog_publish").unwrap();
    let receipt=s.apply_import(&prepared,&preview,confirmation,&ImportJob::new(),|_|{}).unwrap();assert_eq!(receipt.reports,1);
    let after=s.load().unwrap();assert_eq!(after["reports"].as_array().unwrap().len(),2);assert_eq!(after["settings"]["unknown"],before["settings"]["unknown"]);assert!(after["settings"]["catalog"].as_array().unwrap().iter().any(|r|r["work"]=="Справочная услуга"));
    assert_eq!(s.apply_import(&prepared,&preview,confirmation,&ImportJob::new(),|_|{}).unwrap(),receipt);
}
fn management_data() -> Value {
    json!({"reports":[{"id":"keep","type":"day","start":"2026-11-01","end":"2026-11-01","amount":30,"organizations":["A","B"],"warnings":[{"unknown":[1,2]}],"rows":[{"organization":"A","work":"Report only","count":2,"amount":30}]}],"settings":{"orgNames":["A","B","C","D"],"unknown":{"a":[{"preserve":true},[1,2,3]],"label":"escaped \"x\""},"groups":[{"id":"dup","name":"Empty","works":[],"unknown":{"nested":[1,2]}},{"id":"dup","name":"First","works":["one","one","Shared"],"color":"red"},{"id":"other","name":"Second","works":["Shared","group only"]}],"catalog":[{"work":"Catalog only","organization":"B","unknown":{"x":[1,2]}},{"work":"one","organization":"A"}]}})
}
fn management_setup(s: &Storage, data: &Value) -> i64 {
    s.save(data.clone()).unwrap();
    let rev = s.revision().unwrap();
    s.management_index_settings(rev, &ManagementJob::new())
        .unwrap();
    for r in data["reports"].as_array().unwrap() {
        s.management_index_report(r["id"].as_str().unwrap(), rev, &ManagementJob::new())
            .unwrap();
    }
    rev
}
#[test]
fn management_full_fidelity_pages_counts_and_settings_only_crud() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let original = management_data();
    let rev = management_setup(&s, &original);
    let boot = s.management_bootstrap(rev).unwrap();
    assert_eq!(boot.groups, 3);
    assert_eq!(boot.memberships, 5);
    assert_eq!(boot.catalog, 2);
    assert_eq!(boot.reports, 1);
    assert_eq!(boot.org_names, vec!["A", "B", "C", "D"]);
    let first = s.management_group_page(rev, "", None, 1).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].name, "Empty");
    assert_eq!(first[0].members, 0);
    let later = s
        .management_group_page(rev, "", Some(first[0].handle), 500)
        .unwrap();
    assert_eq!(later.len(), 2);
    assert_eq!(later[0].id, "dup");
    assert_eq!(later[0].members, 3);
    assert_eq!(
        s.management_member_page(rev, later[0].handle, None, 1)
            .unwrap()[0]
            .work,
        "one"
    );
    assert_eq!(
        s.management_member_page(rev, later[0].handle, None, 500)
            .unwrap()
            .len(),
        3
    );
    let work = s.management_work_summary(rev, "", None).unwrap();
    assert_eq!(work.total, 5);
    assert_eq!(work.grouped, 3);
    assert_eq!(
        s.management_work_page(rev, "", None, None, 1)
            .unwrap()
            .len(),
        1
    );
    let before_reports = s
        .connect()
        .unwrap()
        .query_row("SELECT json FROM reports WHERE id='keep'", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap();
    let job = Arc::new(ManagementJob::new());
    let mut edit = s
        .management_begin_settings(rev, 64 * 1024 * 1024, job)
        .unwrap();
    edit.rename_group(later[0].handle, "Renamed").unwrap();
    edit.append_members(first[0].handle, &["new".into(), "Shared".into()])
        .unwrap();
    let group = edit.create_group("dup", "Third").unwrap();
    edit.append_members(group, &["last".into()]).unwrap();
    let catalog = s.management_catalog_page(rev, "", None, None, 500).unwrap();
    edit.update_catalog(catalog[0].handle, "Changed catalog", Some("B"))
        .unwrap();
    edit.set_scalar("theme", ManagementScalar::String("dark".into()))
        .unwrap();
    let receipt = s.management_commit_settings(edit).unwrap();
    assert!(receipt.revision > rev);
    assert_eq!(
        s.connect()
            .unwrap()
            .query_row("SELECT json FROM reports WHERE id='keep'", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        before_reports
    );
    let data = s.load().unwrap();
    assert_eq!(data["settings"]["unknown"], original["settings"]["unknown"]);
    assert_eq!(data["settings"]["groups"][1]["color"], "red");
    assert_eq!(
        data["settings"]["groups"][0]["unknown"],
        original["settings"]["groups"][0]["unknown"]
    );
    assert_eq!(
        data["settings"]["groups"][0]["works"],
        json!(["new", "Shared"])
    );
    assert_eq!(
        data["settings"]["groups"][1]["works"],
        original["settings"]["groups"][1]["works"]
    );
    assert_eq!(
        data["settings"]["catalog"][0]["unknown"],
        original["settings"]["catalog"][0]["unknown"]
    );
    assert_eq!(data["settings"]["theme"], "dark");
    assert_eq!(s.management_bootstrap(receipt.revision).unwrap().groups, 4);
}
#[test]
fn management_targeted_delete_clear_stale_and_cancel_preserve_settings() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let mut data = management_data();
    let mut second = data["reports"][0].clone();
    second["id"] = json!("other");
    data["reports"].as_array_mut().unwrap().push(second);
    let rev = management_setup(&s, &data);
    let job = ManagementJob::new();
    job.cancel();
    assert!(s
        .management_delete_reports(rev, &["keep".into()], &job)
        .is_err());
    let receipt = s
        .management_delete_reports(rev, &["keep".into()], &ManagementJob::new())
        .unwrap();
    assert_eq!(receipt.deleted, 1);
    assert_eq!(s.load().unwrap()["reports"].as_array().unwrap().len(), 1);
    assert!(s
        .management_clear_reports(rev, &ManagementJob::new())
        .is_err());
    let receipt = s
        .management_clear_reports(receipt.revision, &ManagementJob::new())
        .unwrap();
    assert_eq!(receipt.deleted, 1);
    assert_eq!(s.load().unwrap()["settings"], data["settings"]);
    assert!(s.load().unwrap()["reports"].as_array().unwrap().is_empty());
}
fn management_golden(data: &Value) -> Value {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/native_management_golden.cjs");
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
        .write_all(data.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn management_real_js_goldens_full_counts_search_navigation_and_unknown_nodes() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let mut data = management_data();
    let mut second = data["reports"][0].clone();
    second["id"] = json!("later");
    second["start"] = json!("2026-11-02");
    second["end"] = json!("2026-11-02");
    data["reports"].as_array_mut().unwrap().push(second);
    let rev = management_setup(&s, &data);
    let old = management_golden(&data);
    let coverage = s.management_work_summary(rev, "", None).unwrap();
    assert_eq!(
        json!({"total":coverage.total,"grouped":coverage.grouped,"ungrouped":coverage.ungrouped}),
        old["coverage"]
    );
    let groups = s.management_group_page(rev, "", None, 500).unwrap();
    let actual: Vec<_> = groups
        .iter()
        .map(|g| json!({"id":g.id,"name":g.name,"members":g.members,"unique":g.unique_members,"works":s.management_member_page(rev,g.handle,None,500).unwrap().iter().map(|m|m.work.clone()).collect::<Vec<_>>()}))
        .collect();
    assert_eq!(json!(actual), old["groups"]);
    assert_eq!(
        json!(s.management_work_page(rev, "", None, None, 500).unwrap()),
        old["union"]
    );
    for report in s
        .management_report_page(rev, &ManagementReportFilter::default(), None, 500)
        .unwrap()
    {
        let old_report = old["reports"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == report.id)
            .unwrap();
        assert_eq!(report.rows, old_report["rows"].as_i64().unwrap());
        assert_eq!(report.saved_amount, old_report["savedAmount"].as_f64());
        assert_eq!(report.warnings, old_report["warnings"].as_i64().unwrap());
        let orgs = s
            .management_report_org_page(rev, &report.id, None, 500)
            .unwrap()
            .into_iter()
            .map(|(_, name)| name)
            .collect::<Vec<_>>();
        assert_eq!(json!(orgs), old_report["organizations"]);
    }
    assert_eq!(s.management_group_count(rev, "Second").unwrap(), 1);
    assert_eq!(
        s.management_catalog_count(rev, "one", Some("A")).unwrap(),
        1
    );
    let f = ManagementReportFilter::default();
    let total = s.management_report_summary(rev, &f).unwrap();
    assert_eq!(
        (total.reports, total.rows, total.amount, total.periods),
        (2, 2, Some(60.), 2)
    );
    let second_page = s.management_report_window(rev, &f, 1, 1).unwrap();
    assert_eq!(second_page.total_rows, 2);
    assert_eq!(second_page.rows[0].id, "keep");
    let filtered = ManagementReportFilter { to: Some("2026-11-01".into()), ..f.clone() };
    let only = s.management_report_window(rev, &filtered, 0, 1).unwrap();
    assert_eq!(only.total_rows, 1);
    assert_eq!(only.rows[0].id, "keep");
    assert!(s.management_report_window(rev, &filtered, 1, 1).is_err());
    assert!(s.management_report_window(rev + 1, &f, 0, 1).is_err());
    assert!(s.management_report_window(rev, &f, usize::MAX, 2).is_err());
    let empty = ManagementReportFilter { organization: Some("unknown".into()), ..f.clone() };
    let empty_page = s.management_report_window(rev, &empty, 0, 1).unwrap();
    assert_eq!(empty_page.total_rows, 0);
    assert!(empty_page.rows.is_empty());
    let org_page = s.management_report_org_window(rev, "keep", 1, 1).unwrap();
    assert_eq!(org_page.total_rows, 2);
    assert_eq!(org_page.rows, vec![(1, "B".into())]);
    assert!(s.management_report_org_window(rev, "keep", 2, 1).is_err());
    let first = s.management_report_page(rev, &f, None, 1).unwrap();
    assert_eq!(first[0].id, "later");
    assert_eq!(first[0].saved_amount, Some(30.));
    assert_eq!(
        s.management_report_page(rev, &f, Some(&first[0].cursor()), 1)
            .unwrap()[0]
            .id,
        "keep"
    );
    let f = ManagementReportFilter {
        report_type: Some("day".into()),
        ..Default::default()
    };
    let periods = s.management_period_page(rev, &f, None, 1).unwrap();
    assert_eq!(periods[0].start, "2026-11-02");
    assert_eq!(
        s.management_period_page(rev, &f, Some(&periods[0].start), 1)
            .unwrap()[0]
            .start,
        "2026-11-01"
    );
    assert_eq!(
        s.management_report_org_page(rev, "keep", None, 500)
            .unwrap(),
        vec![(0, "A".into()), (1, "B".into())]
    );
    let root = s.management_node_page(rev, None, None, 1).unwrap()[0].handle;
    let fields = s.management_node_page(rev, Some(root), None, 500).unwrap();
    let unknown = fields
        .iter()
        .find(|n| n.field.as_deref() == Some("unknown"))
        .unwrap();
    assert_eq!(unknown.kind, "object");
    assert_eq!(unknown.children, 2);
    assert_eq!(
        s.management_node_page(rev, Some(unknown.handle), None, 1)
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn management_stale_cancel_fault_and_private_session_cleanup_preserve_live_data() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let data = management_data();
    let rev = management_setup(&s, &data);
    let groups = s.management_group_page(rev, "", None, 500).unwrap();
    let job = Arc::new(ManagementJob::new());
    let mut edit = s
        .management_begin_settings(rev, 16 * 1024 * 1024, job.clone())
        .unwrap();
    edit.rename_group(groups[0].handle, "not committed")
        .unwrap();
    assert!(edit
        .set_scalar("orgNames", ManagementScalar::String("bad".into()))
        .is_err());
    assert_eq!(s.management_cleanup_sessions().unwrap(), (0, 1));
    job.cancel();
    assert!(s.management_commit_settings(edit).is_err());
    assert_eq!(s.load().unwrap(), data);
    assert_eq!(s.management_cleanup_sessions().unwrap(), (0, 0));
    let mut edit = s
        .management_begin_settings(rev, 16 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    edit.rename_group(groups[0].handle, "stale").unwrap();
    let c = s.connect().unwrap();
    c.execute("UPDATE reports SET json=json WHERE id='keep'", [])
        .unwrap();
    assert!(s
        .management_commit_settings(edit)
        .unwrap_err()
        .contains("Stale"));
    let new = s.revision().unwrap();
    assert_eq!(s.load().unwrap(), data);
    let mut edit = s
        .management_begin_settings(new, 16 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    edit.rename_group(groups[0].handle, "failed").unwrap();
    c.execute_batch("CREATE TRIGGER management_injected_failure BEFORE INSERT ON kv WHEN NEW.name='settings' BEGIN SELECT RAISE(ABORT,'injected settings write');END;").unwrap();
    assert!(s
        .management_commit_settings(edit)
        .unwrap_err()
        .contains("injected"));
    assert_eq!(s.load().unwrap(), data);
    assert_eq!(s.revision().unwrap(), new);
    assert_eq!(s.management_cleanup_sessions().unwrap(), (0, 0));
}
#[test]
fn management_session_sqlite_full_rolls_back_batch_and_reports_are_unaddressable() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let data = management_data();
    let rev = management_setup(&s, &data);
    let group = s.management_group_page(rev, "", None, 1).unwrap()[0].handle;
    let mut edit = s
        .management_begin_settings(rev, 4 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    let huge = vec!["x".repeat(60000); 100];
    assert!(edit.append_members(group, &huge).is_err());
    assert!(edit.member_page(group, None, 500).unwrap().is_empty());
    edit.append_members(group, &["small".into()]).unwrap();
    let c = s.connect().unwrap();
    c.execute_batch("CREATE TRIGGER protect_reports_delete BEFORE DELETE ON reports BEGIN SELECT RAISE(ABORT,'settings touched reports');END;CREATE TRIGGER protect_reports_update BEFORE UPDATE ON reports BEGIN SELECT RAISE(ABORT,'settings touched reports');END;CREATE TRIGGER protect_reports_insert BEFORE INSERT ON reports BEGIN SELECT RAISE(ABORT,'settings touched reports');END;").unwrap();
    let receipt = s.management_commit_settings(edit).unwrap();
    assert!(receipt.revision > rev);
    assert_eq!(s.load().unwrap()["reports"], data["reports"]);
    assert_eq!(
        s.load().unwrap()["settings"]["groups"][0]["works"],
        json!(["small"])
    );
}
#[test]
fn management_live_sqlite_full_rolls_back_settings_blob_revision_and_indexes() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let data = management_data();
    let rev = management_setup(&s, &data);
    let c = s.connect().unwrap();
    let pages: i64 = c.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap();
    let mut edit = s
        .management_begin_settings(rev, 16 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    edit.set_scalar("large", ManagementScalar::String("z".repeat(60000)))
        .unwrap();
    let error = s
        .management_commit_settings_with_limit(edit, Some(pages))
        .unwrap_err();
    assert!(error.contains("full"), "{error}");
    assert_eq!(s.revision().unwrap(), rev);
    assert_eq!(s.load().unwrap(), data);
    assert_eq!(s.management_bootstrap(rev).unwrap().memberships, 5);
}
#[test]
fn management_text_blob_roundtrip_delete_domain_and_unknown_shapes() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let data = management_data();
    let _rev = management_setup(&s, &data);
    let c = s.connect().unwrap();
    c.execute(
        "UPDATE kv SET json=CAST(json AS BLOB) WHERE name='settings'",
        [],
    )
    .unwrap();
    let rev = s.revision().unwrap();
    s.management_index_settings(rev, &ManagementJob::new())
        .unwrap();
    assert_eq!(s.load().unwrap()["settings"], data["settings"]);
    let groups = s.management_group_page(rev, "", None, 500).unwrap();
    let catalog = s.management_catalog_page(rev, "", None, None, 500).unwrap();
    let mut edit = s
        .management_begin_settings(rev, 16 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    edit.delete_group(groups[1].handle).unwrap();
    edit.delete_catalog(&[catalog[1].handle]).unwrap();
    let created = edit.create_catalog("new catalog", Some("A")).unwrap();
    edit.update_catalog(created, "new updated", None).unwrap();
    let newgroup = edit.create_group("dup", "Retained duplicate").unwrap();
    edit.append_members(newgroup, &["new work".into(), "same".into()])
        .unwrap();
    let members = edit.member_page(newgroup, None, 500).unwrap();
    edit.remove_members(newgroup, &[members[0].handle]).unwrap();
    let receipt = s.management_commit_settings(edit).unwrap();
    let data = s.load().unwrap();
    assert_eq!(data["settings"]["groups"][0]["id"], "dup");
    assert_eq!(data["settings"]["groups"][0]["name"], "Empty");
    assert_eq!(data["settings"]["groups"][1]["name"], "Second");
    assert_eq!(data["settings"]["groups"][2]["works"], json!(["same"]));
    assert_eq!(
        data["settings"]["catalog"][1],
        json!({"work":"new updated"})
    );
    assert_eq!(
        s.management_catalog_count(receipt.revision, "", None)
            .unwrap(),
        2
    );
    assert!(s.management_group_page(rev, "", None, 1).is_err());
}
#[test]
fn management_restart_child_session() {
    let Some(root) = std::env::var_os("ANALIZ_MANAGEMENT_CRASH_STAGE") else {
        return;
    };
    let s = Storage::new(PathBuf::from(root)).unwrap();
    let rev = s.management_revision().unwrap();
    let edit = s
        .management_begin_settings(rev, 16 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    std::mem::forget(edit);
    std::process::exit(0);
}
#[test]
fn management_restart_cleanup_respects_live_leases_and_preserves_unknown_dirs() {
    use std::process::Command;
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let rev = management_setup(&s, &management_data());
    let live = s
        .management_begin_settings(rev, 16 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("storage::management_tests::management_restart_child_session")
        .env("ANALIZ_MANAGEMENT_CRASH_STAGE", dir.path())
        .status()
        .unwrap();
    assert!(status.success());
    let foreign = dir.path().join("management-stages").join("user-directory");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("keep"), "user data").unwrap();
    assert_eq!(s.management_cleanup_sessions().unwrap(), (1, 1));
    assert!(foreign.join("keep").exists());
    drop(live);
    assert_eq!(s.management_cleanup_sessions().unwrap(), (0, 0));
}
#[test]
fn management_bounded_bootstrap_refuses_implicit_legacy_snapshot_migration() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("reports.json"),
        management_data().to_string(),
    )
    .unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    assert!(s
        .management_revision()
        .unwrap_err()
        .contains("explicit migration"));
    let c = Connection::open(dir.path().join("reports.sqlite3")).unwrap();
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM reports", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(dir.path().join("reports.json").exists());
}
#[test]
fn management_shared_source_artifact_deletion_is_fail_closed_until_c3_lifecycle() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let mut data = management_data();
    for (field, value) in [
        ("sourceDiagnosticsReport", json!("shared-source")),
        ("sourceDiagnostics", json!({"issues":1,"hash":"x"})),
        ("sourceIssues", json!([{"kind":"invalid"}])),
    ] {
        data = management_data();
        data["reports"][0][field] = value;
        let rev = management_setup(&s, &data);
        assert!(
            s.management_report_page(rev, &ManagementReportFilter::default(), None, 1)
                .unwrap()[0]
                .shared_source_artifact
        );
        assert!(s
            .management_delete_reports(rev, &["keep".into()], &ManagementJob::new())
            .unwrap_err()
            .contains("Shared source"));
        assert!(s
            .management_clear_reports(rev, &ManagementJob::new())
            .is_err());
        assert_eq!(s.load().unwrap(), data);
    }
    let c = s.connect().unwrap();
    c.execute("UPDATE reports SET json=json WHERE id='keep'", [])
        .unwrap();
    let rev = s.revision().unwrap();
    assert!(s
        .management_clear_reports(rev, &ManagementJob::new())
        .unwrap_err()
        .contains("unindexed"));
    assert_eq!(s.load().unwrap(), data);
}
#[test]
fn management_bounds_corruption_no_partial_settings_and_session_limit() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let data = management_data();
    let rev = management_setup(&s, &data);
    assert!(s.management_group_page(rev, "", None, 0).is_err());
    assert!(s.management_member_page(rev, 999999, None, 500).is_err());
    assert!(s
        .management_catalog_page(rev, &"x".repeat(513), None, None, 1)
        .is_err());
    let mut sessions = Vec::new();
    for _ in 0..4 {
        sessions.push(
            s.management_begin_settings(rev, 4 * 1024 * 1024, Arc::new(ManagementJob::new()))
                .unwrap(),
        );
    }
    assert!(s
        .management_begin_settings(rev, 4 * 1024 * 1024, Arc::new(ManagementJob::new()))
        .is_err());
    drop(sessions);
    assert_eq!(s.management_cleanup_sessions().unwrap(), (0, 0));
    let c = s.connect().unwrap();
    for raw in [
        b"{\"groups\":[],\"groups\":[],\"catalog\":[],\"orgNames\":[]}".to_vec(),
        vec![0xff, 0xfe],
        format!(
            "{{\"groups\":[],\"catalog\":[],\"orgNames\":[],\"unknown\":\"{}\"}}",
            "x".repeat(65537)
        )
        .into_bytes(),
    ] {
        c.execute("UPDATE kv SET json=?1 WHERE name='settings'", [&raw])
            .unwrap();
        let rev = s.revision().unwrap();
        assert!(s
            .management_index_settings(rev, &ManagementJob::new())
            .is_err());
        assert!(s
            .management_bootstrap(rev)
            .unwrap_err()
            .contains("unindexed"));
        assert_eq!(
            c.query_row("SELECT json FROM kv WHERE name='settings'", [], |r| r
                .get::<_, Vec<u8>>(0))
                .unwrap(),
            raw
        );
    }
}
#[test]
fn management_concurrent_writer_and_mid_parse_cancel_never_publish_old_settings() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let mut data = management_data();
    data["settings"]["largeUnknown"] = json!((0..40000).collect::<Vec<_>>());
    s.save(data.clone()).unwrap();
    let c = s.connect().unwrap();
    let rev = s.revision().unwrap();
    let job = Arc::new(ManagementJob::new());
    let worker_job = job.clone();
    let root = dir.path().to_path_buf();
    let worker = thread::spawn(move || {
        Storage::new(root)
            .unwrap()
            .management_index_settings(rev, &worker_job)
    });
    let start = Instant::now();
    while job.records_processed() < 100 {
        assert!(start.elapsed() < Duration::from_secs(10));
        thread::yield_now();
    }
    c.execute("UPDATE kv SET json=json WHERE name='settings'", [])
        .unwrap();
    assert!(worker.join().unwrap().is_err());
    let rev = s.revision().unwrap();
    assert!(s.management_bootstrap(rev).is_err());
    assert_eq!(s.load().unwrap()["settings"], data["settings"]);
    let job = Arc::new(ManagementJob::new());
    let worker_job = job.clone();
    let root = dir.path().to_path_buf();
    let worker = thread::spawn(move || {
        Storage::new(root)
            .unwrap()
            .management_index_settings(rev, &worker_job)
    });
    let start = Instant::now();
    while job.records_processed() < 100 {
        assert!(start.elapsed() < Duration::from_secs(10));
        thread::yield_now();
    }
    assert!(job.cancel());
    assert!(worker.join().unwrap().is_err());
    assert_eq!(s.revision().unwrap(), rev);
    assert!(s.management_bootstrap(rev).is_err());
    assert_eq!(s.management_cleanup_sessions().unwrap(), (0, 0));
}
#[test]
fn management_report_metadata_requires_complete_raw_index_without_implicit_backfill() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let rev = management_setup(&s, &management_data());
    let c = s.connect().unwrap();
    c.execute("DELETE FROM report_row_index WHERE report_id='keep'", [])
        .unwrap();
    assert!(s
        .management_index_report("keep", rev, &ManagementJob::new())
        .unwrap_err()
        .contains("Raw report index"));
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM report_row_index WHERE report_id='keep'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    s.index_report("keep", rev).unwrap();
    s.management_index_report("keep", rev, &ManagementJob::new())
        .unwrap();
}
#[test]
fn management_delete_preserves_unrelated_kv_receipts_and_report_indexes() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let mut data = management_data();
    let mut other = data["reports"][0].clone();
    other["id"] = json!("other");
    data["reports"].as_array_mut().unwrap().push(other);
    let rev = management_setup(&s, &data);
    let c = s.connect().unwrap();
    c.execute(
        "INSERT INTO kv VALUES('unrelated','{\"keep\":[1,2,3]}')",
        [],
    )
    .unwrap();
    let operation = "a".repeat(64);
    c.execute(
        "INSERT INTO import_receipts VALUES(?1,?2,2,2,0)",
        params![operation, rev],
    )
    .unwrap();
    let rev = s.revision().unwrap();
    assert!(s
        .management_delete_reports(
            rev,
            &["keep".into(), "missing".into()],
            &ManagementJob::new()
        )
        .is_err());
    assert_eq!(s.management_bootstrap(rev).unwrap().reports, 2);
    let job = ManagementJob::new();
    let receipt = s
        .management_delete_reports(rev, &["keep".into(), "keep".into()], &job)
        .unwrap();
    assert_eq!(receipt.deleted, 1);
    assert!(!job.cancel());
    assert_eq!(
        s.management_bootstrap(receipt.revision)
            .unwrap()
            .indexed_reports,
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM report_row_index WHERE report_id='other'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM import_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT json FROM kv WHERE name='unrelated'", [], |r| r
            .get::<_, String>(
            0
        ))
        .unwrap(),
        "{\"keep\":[1,2,3]}"
    );
    let cleared = s
        .management_clear_reports(receipt.revision, &ManagementJob::new())
        .unwrap();
    assert_eq!(cleared.deleted, 1);
    assert_eq!(s.load().unwrap()["settings"], data["settings"]);
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM import_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM kv WHERE name='unrelated'", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap(),
        1
    );
}

#[test]
fn c3_source_diagnostic_links_block_management_delete_even_without_json_sidecars() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let mut data = management_data();
    let mut other = data["reports"][0].clone();
    other["id"] = json!("other");
    data["reports"].as_array_mut().unwrap().push(other);
    let revision = management_setup(&s, &data);
    // Model a native-wide day's authoritative source-sidecar link. The JSON
    // metadata cache alone must never certify deletion of a linked report.
    let c = s.connect().unwrap();
    c.execute(
        "INSERT INTO report_source_diagnostic_links(report_id,anchor_id) VALUES('keep','keep')",
        [],
    ).unwrap();
    let before = s.load().unwrap();
    let error = s.management_delete_reports(
        revision,
        &["keep".into()],
        &ManagementJob::new(),
    ).unwrap_err();
    assert!(error.contains("source diagnostics"), "{error}");
    let error = s.management_clear_reports(
        revision,
        &ManagementJob::new(),
    ).unwrap_err();
    assert!(error.contains("source diagnostics"), "{error}");
    let backup = dir.path().join("linked.sqlite3");
    let error = s
        .management_delete_report_with_backup(revision, "keep", &backup, &ManagementJob::new())
        .unwrap_err();
    assert!(error.contains("source diagnostics"), "{error}");
    assert!(backup.exists());
    assert_eq!(s.revision().unwrap(), revision);
    assert_eq!(s.load().unwrap(), before);
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM report_source_diagnostic_links", [], |r| r.get::<_, i64>(0)).unwrap(),
        1,
    );
    // Ordinary unrelated reports can still be removed through bounded CRUD.
    let receipt = s.management_delete_reports(
        revision,
        &["other".into()],
        &ManagementJob::new(),
    ).unwrap();
    assert_eq!(receipt.deleted, 1);
    assert_eq!(s.load().unwrap()["reports"][0]["id"], "keep");
    assert_eq!(s.load().unwrap()["settings"], data["settings"]);
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM report_source_diagnostic_links", [], |r| r.get::<_, i64>(0)).unwrap(),
        1,
    );
}


#[test]
fn management_reindex_after_folder_change_keeps_reports_and_settings() {
    let dir = tempdir().unwrap();
    let storage = Storage::new(dir.path().join("live")).unwrap();
    let data = management_data();
    let original_revision = management_setup(&storage, &data);
    let before = storage.management_bootstrap(original_revision).unwrap();
    let excel_folder = dir.path().join("excel-files");
    std::fs::create_dir(&excel_folder).unwrap();

    // Changing the Excel folder changes the SQLite revision but must not
    // erase existing reports or break the bounded settings bootstrap.
    storage
        .set_folder(excel_folder.to_string_lossy().into_owned())
        .unwrap();
    let current_revision = storage.revision().unwrap();
    assert!(current_revision > original_revision);
    for _ in 0..2 {
        storage
            .management_index_settings(current_revision, &ManagementJob::new())
            .unwrap();
        let after = storage.management_bootstrap(current_revision).unwrap();
        assert_eq!(after.reports, before.reports);
        assert_eq!(after.org_names, before.org_names);
        assert_eq!(after.groups, before.groups);
        assert_eq!(after.catalog, before.catalog);
    }
    assert_eq!(storage.load().unwrap()["reports"], data["reports"]);
    assert_eq!(storage.load().unwrap()["settings"], data["settings"]);
}

#[test]
fn management_single_report_delete_requires_fresh_verified_backup() {
    let dir = tempdir().unwrap();
    let s = Storage::new(dir.path().into()).unwrap();
    let data = management_data();
    s.save(data.clone()).unwrap();
    let revision = s.revision().unwrap();
    let stale = dir.path().join("stale.sqlite3");
    let job = ManagementJob::new();
    assert!(s
        .management_delete_report_with_backup(revision - 1, "keep", &stale, &job)
        .unwrap_err()
        .contains("Stale storage revision"));
    assert!(!stale.exists());

    let existing = dir.path().join("existing.sqlite3");
    fs::write(&existing, "keep this file").unwrap();
    assert!(s
        .management_delete_report_with_backup(revision, "keep", &existing, &job)
        .is_err());
    assert_eq!(fs::read_to_string(&existing).unwrap(), "keep this file");
    assert_eq!(s.revision().unwrap(), revision);

    let backup = dir.path().join("before-delete.sqlite3");
    let receipt = s
        .management_delete_report_with_backup(revision, "keep", &backup, &job)
        .unwrap();
    assert_eq!(receipt.deleted, 1);
    assert!(receipt.revision > revision);
    assert!(s.load().unwrap()["reports"].as_array().unwrap().is_empty());
    assert_eq!(s.load().unwrap()["settings"], data["settings"]);
    let copy = Connection::open_with_flags(backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap();
    assert_eq!(read_model::revision(&copy).unwrap(), revision);
    let backed_up: String = copy
        .query_row("SELECT id FROM reports", [], |r| r.get(0))
        .unwrap();
    assert_eq!(backed_up, "keep");
}

#[test]
fn management_single_delete_without_backup_indexes_then_preserves_other_data() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().join("live")).unwrap();
    let mut data = management_data();
    let mut other = data["reports"][0].clone();
    other["id"] = json!("other");
    data["reports"].as_array_mut().unwrap().push(other.clone());
    store.save(data.clone()).unwrap();
    let revision = store.revision().unwrap();
    let cancelled = ManagementJob::new();
    cancelled.cancel();
    assert!(store.management_delete_report(revision, "keep", &cancelled).is_err());
    assert!(store.management_delete_report(revision - 1, "keep", &ManagementJob::new()).is_err());
    assert_eq!(store.load().unwrap(), data);
    let receipt = store.management_delete_report(revision, "keep", &ManagementJob::new()).unwrap();
    assert_eq!(receipt.deleted, 1);
    assert!(receipt.revision > revision);
    let after = store.load().unwrap();
    assert_eq!(after["reports"], json!([other]));
    assert_eq!(after["settings"], data["settings"]);
    assert_eq!(store.connect().unwrap().query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0)).unwrap(), "ok");
}

#[test]
fn deletion_without_backup_rejects_integrity_failure_before_single_or_clear() {
    for single in [true, false] {
        let dir = tempdir().unwrap();
        let store = Storage::new(dir.path().into()).unwrap();
        let data = management_data();
        let revision = management_setup(&store, &data);
        store.connect().unwrap().execute_batch("PRAGMA ignore_check_constraints=ON; INSERT INTO storage_revision(singleton,revision) VALUES(2,0);").unwrap();
        let result = if single {
            store.management_delete_report(revision, "keep", &ManagementJob::new())
        } else {
            store.unified_clear_reports(revision, &ManagementJob::new())
        };
        assert!(result.unwrap_err().contains("integrity check failed"));
        assert_eq!(store.load().unwrap(), data);
        assert_eq!(store.revision().unwrap(), revision);
    }
}

#[test]
fn single_delete_without_backup_rolls_back_on_sql_failure() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let data = management_data();
    store.save(data.clone()).unwrap();
    let revision = store.revision().unwrap();
    store.connect().unwrap().execute_batch("CREATE TRIGGER reject_single_delete BEFORE DELETE ON reports BEGIN SELECT RAISE(ABORT,'injected single delete failure'); END;").unwrap();
    let error = store.management_delete_report(revision, "keep", &ManagementJob::new()).unwrap_err();
    assert!(error.contains("injected single delete failure"), "{error}");
    assert_eq!(store.load().unwrap(), data);
    assert_eq!(store.revision().unwrap(), revision);
    assert_eq!(store.connect().unwrap().query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0)).unwrap(), "ok");
}

#[test]
fn management_settings_index_allows_concurrent_analysis_filter_writes() {
    let dir=tempdir().unwrap();
    let root=dir.path().to_path_buf();
    let storage=Storage::new(root.clone()).unwrap();
    let mut data=management_data();
    data["settings"]["catalog"]=Value::Array((0..15000).map(|i| json!({"work":format!("Work {i}")})).collect());
    storage.save(data).unwrap();
    let revision=storage.revision().unwrap();
    let running=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let writer_running=running.clone();
    let writer=std::thread::spawn(move || {
        let concurrent=Storage::new(root).unwrap();
        let mut writes=0;
        while writer_running.load(std::sync::atomic::Ordering::SeqCst) {
            let id=concurrent.analysis_filter_create(revision,"organizations").unwrap();
            concurrent.analysis_filter_delete(&id).unwrap();writes+=1;
        }
        writes
    });
    let result=storage.management_index_settings(revision,&ManagementJob::new());
    running.store(false,std::sync::atomic::Ordering::SeqCst);
    let writes=writer.join().unwrap();
    assert!(writes>0,"concurrent writes must overlap staging");
    assert!(result.is_ok(),"index publication failed during unrelated filter writes: {result:?}");
    assert_eq!(storage.management_bootstrap(revision).unwrap().catalog,15000);
}

#[test]
fn management_settings_index_stages_before_entering_filter_writer_queue() {
    let dir = tempdir().unwrap();
    let storage = Storage::new(dir.path().into()).unwrap();
    storage.save(management_data()).unwrap();
    let revision = storage.revision().unwrap();
    let job = ManagementJob::new();

    // Deny admission deterministically, without relying on a thread winning
    // a scheduling race or waiting for SQLite's busy timeout.
    let gate = storage.analysis_filter_gate.clone();
    let poisoned = std::thread::spawn(move || {
        let _writer = gate.lock().unwrap();
        panic!("simulate an unavailable filter writer queue");
    });
    assert!(poisoned.join().is_err());

    let result = storage.management_index_settings(revision, &job);
    assert!(job.records_processed() > 0, "staging should precede admission");
    assert!(result.is_err(), "publication bypassed writer admission");
    assert!(storage.management_bootstrap(revision).unwrap_err().contains("unindexed"));
    assert_eq!(storage.revision().unwrap(), revision);
}

#[test]
fn bulk_group_removal_is_revision_checked_and_preserves_source_data() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().join("live")).unwrap();
    let original = management_data();
    let rev = management_setup(&store, &original);
    let groups = store.management_group_page(rev, "", None, 50).unwrap();
    let selected = [groups[0].handle, groups[1].handle];

    assert!(store.unified_delete_groups(rev, Some(&selected), 2, None, false).is_err());
    assert!(store.unified_delete_groups(rev, Some(&[selected[0], i64::MAX]), 2, None, true).is_err());
    assert_eq!(store.revision().unwrap(), rev);
    assert_eq!(store.load().unwrap(), original);

    store.connect().unwrap().execute_batch(
        "CREATE TRIGGER reject_group_publication BEFORE INSERT ON kv
         WHEN NEW.name='settings' BEGIN SELECT RAISE(ABORT, 'group write failed'); END;",
    ).unwrap();
    assert!(store.unified_delete_groups(rev, Some(&selected), 2, None, true)
        .unwrap_err().contains("group write failed"));
    assert_eq!(store.revision().unwrap(), rev);
    assert_eq!(store.load().unwrap(), original);
    store.connect().unwrap().execute_batch("DROP TRIGGER reject_group_publication").unwrap();

    let receipt = store.unified_delete_groups(rev, Some(&selected), 2, None, true).unwrap();
    assert!(store.unified_delete_groups(rev, Some(&selected), 2, None, true).is_err());
    let mut expected = original.clone();
    expected["settings"]["groups"] = json!([original["settings"]["groups"][2]]);
    assert_eq!(store.load().unwrap(), expected);

    let rev = receipt.revision;
    let backup = dir.path().join("before-clear.sqlite3");
    assert!(store.unified_delete_groups(rev, None, 1, None, false).is_err());
    assert!(store.unified_delete_groups(rev, None, 2, Some(&backup), true).is_err());
    assert!(!backup.exists());
    let exported = dir.path().join("groups.json");
    store.unified_groups_export(rev, &exported).unwrap();
    store.unified_delete_groups(rev, None, 1, None, true).unwrap();
    assert!(!backup.exists());
    expected["settings"]["groups"] = json!([]);
    assert_eq!(store.load().unwrap(), expected);
    let rev = store.revision().unwrap();
    let preview = store.unified_groups_preview(rev, &exported).unwrap();
    store.unified_groups_import(rev, &exported, &preview.hash, true).unwrap();
    expected["settings"]["groups"] = json!([original["settings"]["groups"][2]]);
    assert_eq!(store.load().unwrap(), expected);
}

#[test]
fn deleting_selected_tree_nodes_keeps_surviving_descendants_linked() {
    let dir = tempdir().unwrap();
    let store = Storage::new(dir.path().into()).unwrap();
    let mut original = management_data();
    original["settings"]["groups"] = json!([
        {"id":"root","name":"Root","works":[]},
        {"id":"first","name":"First","works":[]},
        {"id":"second","name":"Second","works":[]},
        {"id":"leaf","name":"Leaf","works":["one"]}
    ]);
    original["settings"]["workGroupTreeParents"] = json!([
        {"child":"first","parent":"root"},
        {"child":"second","parent":"first"},
        {"child":"leaf","parent":"second"}
    ]);
    let revision = management_setup(&store, &original);
    let handles: Vec<_> = store.management_group_page(revision, "", None, 50).unwrap()
        .into_iter().filter(|group| matches!(group.id.as_str(), "first" | "second"))
        .map(|group| group.handle).collect();
    let changed = store.unified_delete_groups(revision, Some(&handles), 2, None, true).unwrap();
    let after = store.load().unwrap();
    assert_eq!(after["settings"]["workGroupTreeParents"], json!([{"child":"leaf","parent":"root"}]));
    assert_eq!(after["reports"], original["reports"]);

    let all = store.management_group_page(changed.revision, "", None, 50).unwrap();
    let root = all.iter().find(|group| group.id == "root").unwrap().handle;
    assert!(store.unified_group_tree_edit(changed.revision, GroupTreeEdit::Delete {
        group: root, delete_descendants: true, confirmed: false,
    }).is_err());
    assert_eq!(store.load().unwrap(), after);

    store.unified_group_tree_edit(changed.revision, GroupTreeEdit::Delete {
        group: root, delete_descendants: true, confirmed: true,
    }).unwrap();
    let cleared = store.load().unwrap();
    assert_eq!(cleared["settings"]["groups"], json!([]));
    assert_eq!(cleared["settings"]["workGroupTreeParents"], json!([]));
    assert_eq!(cleared["reports"], original["reports"]);
}
