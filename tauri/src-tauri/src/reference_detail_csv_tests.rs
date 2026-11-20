use super::*;

#[test]
fn detail_csv_streams_all_filtered_rows_and_preserves_the_selected_table_shape() {
    let (dir, store, revision, scope) = fixture();
    let mut q = query();
    q.to = "2024-01-02".into();
    q.from = q.to.clone();
    let before = json!({"from":"2024-01-01","to":"2024-01-01"});
    for section in [
        "comparison",
        "contributions",
        "organizations",
        "usual",
        "regularity",
        "structure",
    ] {
        let mut spec = json!({"section":section,"metric":"amount"});
        match section {
            "usual" | "regularity" => {
                spec["options"] = json!({"status":"all","search":""});
                spec["window"] = json!(4);
            }
            "organizations" => spec["baseOrg"] = json!("A"),
            _ => {
                spec["before"] = before.clone();
                if section == "structure" {
                    spec["limit"] = json!(10);
                }
            }
        }
        let table: DetailCsvOptions = serde_json::from_value(spec).unwrap();
        let path = dir.path().join(format!("{section}.csv"));
        store
            .reference_detail_csv(revision, &scope, &q, None, &table, &path)
            .unwrap();
        let data = std::fs::read_to_string(&path).unwrap();
        assert!(data.starts_with('\u{feff}'));
        assert!(data.contains("\"Подразделения\";\"A\""));
        let data_rows = data
            .lines()
            .filter(|line| line.starts_with("\"Работа ") && !line.starts_with("\"Работа /"))
            .count();
        if section == "structure" {
            assert!(data_rows <= 10);
            assert!(
                data.contains("Остальн") || data.contains("Прочие"),
                "{data}"
            );
        } else {
            assert!(data_rows > 50, "{section} exported {data_rows}");
            assert!(
                data.contains("\"ЯРКАЯ услуга\""),
                "missing final record in {section}"
            );
        }
        let original = std::fs::read(&path).unwrap();
        assert!(store
            .reference_detail_csv(revision, &scope, &q, None, &table, &path)
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}
#[test]
fn detail_csv_validates_scope_destination_and_formula_safe_filter_text() {
    let (dir, store, r, scope) = fixture();
    let q = query();
    let table:DetailCsvOptions=serde_json::from_value(json!({"section":"regularity","metric":"count","options":{"search":"=формула","status":"all"},"window":4})).unwrap();
    let path = dir.path().join("scope.csv");
    assert!(store
        .reference_detail_csv(r + 1, &scope, &q, None, &table, &path)
        .is_err());
    assert!(!path.exists());
    assert!(store
        .reference_detail_csv(r, &scope, &q, None, &table, Path::new("relative.csv"))
        .is_err());
    assert!(store
        .reference_detail_csv(
            r,
            &scope,
            &q,
            None,
            &table,
            &dir.path().join("report.sqlite3")
        )
        .is_err());
    store
        .reference_detail_csv(r, &scope, &q, None, &table, &path)
        .unwrap();
    let data = std::fs::read_to_string(&path).unwrap();
    assert!(data.contains("\"Поиск\";\"'=формула\""));
    assert!(!data.contains("ЯРКАЯ услуга"));
}

#[test]
fn unavailable_interval_preserves_only_the_complete_sides_values_on_screen_and_in_csv() {
    let (dir, store, r, scope) = fixture();
    let mut selection = query();
    selection.from = "2024-03-01".into();
    selection.to = selection.from.clone();
    let before = RangeQuery {
        from: "2024-01-01".into(),
        to: "2024-01-01".into(),
    };
    let cmp = ComparisonQuery {
        before: before.clone(),
        after: RangeQuery {
            from: selection.from.clone(),
            to: selection.to.clone(),
        },
        percent: 0.,
        min_amount: 0.,
        min_count: 0.,
    };
    let page = store
        .analysis_comparison_page(r, &scope, &cmp, None, None, 50)
        .unwrap();
    let first = page.iter().find(|x| x.name == "Работа 00").unwrap();
    assert!(first.amount.is_none());
    assert_eq!(first.available.as_ref().unwrap().amount, Some(-10.));
    let table: DetailCsvOptions =
        serde_json::from_value(json!({"section":"comparison","metric":"amount","before":before}))
            .unwrap();
    let path = dir.path().join("partial.csv");
    store
        .reference_detail_csv(r, &scope, &selection, None, &table, &path)
        .unwrap();
    let data = std::fs::read_to_string(path).unwrap();
    assert!(data.contains("\"Работа / группа\";\"Значение раньше\""));
    assert!(data.lines().any(|line| line == "\"Работа 00\";\"-10\""));
    assert!(!data.contains("Разница"));
}
