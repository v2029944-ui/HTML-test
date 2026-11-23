use super::*;
use serde_json::json;

fn data(name: &str) -> serde_json::Value {
    json!({"reports":[{"id":name,"type":"day","start":"2024-02-29","end":"2024-02-29","rows":[]}],"settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}})
}
fn runtime_fixture() -> (tempfile::TempDir, Runtime) {
    let root = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(root.path().to_owned()).unwrap();
    (root, runtime)
}
fn connect(runtime: &Runtime, folder: &Path) -> WorkspaceStatus {
    runtime.grant(folder.to_owned()).unwrap();
    runtime.connect(folder).unwrap()
}
fn read(runtime: &Runtime, token: &str) -> serde_json::Value {
    runtime
        .execute(Some(token), Access::Read, |store| store.load())
        .unwrap()
}

#[test]
fn personal_and_empty_team_are_independent_and_stale_tokens_cannot_cross_modes() {
    let (_root, runtime) = runtime_fixture();
    let personal = runtime.status().unwrap();
    runtime
        .execute(Some(&personal.workspace_token), Access::Write, |store| {
            store.save(data("personal"))
        })
        .unwrap();
    let shared = tempfile::tempdir().unwrap();
    let team = connect(&runtime, shared.path());
    assert_eq!(team.generation, Some(0));
    assert!(read(&runtime, &team.workspace_token)["reports"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(runtime
        .execute(Some(&personal.workspace_token), Access::Write, |store| {
            store.save(data("wrong"))
        })
        .is_err());
    runtime
        .execute(Some(&team.workspace_token), Access::Write, |store| {
            store.save(data("team"))
        })
        .unwrap();
    assert_eq!(runtime.status().unwrap().generation, Some(1));
    assert!(runtime.disconnect(Some(&personal.workspace_token)).is_err());
    assert_eq!(runtime.status().unwrap().mode, "shared");
    let current = runtime.status().unwrap();
    let personal = runtime.disconnect(Some(&current.workspace_token)).unwrap();
    assert_eq!(
        read(&runtime, &personal.workspace_token)["reports"][0]["id"],
        "personal"
    );
    let team = connect(&runtime, shared.path());
    assert_eq!(
        read(&runtime, &team.workspace_token)["reports"][0]["id"],
        "team"
    );
}

#[test]
fn other_client_publication_invalidates_selection_and_sync_pulls_latest() {
    let (_a, a) = runtime_fixture();
    let (_b, b) = runtime_fixture();
    let folder = tempfile::tempdir().unwrap();
    let first = connect(&a, folder.path());
    let stale = connect(&b, folder.path());
    a.execute(Some(&first.workspace_token), Access::Write, |store| {
        store.save(data("remote"))
    })
    .unwrap();
    let mut ran = false;
    assert!(b
        .execute(Some(&stale.workspace_token), Access::Write, |_| {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(!ran);
    let fresh = b.sync().unwrap();
    assert_ne!(stale.workspace_token, fresh.workspace_token);
    assert_eq!(
        read(&b, &fresh.workspace_token)["reports"][0]["id"],
        "remote"
    );
}

#[test]
fn failed_mutation_is_discarded_without_publication_and_local_config_is_not_shared() {
    let (_a, a) = runtime_fixture();
    let (_b, b) = runtime_fixture();
    let folder = tempfile::tempdir().unwrap();
    let team = connect(&a, folder.path());
    assert!(a
        .execute(Some(&team.workspace_token), Access::Write, |store| {
            store.save(data("partial"))?;
            Err::<(), _>("later validation failed".into())
        })
        .is_err());
    assert_eq!(a.status().unwrap().generation, Some(0));
    assert!(read(&a, &team.workspace_token)["reports"]
        .as_array()
        .unwrap()
        .is_empty());
    let excel = tempfile::tempdir().unwrap();
    a.execute(Some(&team.workspace_token), Access::Local, |store| {
        store.set_folder(excel.path().to_string_lossy().to_string())
    })
    .unwrap();
    a.execute(Some(&team.workspace_token), Access::Write, |store| {
        store.save(data("remote"))
    })
    .unwrap();
    let peer = connect(&b, folder.path());
    assert!(b
        .execute(Some(&peer.workspace_token), Access::Read, |store| store
            .info())
        .unwrap()["folder"]
        .is_null());
    assert_eq!(
        a.execute(Some(&team.workspace_token), Access::Read, |store| store
            .info())
            .unwrap()["folder"],
        excel.path().canonicalize().unwrap().to_string_lossy().as_ref()
    );
}

#[test]
fn selected_share_offline_at_restart_never_falls_back_to_personal_and_reconnects_explicitly() {
    let (root, a) = runtime_fixture();
    let personal = a.status().unwrap();
    a.execute(Some(&personal.workspace_token), Access::Write, |store| {
        store.save(data("personal"))
    })
    .unwrap();
    let parent = tempfile::tempdir().unwrap();
    let folder = parent.path().join("team");
    fs::create_dir(&folder).unwrap();
    connect(&a, &folder);
    drop(a);
    let away = parent.path().join("away");
    fs::rename(&folder, &away).unwrap();
    let restarted = Runtime::new(root.path().to_owned()).unwrap();
    let status = restarted.status().unwrap();
    assert_eq!(status.mode, "shared");
    assert!(!status.available);
    assert!(restarted
        .execute(Some(&status.workspace_token), Access::Read, |store| store
            .load())
        .is_err());
    fs::rename(&away, &folder).unwrap();
    let recovered = restarted.sync().unwrap();
    assert!(recovered.available);
    assert!(read(&restarted, &recovered.workspace_token)["reports"]
        .as_array()
        .unwrap()
        .is_empty());
    let personal = restarted.disconnect(Some(&recovered.workspace_token)).unwrap();
    assert_eq!(
        read(&restarted, &personal.workspace_token)["reports"][0]["id"],
        "personal"
    );
}

#[test]
fn closing_window_cancels_its_auto_import_lease_while_runtime_is_busy() {
    use std::sync::{mpsc, Arc};
    use std::time::Duration;
    let (_root, runtime) = runtime_fixture();
    let runtime = Arc::new(runtime);
    let token = runtime.status().unwrap().workspace_token;
    runtime
        .execute(Some(&token), Access::Local, |store| {
            store.set_folder_import_session("main", true)
        })
        .unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let reader = Arc::clone(&runtime);
    let token2 = token.clone();
    let busy = std::thread::spawn(move || {
        reader.execute(Some(&token2), Access::Read, |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        })
    });
    entered_rx.recv().unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let closer = Arc::clone(&runtime);
    let closing = std::thread::spawn(move || {
        closer.close_window("main");
        done_tx.send(()).unwrap();
    });
    let cancelled_without_waiting = done_rx.recv_timeout(Duration::from_millis(250)).is_ok();
    release_tx.send(()).unwrap();
    busy.join().unwrap().unwrap();
    closing.join().unwrap();
    assert!(
        cancelled_without_waiting,
        "window teardown must not wait behind a running import"
    );
    runtime
        .execute(Some(&token), Access::Read, |store| {
            assert!(store.folder_import.lock().unwrap().is_none());
            Ok(())
        })
        .unwrap();
}

#[test]
fn confirmed_share_cannot_silently_roll_back_to_empty_after_poll_restart_or_reconnect() {
    let (root, runtime) = runtime_fixture();
    let folder = tempfile::tempdir().unwrap();
    let team = connect(&runtime, folder.path());
    runtime
        .execute(Some(&team.workspace_token), Access::Write, |store| {
            store.save(data("confirmed"))
        })
        .unwrap();
    let snapshot = fs::read_dir(folder.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|s| s == "snapshot"))
        .unwrap();
    let bytes = fs::read(&snapshot).unwrap();
    fs::remove_file(&snapshot).unwrap();
    let missing = runtime.sync().unwrap();
    assert!(
        !missing.available,
        "a disappeared generation is not a new empty team"
    );
    assert!(missing.error.unwrap().contains("поколен"));
    drop(runtime);
    let restarted = Runtime::new(root.path().to_owned()).unwrap();
    assert!(!restarted.status().unwrap().available);
    restarted.grant(folder.path().to_owned()).unwrap();
    assert!(restarted.connect(folder.path()).is_err());
    assert!(!restarted.status().unwrap().available);
    fs::write(snapshot, bytes).unwrap();
    let restored = restarted.sync().unwrap();
    assert!(restored.available);
    assert_eq!(
        read(&restarted, &restored.workspace_token)["reports"][0]["id"],
        "confirmed"
    );
}

#[test]
fn durable_pending_retries_after_restart_without_reapplying_the_mutation() {
    let (root, runtime) = runtime_fixture();
    let parent = tempfile::tempdir().unwrap();
    let folder = parent.path().join("team");
    let away = parent.path().join("away");
    fs::create_dir(&folder).unwrap();
    let team = connect(&runtime, &folder);
    let result = runtime.execute(Some(&team.workspace_token), Access::Write, |store| {
        store.save(data("once"))?;
        fs::rename(&folder, &away).unwrap();
        Ok(())
    });
    assert!(result.is_err());
    assert!(runtime.status().unwrap().pending);
    drop(runtime);
    let restarted = Runtime::new(root.path().to_owned()).unwrap();
    assert_eq!(restarted.status().unwrap().mode, "shared");
    assert!(restarted.status().unwrap().pending);
    assert!(!restarted.status().unwrap().available);
    fs::rename(&away, &folder).unwrap();
    let synced = restarted.sync().unwrap();
    assert!(!synced.pending);
    assert!(synced.available);
    assert_eq!(synced.generation, Some(1));
    assert_eq!(
        read(&restarted, &synced.workspace_token)["reports"][0]["id"],
        "once"
    );
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
}

#[test]
fn remapped_share_with_same_or_higher_generation_does_not_replace_the_trusted_team() {
    for other_generations in [1, 2] {
        let (_root, runtime) = runtime_fixture();
        let (_other_root, other) = runtime_fixture();
        let parent = tempfile::tempdir().unwrap();
        let folder = parent.path().join("team");
        let replacement = parent.path().join("replacement");
        fs::create_dir(&folder).unwrap();
        fs::create_dir(&replacement).unwrap();
        let selected = connect(&runtime, &folder);
        runtime
            .execute(Some(&selected.workspace_token), Access::Write, |store| {
                store.save(data("trusted"))
            })
            .unwrap();
        let peer = connect(&other, &replacement);
        for _ in 0..other_generations {
            other
                .execute(Some(&peer.workspace_token), Access::Write, |store| {
                    store.save(data("different-team"))
                })
                .unwrap();
        }
        fs::rename(&folder, parent.path().join("original")).unwrap();
        fs::rename(&replacement, &folder).unwrap();
        let status = runtime.sync().unwrap();
        assert!(
            !status.available,
            "a remapped share is not the same trusted team even when its generation is higher"
        );
        assert!(runtime
            .execute(Some(&status.workspace_token), Access::Read, |store| store
                .load())
            .is_err());
    }
}

#[test]
fn competing_publication_is_not_success_and_explicit_discard_keeps_remote_winner() {
    let (_a, a) = runtime_fixture();
    let (_b, b) = runtime_fixture();
    let folder = tempfile::tempdir().unwrap();
    let client = connect(&a, folder.path());
    let peer = connect(&b, folder.path());
    let result = a.execute(Some(&client.workspace_token), Access::Write, |store| {
        store.save(data("unpublished"))?;
        b.execute(Some(&peer.workspace_token), Access::Write, |other| {
            other.save(data("winner"))
        })?;
        Ok(())
    });
    assert!(result.is_err());
    let pending = a.status().unwrap();
    assert!(pending.pending);
    assert!(!pending.available);
    assert!(a
        .execute(Some(&pending.workspace_token), Access::Read, |store| store
            .load())
        .is_err());
    let still_pending = a.sync().unwrap();
    assert!(still_pending.pending);
    let refreshed = a
        .discard_pending(Some(&still_pending.workspace_token))
        .unwrap();
    assert!(!refreshed.pending);
    assert!(refreshed.available);
    assert_eq!(refreshed.generation, Some(1));
    assert_eq!(
        read(&a, &refreshed.workspace_token)["reports"][0]["id"],
        "winner"
    );
    let state = a.state.lock().unwrap();
    let cache = a.cache_dir(&state).unwrap();
    assert!(fs::read_dir(cache).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("abandoned-")));
    assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 1);
}

#[test]
fn concurrent_local_mutations_are_serialized_and_publish_each_latest_state() {
    use std::sync::{Arc, Barrier};
    let (_root, runtime) = runtime_fixture();
    let folder = tempfile::tempdir().unwrap();
    let selected = connect(&runtime, folder.path());
    let runtime = Arc::new(runtime);
    let barrier = Arc::new(Barrier::new(2));
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for id in ["one", "two"] {
            let runtime = Arc::clone(&runtime);
            let barrier = Arc::clone(&barrier);
            let token = selected.workspace_token.clone();
            workers.push(scope.spawn(move || {
                barrier.wait();
                runtime.execute(Some(&token), Access::Write, |store| {
                    let mut value = store.load()?;
                    value["reports"]
                        .as_array_mut()
                        .unwrap()
                        .push(data(id)["reports"][0].clone());
                    store.save(value)
                })
            }));
        }
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
    });
    assert_eq!(runtime.status().unwrap().generation, Some(2));
    let value = read(&runtime, &selected.workspace_token);
    let mut ids: Vec<_> = value["reports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|report| report["id"].as_str().unwrap())
        .collect();
    ids.sort();
    assert_eq!(ids, ["one", "two"]);
}

#[test]
fn ungranted_folder_and_missing_workspace_token_never_run_data_commands() {
    let (_root, runtime) = runtime_fixture();
    let folder = tempfile::tempdir().unwrap();
    assert!(runtime.connect(folder.path()).is_err());
    assert_eq!(runtime.status().unwrap().mode, "personal");
    let mut ran = false;
    assert!(runtime
        .execute(None, Access::Write, |_| {
            ran = true;
            Ok(())
        })
        .is_err());
    assert!(!ran);
    assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 0);
}

#[test]
fn personal_reports_transfer_only_after_review_and_leave_source_unchanged() {
    let (_root, runtime) = runtime_fixture();
    let private = runtime.status().unwrap();
    runtime.execute(Some(&private.workspace_token), Access::Write, |store| {
        store.save(json!({"reports":[
            {"id":"local-day-1","type":"day","start":"2024-02-28","end":"2024-02-28",
             "desktopFolder":"C:\\personal\\excel","rows":[{"organization":"A","work":"Форсунка","count":2,"amount":300}]},
            {"id":"local-day-2","type":"day","start":"2024-02-29","end":"2024-02-29","rows":[]}
        ],"settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}}))
    }).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let shared = connect(&runtime, folder.path());
    let preview = runtime.preview_personal_transfer(Some(&shared.workspace_token),0).unwrap();
    assert_eq!(preview.total_reports,2);
    assert_eq!(preview.rows.len(),2);
    assert!(preview.rows.iter().all(|r|r.status=="ready"));
    let receipt = runtime.apply_personal_transfer(
        Some(&shared.workspace_token),
        preview.source_revision,preview.target_revision,
        vec!["local-day-1".into(),"local-day-2".into()],
    ).unwrap();
    assert_eq!(receipt.imported,2);
    assert!(std::path::Path::new(&receipt.backup_path).exists());
    assert_eq!(runtime.status().unwrap().generation,Some(1));
    let team_data = read(&runtime,&shared.workspace_token);
    assert_eq!(team_data["reports"].as_array().unwrap().len(),2);
    assert!(team_data["reports"][0].get("desktopFolder").is_none());
    assert_eq!(team_data["settings"]["orgNames"],json!(["A","B","C","D"]),
        "a new empty team must inherit the organization names needed for its first reports");
    assert_eq!(team_data["settings"]["groups"],json!([]),
        "personal group definitions must not be published implicitly");
    let post = runtime.preview_personal_transfer(Some(&shared.workspace_token),0).unwrap();
    assert!(post.rows.iter().all(|r|r.status=="duplicate"));
    assert!(runtime.apply_personal_transfer(
        Some(&shared.workspace_token),post.source_revision,post.target_revision,
        vec!["local-day-1".into()]
    ).is_err());
    assert_eq!(runtime.status().unwrap().generation,Some(1));
    let private = runtime.disconnect(Some(&shared.workspace_token)).unwrap();
    let originals=read(&runtime,&private.workspace_token);
    assert_eq!(originals["reports"].as_array().unwrap().len(),2);
    assert!(originals["reports"][0].get("desktopFolder").is_some());
}
#[test]
fn personal_transfer_rejects_conflicts_and_stale_reviews_without_writing() {
    let (_root,runtime)=runtime_fixture();
    let private=runtime.status().unwrap();
    runtime.execute(Some(&private.workspace_token),Access::Write,|s|s.save(data("personal"))).unwrap();
    let folder=tempfile::tempdir().unwrap();
    let team=connect(&runtime,folder.path());
    let preview=runtime.preview_personal_transfer(Some(&team.workspace_token),0).unwrap();
    let selected=vec!["personal".to_string()];
    assert!(runtime.apply_personal_transfer(
        Some(&private.workspace_token),preview.source_revision,preview.target_revision,selected.clone()
    ).is_err(),"stale personal mode token must not authorize transfer");
    runtime.execute(Some(&team.workspace_token),Access::Write,|s|s.save(data("shared"))).unwrap();
    assert!(runtime.apply_personal_transfer(
        Some(&team.workspace_token),preview.source_revision,preview.target_revision,selected.clone()
    ).is_err(),"stale destination revision must not publish");
    let conflict=runtime.preview_personal_transfer(Some(&team.workspace_token),0).unwrap();
    assert_eq!(conflict.rows[0].status,"conflict");
    assert!(runtime.apply_personal_transfer(
        Some(&team.workspace_token),conflict.source_revision,conflict.target_revision,selected
    ).is_err(),"cross-id overlapping date must not silently replace");
    assert_eq!(runtime.status().unwrap().generation,Some(1));
    assert_eq!(read(&runtime,&team.workspace_token)["reports"][0]["id"],"shared");
}
#[test]
fn personal_transfer_publishes_to_a_second_client_without_reimporting_excel() {
    let (_root,first)=runtime_fixture();
    let (_root2,second)=runtime_fixture();
    let private=first.status().unwrap();
    first.execute(Some(&private.workspace_token),Access::Write,|s|s.save(data("from-excel"))).unwrap();
    let folder=tempfile::tempdir().unwrap();
    let team=connect(&first,folder.path());
    let stale=connect(&second,folder.path());
    let preview=first.preview_personal_transfer(Some(&team.workspace_token),0).unwrap();
    first.apply_personal_transfer(Some(&team.workspace_token),
        preview.source_revision,preview.target_revision,vec!["from-excel".into()]).unwrap();
    assert!(second.execute(Some(&stale.workspace_token),Access::Read,|s|s.load()).is_err());
    let fresh=second.sync().unwrap();
    assert_eq!(read(&second,&fresh.workspace_token)["reports"][0]["id"],"from-excel");
}

#[test]
fn personal_transfer_keeps_linked_wide_records_together_and_rebuilds_rows() {
    let (_root,runtime)=runtime_fixture();
    let initial=runtime.status().unwrap();
    runtime.execute(Some(&initial.workspace_token),Access::Write,|s|s.save(json!({
        "reports":[
            {"id":"wide-parent","type":"day","start":"2024-03-01","end":"2024-03-01","rows":[
                {"organization":"A","work":"Ремонт форсунок","count":2.5,"amount":300.75}
            ]},
            {"id":"wide-child","type":"day","start":"2024-03-01","end":"2024-03-01","rows":[
                {"organization":"B","work":"Ремонт ТНВД","count":3,"amount":390}
            ]}
        ],"settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}
    }))).unwrap();
    {
        let state=runtime.state.lock().unwrap();
        state.personal.connect().unwrap().execute(
            "INSERT INTO report_source_diagnostic_links(report_id,anchor_id) VALUES('wide-child','wide-parent')",[]
        ).unwrap();
    }
    let folder=tempfile::tempdir().unwrap();
    let shared=connect(&runtime,folder.path());
    let preview=runtime.preview_personal_transfer(Some(&shared.workspace_token),0).unwrap();
    assert_eq!(preview.rows.len(),2);
    assert!(runtime.apply_personal_transfer(
        Some(&shared.workspace_token),preview.source_revision,preview.target_revision,
        vec!["wide-child".into()]
    ).unwrap_err().contains("Связанные диагностические"));
    assert_eq!(runtime.status().unwrap().generation,Some(0));
    runtime.apply_personal_transfer(
        Some(&shared.workspace_token),preview.source_revision,preview.target_revision,
        vec!["wide-child".into(),"wide-parent".into()]
    ).unwrap();
    let state=runtime.state.lock().unwrap();
    let conn=state.checkout.as_ref().unwrap().storage.connect().unwrap();
    let n:i64=conn.query_row("SELECT COUNT(*) FROM report_rows WHERE report_id IN ('wide-child','wide-parent')",[],|r|r.get(0)).unwrap();
    assert_eq!(n,2);
    let links:i64=conn.query_row("SELECT COUNT(*) FROM report_source_diagnostic_links WHERE report_id='wide-child' AND anchor_id='wide-parent'",[],|r|r.get(0)).unwrap();
    assert_eq!(links,1);
}

#[test]
fn personal_transfer_does_not_block_different_report_types_with_overlapping_dates() {
    let (_root, runtime) = runtime_fixture();
    let personal = runtime.status().unwrap();
    runtime.execute(Some(&personal.workspace_token), Access::Write, |store| {
        store.save(data("individual-day"))
    }).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let shared = connect(&runtime, folder.path());
    runtime.execute(Some(&shared.workspace_token), Access::Write, |store| {
        store.save(json!({"reports":[{
            "id":"existing-week","type":"week","start":"2024-02-26",
            "end":"2024-03-03","rows":[]
        }],"settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}}))
    }).unwrap();
    let page = runtime.preview_personal_transfer(Some(&shared.workspace_token), 0).unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].status, "ready");
    runtime.apply_personal_transfer(
        Some(&shared.workspace_token), page.source_revision, page.target_revision,
        vec!["individual-day".into()],
    ).unwrap();
    let ids = read(&runtime, &shared.workspace_token)["reports"].as_array()
        .unwrap().iter().map(|r|r["id"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"existing-week".to_string()));
    assert!(ids.contains(&"individual-day".to_string()));
}

#[test]
fn personal_transfer_blocks_an_unknown_organization_without_changing_team_data() {
    let (_root, runtime) = runtime_fixture();
    let personal = runtime.status().unwrap();
    runtime.execute(Some(&personal.workspace_token), Access::Write, |store| {
        store.save(json!({"reports":[{"id":"local-a","type":"day",
            "start":"2024-02-28","end":"2024-02-28",
            "rows":[{"organization":"A","work":"Ремонт","count":1,"amount":100}]
        }],"settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}}))
    }).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let team = connect(&runtime, folder.path());
    runtime.execute(Some(&team.workspace_token), Access::Write, |store| {
        store.save(json!({"reports":[{"id":"team-x","type":"day",
            "start":"2024-03-01","end":"2024-03-01","rows":[]
        }],"settings":{"orgNames":["X","Y","Z","W"],"groups":[],"catalog":[]}}))
    }).unwrap();
    let before = runtime.status().unwrap().generation;
    let preview = runtime.preview_personal_transfer(Some(&team.workspace_token), 0).unwrap();
    assert_eq!(preview.rows[0].status,"organization_conflict");
    assert!(preview.organization_warning.is_some());
    let error = runtime.apply_personal_transfer(
        Some(&team.workspace_token), preview.source_revision, preview.target_revision,
        vec!["local-a".into()]
    ).unwrap_err();
    assert!(error.contains("Подразделение"),"{error}");
    assert_eq!(runtime.status().unwrap().generation,before);
    let data = read(&runtime,&team.workspace_token);
    assert_eq!(data["reports"].as_array().unwrap().len(),1);
    assert_eq!(data["settings"]["orgNames"],json!(["X","Y","Z","W"]));
}

#[test]
fn personal_wide_source_anchor_can_be_moved_before_linked_followup_batch() {
    let (_root,runtime)=runtime_fixture();
    let personal=runtime.status().unwrap();
    runtime.execute(Some(&personal.workspace_token),Access::Write,|s|s.save(json!({
        "reports":[
          {"id":"source-anchor","type":"day","start":"2024-04-01","end":"2024-04-01",
           "rows":[{"organization":"A","work":"Контроль","count":1,"amount":1}]},
          {"id":"linked-day","type":"day","start":"2024-04-02","end":"2024-04-02",
           "rows":[{"organization":"A","work":"Контроль","count":1,"amount":1}]}
        ],"settings":{"orgNames":["A","B","C","D"],"groups":[],"catalog":[]}
    }))).unwrap();
    {
        let state=runtime.state.lock().unwrap();
        state.personal.connect().unwrap().execute(
            "INSERT INTO report_source_diagnostic_links(report_id,anchor_id) VALUES('linked-day','source-anchor')",[]
        ).unwrap();
    }
    let folder=tempfile::tempdir().unwrap();
    let team=connect(&runtime,folder.path());
    let first=runtime.preview_personal_transfer(Some(&team.workspace_token),0).unwrap();
    runtime.apply_personal_transfer(
        Some(&team.workspace_token),first.source_revision,first.target_revision,
        vec!["source-anchor".into()]
    ).unwrap();
    let second=runtime.preview_personal_transfer(Some(&team.workspace_token),0).unwrap();
    assert_eq!(second.rows.iter().find(|r|r.id=="source-anchor").unwrap().status,"duplicate");
    runtime.apply_personal_transfer(
        Some(&team.workspace_token),second.source_revision,second.target_revision,
        vec!["linked-day".into()]
    ).unwrap();
    let state=runtime.state.lock().unwrap();
    let connection=state.checkout.as_ref().unwrap().storage.connect().unwrap();
    let refs:i64=connection.query_row(
        "SELECT count(*) FROM report_source_diagnostic_links WHERE report_id='linked-day' AND anchor_id='source-anchor'",
        [],|r|r.get(0)
    ).unwrap();
    assert_eq!(refs,1);
    assert_eq!(runtime.status().is_ok(),true);
}
