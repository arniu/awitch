use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::pointing::apps::declared;
use crate::pointing::doc::Doc;
use crate::pointing::record::{Edit, FileEdits};
use crate::pointing::schema::{App, File};
use crate::pointing::{Instance, State, Target};

/// A scratch config root for one test case: a fresh dir under the system
/// temp, so pointing never touches the real agent config (sandbox guardrail).
/// `tag` must be unique per case — tests share the pid.
fn temp_config_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("awitch-point-test-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The gateway a point aims at.
fn target() -> Target {
    Target {
        url: "http://127.0.0.1:10689".into(),
        key: "0123456789abcdef0123456789abcdef".into(),
    }
}

/// The gateway a user's own config already points at: what a point overwrites
/// and what an undo puts back.
fn other_target() -> Target {
    Target {
        url: "http://127.0.0.1:9999".into(),
        key: "the-user-own-key".into(),
    }
}

fn instance(app: &'static App, dir: &Path) -> Instance {
    app.bind(dir).with_home(dir)
}

/// A key of the user's own: no spec manages it, so no operation may touch it.
const USER_KEY: &str = "awitch_point_test_user_key";

fn read_doc_of(instance: &Instance, file: &File) -> Doc {
    super::read_doc(&instance.file_path(file)).unwrap()
}

fn write_doc_of(instance: &Instance, file: &File, doc: &Doc) {
    super::write_doc(&instance.file_path(file), doc).unwrap();
}

/// Point every doc at `t` with the spec's own values — a doc that already
/// points elsewhere — and leave the user's own key beside them.
fn seed_pointed_docs(instance: &Instance, t: &Target) {
    for file in instance.app.files {
        assert_no_collision(instance.app, file);
        let mut doc = read_doc_of(instance, file);
        for patch in file.patches {
            doc.set(patch.key_path, patch.value.render(t).unwrap())
                .unwrap();
        }
        doc.set(USER_KEY, Value::String(USER_KEY.into())).unwrap();
        write_doc_of(instance, file, &doc);
    }
}

/// The seeding guard: the user's key must not sit on a path a spec manages,
/// or "the user's own content" would be what the spec writes.
fn assert_no_collision(app: &App, file: &File) {
    for patch in file.patches {
        assert!(
            patch.key_path != USER_KEY && !patch.key_path.starts_with(&format!("{USER_KEY}.")),
            "{}: user key {USER_KEY} collides with managed key '{}'",
            app.name,
            patch.key_path
        );
    }
}

fn assert_user_key(instance: &Instance, file: &File, doc: &Doc) {
    assert_eq!(
        doc.get(USER_KEY),
        Some(&Value::String(USER_KEY.into())),
        "{}: {} kept the user's own key",
        instance.app.name,
        file.path
    );
}

// ---- spec §3: state construction over C × R ----

/// One row of the spec's transfer table (spec §4): how the app's docs (C) and
/// the pointing record (R) stand before an operation runs.
#[derive(Debug, Clone, Copy)]
enum Scenario {
    /// C absent, R absent: untouched.
    A,
    /// C present, R absent: docs still pointed at the gateway, no record of
    /// them — the loss the read-back exists for.
    B,
    /// C absent, R complete: pointed, then the docs removed.
    C,
    /// C present, R complete: pointed.
    D,
    /// C absent, R corrupt: pointed, record corrupted, docs removed.
    E,
    /// C present, R corrupt: pointed, record corrupted.
    F,
}

fn build_state(instance: &Instance, s: Scenario) {
    match s {
        Scenario::A => {}
        Scenario::B => {
            instance.point(&target()).unwrap();
            instance.record_file.clear().unwrap();
        }
        Scenario::C => {
            instance.point(&target()).unwrap();
            remove_docs(instance);
        }
        Scenario::D => instance.point(&target()).unwrap(),
        Scenario::E => {
            instance.point(&target()).unwrap();
            tamper_record(instance);
            remove_docs(instance);
        }
        Scenario::F => {
            instance.point(&target()).unwrap();
            tamper_record(instance);
        }
    }
}

fn remove_docs(instance: &Instance) {
    for file in instance.app.files {
        let _ = std::fs::remove_file(instance.file_path(file));
    }
}

fn tamper_record(instance: &Instance) {
    let path = &instance.record_file.path;
    let raw = std::fs::read_to_string(path).unwrap();
    let key = instance.app.files[0].patches[0].key_path;
    let needle = format!("key_path = \"{key}\"");
    let tampered = raw.replace(&needle, &format!("key_path = \"{key} \""));
    assert_ne!(
        tampered, raw,
        "{}: tamper must change the record",
        instance.app.name
    );
    std::fs::write(path, tampered).unwrap();
}

/// The record's files: None when the file is absent or unverifiable.
fn record(instance: &Instance) -> Option<Vec<FileEdits>> {
    instance
        .record_file
        .read()
        .unwrap()
        .and_then(|text| super::record::decode(&text))
}

fn record_complete(instance: &Instance) -> bool {
    record(instance).is_some()
}

fn record_absent(instance: &Instance) -> bool {
    instance.record_file.read().unwrap().is_none()
}

/// Pointed, and the target reads back out of the live docs — the write went to
/// disk and the read-back walk agrees with what it wrote.
fn assert_pointed(instance: &Instance) {
    let report = instance.check().unwrap();
    assert_eq!(report.state, State::Normal, "{}", instance.app.name);
    assert_eq!(
        report.target,
        Some(target()),
        "{}: the docs read back as the target",
        instance.app.name
    );
}

/// Unpointed, whatever residue the docs may still hold (spec §check: an
/// unrecorded doc reads back as a target, never as a state).
fn assert_unpointed(instance: &Instance) {
    let report = instance.check().unwrap();
    assert_eq!(report.state, State::Unpointed, "{}", instance.app.name);
}

// ---- spec §4: the transfer table, through the operations ----

#[test]
fn point_transfers_per_the_spec_table() {
    for app in declared() {
        for s in [Scenario::A, Scenario::B, Scenario::C, Scenario::D] {
            let dir = temp_config_dir(&format!("point-{}-{s:?}", app.name));
            let instance = instance(app, &dir);
            build_state(&instance, s);
            instance.point(&target()).unwrap();
            assert!(record_complete(&instance), "{} {s:?}: record", app.name);
            assert!(instance.is_installed(), "{} {s:?}: docs", app.name);
            let _ = std::fs::remove_dir_all(&dir);
        }
        for s in [Scenario::E, Scenario::F] {
            let dir = temp_config_dir(&format!("point-{}-{s:?}", app.name));
            let instance = instance(app, &dir);
            build_state(&instance, s);
            let err = instance.point(&target()).unwrap_err();
            assert!(
                err.to_string().contains("corrupt"),
                "{} {s:?}: {err}",
                app.name
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

#[test]
fn undo_transfers_per_the_spec_table() {
    for app in declared() {
        for s in [Scenario::A, Scenario::B] {
            let dir = temp_config_dir(&format!("undo-{}-{s:?}", app.name));
            let instance = instance(app, &dir);
            build_state(&instance, s);
            assert!(!instance.undo().unwrap(), "{} {s:?}", app.name);
            let _ = std::fs::remove_dir_all(&dir);
        }

        for s in [Scenario::C, Scenario::D] {
            let dir = temp_config_dir(&format!("undo-{}-{s:?}", app.name));
            let instance = instance(app, &dir);
            build_state(&instance, s);
            assert!(instance.undo().unwrap(), "{} {s:?}", app.name);
            assert!(
                record_absent(&instance),
                "{} {s:?}: record cleared",
                app.name
            );
            assert_eq!(
                instance.is_installed(),
                matches!(s, Scenario::D),
                "{} {s:?}: docs",
                app.name
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
        for s in [Scenario::E, Scenario::F] {
            let dir = temp_config_dir(&format!("undo-{}-{s:?}", app.name));
            let instance = instance(app, &dir);
            build_state(&instance, s);
            assert!(
                instance.undo().is_err(),
                "{} {s:?}: corrupt must error",
                app.name
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

#[test]
fn reset_transfers_per_the_spec_table() {
    for app in declared() {
        for s in [Scenario::A, Scenario::B, Scenario::C, Scenario::D] {
            let dir = temp_config_dir(&format!("reset-{}-{s:?}", app.name));
            let instance = instance(app, &dir);
            build_state(&instance, s);
            assert!(!instance.reset().unwrap(), "{} {s:?}", app.name);
            let _ = std::fs::remove_dir_all(&dir);
        }
        for s in [Scenario::E, Scenario::F] {
            let dir = temp_config_dir(&format!("reset-{}-{s:?}", app.name));
            let instance = instance(app, &dir);
            build_state(&instance, s);
            assert!(instance.reset().unwrap(), "{} {s:?}", app.name);
            assert!(
                record_absent(&instance),
                "{} {s:?}: record cleared",
                app.name
            );
            if matches!(s, Scenario::F) {
                assert!(
                    instance.is_installed(),
                    "{} {s:?}: docs untouched",
                    app.name
                );
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

#[test]
fn check_states_per_the_spec_table() {
    for app in declared() {
        for s in [Scenario::A, Scenario::B] {
            let dir = temp_config_dir(&format!("check-{}-{s:?}", app.name));
            let instance = instance(app, &dir);
            build_state(&instance, s);
            let st = instance.check().unwrap();
            assert_eq!(st.state, State::Unpointed, "{} {s:?}", app.name);
            // The target is read back from the docs alone, record or no
            // record (spec §check): row B keeps the residue visible.
            assert_eq!(
                st.target,
                matches!(s, Scenario::B).then(target),
                "{} {s:?}: target read-back",
                app.name
            );
            let _ = std::fs::remove_dir_all(&dir);
        }

        for s in [Scenario::C, Scenario::E, Scenario::F] {
            let dir = temp_config_dir(&format!("check-{}-{s:?}", app.name));
            let instance = instance(app, &dir);
            build_state(&instance, s);
            assert_eq!(
                instance.check().unwrap().state,
                State::Unrecoverable,
                "{} {s:?}",
                app.name
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
        let dir = temp_config_dir(&format!("check-{}-D", app.name));
        let instance = instance(app, &dir);
        build_state(&instance, Scenario::D);
        let st = instance.check().unwrap();
        assert_eq!(st.state, State::Normal, "{} D", app.name);
        assert_eq!(
            st.target,
            Some(target()),
            "{} D: target read-back",
            app.name
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn check_is_pointed_across_the_spec_table() {
    for app in declared() {
        for s in [Scenario::A, Scenario::B] {
            let dir = temp_config_dir(&format!("pointed-{}-{s:?}", app.name));
            let instance = instance(app, &dir);
            build_state(&instance, s);
            assert!(
                !instance.check().unwrap().state.is_pointed(),
                "{} {s:?}",
                app.name
            );
            let _ = std::fs::remove_dir_all(&dir);
        }

        for s in [Scenario::C, Scenario::D, Scenario::E, Scenario::F] {
            let dir = temp_config_dir(&format!("pointed-{}-{s:?}", app.name));
            let instance = instance(app, &dir);
            build_state(&instance, s);
            assert!(
                instance.check().unwrap().state.is_pointed(),
                "{} {s:?}",
                app.name
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

// ---- what the operations write, over the whole registry ----

#[test]
fn point_writes_the_spec_values_and_leaves_the_user_content_alone() {
    for app in declared() {
        let dir = temp_config_dir(&format!("managed-{}", app.name));
        let instance = instance(app, &dir);
        seed_pointed_docs(&instance, &other_target());

        instance.point(&target()).unwrap();
        assert_pointed(&instance);

        // Every managed key carries the spec's rendering of the target — the
        // paths come from the schema, so this asserts awitch against itself,
        // never an app's format from the outside.
        for file in app.files {
            let doc = read_doc_of(&instance, file);
            for patch in file.patches {
                assert_eq!(
                    doc.get(patch.key_path),
                    Some(&patch.value.render(&target()).unwrap()),
                    "{}: {}.{}",
                    app.name,
                    file.path,
                    patch.key_path
                );
            }
            assert_user_key(&instance, file, &doc);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn point_records_what_it_replaced() {
    for app in declared() {
        let dir = temp_config_dir(&format!("record-{}", app.name));
        let instance = instance(app, &dir);
        seed_pointed_docs(&instance, &other_target());

        instance.point(&target()).unwrap();

        let Some(files) = record(&instance) else {
            panic!("{}: record must be complete", app.name);
        };
        for file in app.files {
            let bucket = files
                .iter()
                .find(|f| f.path == file.path)
                .unwrap_or_else(|| panic!("{}: no bucket for {}", app.name, file.path));
            assert_eq!(
                bucket.edits.len(),
                file.patches.len(),
                "{}: one edit per managed key of {}",
                app.name,
                file.path
            );
            for patch in file.patches {
                let edit = bucket
                    .edits
                    .iter()
                    .find(|e| e.key_path == patch.key_path)
                    .unwrap();
                assert_eq!(
                    edit.old_value,
                    Some(patch.value.render(&other_target()).unwrap()),
                    "{}: old value of {}",
                    app.name,
                    patch.key_path
                );
                assert_eq!(
                    edit.value,
                    patch.value.render(&target()).unwrap(),
                    "{}: new value of {}",
                    app.name,
                    patch.key_path
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn undo_restores_the_original_values() {
    for app in declared() {
        let dir = temp_config_dir(&format!("restore-{}", app.name));
        let instance = instance(app, &dir);
        seed_pointed_docs(&instance, &other_target());
        instance.point(&target()).unwrap();

        instance.undo().unwrap();
        let report = instance.check().unwrap();
        assert_eq!(report.state, State::Unpointed, "{}", app.name);
        // The docs are back at the user's own gateway: what reads back is the
        // user's config, not awitch's write.
        assert_eq!(
            report.target,
            Some(other_target()),
            "{}: docs back at the user's gateway",
            app.name
        );

        for file in app.files {
            let doc = read_doc_of(&instance, file);
            for patch in file.patches {
                assert_eq!(
                    doc.get(patch.key_path),
                    Some(&patch.value.render(&other_target()).unwrap()),
                    "{}: {}.{}",
                    app.name,
                    file.path,
                    patch.key_path
                );
            }
            assert_user_key(&instance, file, &doc);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn undo_leaves_a_user_edit_alone() {
    for app in declared() {
        let dir = temp_config_dir(&format!("leave-edited-{}", app.name));
        let instance = instance(app, &dir);
        instance.point(&target()).unwrap();

        // The user rewrites one managed key after pointing.
        let edited_file = &app.files[0];
        let edited = &edited_file.patches[0];
        let mut doc = read_doc_of(&instance, edited_file);
        doc.set(
            edited.key_path,
            edited.value.render(&other_target()).unwrap(),
        )
        .unwrap();
        write_doc_of(&instance, edited_file, &doc);

        instance.undo().unwrap();
        assert_unpointed(&instance);

        for file in app.files {
            let doc = read_doc_of(&instance, file);
            for patch in file.patches {
                let kept = file.path == edited_file.path && patch.key_path == edited.key_path;
                let expected = kept.then(|| patch.value.render(&other_target()).unwrap());
                assert_eq!(
                    doc.get(patch.key_path),
                    expected.as_ref(),
                    "{}: {}.{}",
                    app.name,
                    file.path,
                    patch.key_path
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn undo_leaves_a_user_deletion_alone() {
    for app in declared() {
        let dir = temp_config_dir(&format!("leave-deleted-{}", app.name));
        let instance = instance(app, &dir);
        seed_pointed_docs(&instance, &other_target());
        instance.point(&target()).unwrap();

        // The user deletes one managed key after pointing: undo must not put
        // back the value the point had overwritten.
        let dropped_file = &app.files[0];
        let dropped = &dropped_file.patches[0];
        let mut doc = read_doc_of(&instance, dropped_file);
        assert!(
            doc.remove(dropped.key_path),
            "{}: {} was pointed",
            app.name,
            dropped.key_path
        );
        write_doc_of(&instance, dropped_file, &doc);

        instance.undo().unwrap();
        assert_unpointed(&instance);

        for file in app.files {
            let doc = read_doc_of(&instance, file);
            for patch in file.patches {
                let gone = file.path == dropped_file.path && patch.key_path == dropped.key_path;
                let expected = (!gone).then(|| patch.value.render(&other_target()).unwrap());
                assert_eq!(
                    doc.get(patch.key_path),
                    expected.as_ref(),
                    "{}: {}.{}",
                    app.name,
                    file.path,
                    patch.key_path
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn undo_leaves_replaced_values_alone() {
    for app in declared() {
        let dir = temp_config_dir(&format!("leave-replaced-{}", app.name));
        let instance = instance(app, &dir);
        instance.point(&target()).unwrap();

        // The user points the docs at their own gateway: every value carrying
        // it is theirs now, so undo must leave those as they stand.
        for file in app.files {
            let mut doc = read_doc_of(&instance, file);
            for patch in file.patches {
                doc.set(patch.key_path, patch.value.render(&other_target()).unwrap())
                    .unwrap();
            }
            write_doc_of(&instance, file, &doc);
        }

        instance.undo().unwrap();
        let report = instance.check().unwrap();
        assert_eq!(report.state, State::Unpointed, "{}", app.name);
        assert_eq!(
            report.target,
            Some(other_target()),
            "{}: the user's own gateway stays in place",
            app.name
        );

        for file in app.files {
            let doc = read_doc_of(&instance, file);
            for patch in file.patches {
                // Replaced = the value carries the gateway, so the user's
                // write differs from awitch's. A fixed pointer reads the same
                // under both targets, and stays awitch's to undo.
                let theirs = patch.value.render(&other_target()).unwrap();
                let ours = patch.value.render(&target()).unwrap();
                let expected = (theirs != ours).then_some(theirs);
                assert_eq!(
                    doc.get(patch.key_path),
                    expected.as_ref(),
                    "{}: {}.{}",
                    app.name,
                    file.path,
                    patch.key_path
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn env_override_redirects_the_config_root() {
    let mut overridden = 0;
    for app in declared() {
        // Apps whose dir is fixed have no override to redirect.
        if app.base_dir.env_override.is_none() {
            continue;
        }
        overridden += 1;

        let home = temp_config_dir(&format!("override-{}-home", app.name));
        let root = temp_config_dir(&format!("override-{}-root", app.name));
        let plain = instance(app, &home);
        let rooted = instance(app, &home).with_env_override(&root);

        rooted.point(&target()).unwrap();
        assert_pointed(&rooted);

        // Every doc lands under the override root, and the app's declared
        // default dir stays empty.
        for file in app.files {
            let path = rooted.file_path(file);
            assert!(
                path.starts_with(&root),
                "{}: {} is outside the override root",
                app.name,
                path.display()
            );
            assert!(path.exists(), "{}: {}", app.name, path.display());
            assert!(
                !plain.file_path(file).exists(),
                "{}: {} was written outside the override root",
                app.name,
                plain.file_path(file).display()
            );
        }
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&root);
    }
    assert!(overridden > 0, "no app declares an env override");
}

// ---- check observables ----

#[test]
fn check_reports_divergent_when_the_docs_no_longer_hold_the_record() {
    for app in declared() {
        let dir = temp_config_dir(&format!("divergent-{}", app.name));
        let instance = instance(app, &dir);
        build_state(&instance, Scenario::D);

        // The user rewrites one managed key: the record no longer describes
        // what stands in the docs.
        let file = &app.files[0];
        let patch = &file.patches[0];
        let mut doc = read_doc_of(&instance, file);
        doc.set(patch.key_path, patch.value.render(&other_target()).unwrap())
            .unwrap();
        write_doc_of(&instance, file, &doc);

        assert_eq!(
            instance.check().unwrap().state,
            State::Divergent,
            "{}: {}.{}",
            app.name,
            file.path,
            patch.key_path
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn check_ignores_record_edits_for_keys_the_schema_does_not_manage() {
    for app in declared() {
        let dir = temp_config_dir(&format!("stale-edit-{}", app.name));
        let instance = instance(app, &dir);
        build_state(&instance, Scenario::D);
        assert_eq!(instance.check().unwrap().state, State::Normal);

        // A record left over from a spec that managed a key this one does not.
        let Some(mut files) = record(&instance) else {
            panic!("{}: record must be complete", app.name);
        };
        let bucket = files
            .iter_mut()
            .find(|f| f.path == app.files[0].path)
            .expect("the record carries the schema's first file");
        bucket.edits.push(Edit {
            key_path: USER_KEY.into(),
            value: Value::String(USER_KEY.into()),
            old_value: None,
        });
        instance.record_file.write(&files).unwrap();

        // The extra edit is really in the record, and the verdict ignores it.
        let reread = record(&instance).expect("record still complete");
        assert_eq!(
            reread
                .iter()
                .find(|f| f.path == app.files[0].path)
                .unwrap()
                .edits
                .len(),
            app.files[0].patches.len() + 1,
            "{}: the record carries the unmanaged edit",
            app.name
        );
        assert_eq!(
            instance.check().unwrap().state,
            State::Normal,
            "{}",
            app.name
        );
        assert_pointed(&instance);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn check_read_errors_are_errors_not_states() {
    for app in declared() {
        let dir = temp_config_dir(&format!("read-error-{}", app.name));
        let instance = instance(app, &dir);
        build_state(&instance, Scenario::D);

        let file = &app.files[0];
        let path = instance.file_path(file);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();

        let err = instance.check().unwrap_err();
        // A read failure is an error, not a state — and it names the file
        // while keeping the underlying io cause in the chain.
        let text = format!("{err:#}");
        assert!(text.contains(&format!("read {}", path.display())), "{text}");
        assert!(err.source().is_some(), "io cause kept in the chain");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn check_reads_target_back_after_reset_residue() {
    for app in declared() {
        let dir = temp_config_dir(&format!("reset-residue-{}", app.name));
        let instance = instance(app, &dir);
        // Pointed, then the record corrupted, docs still present.
        build_state(&instance, Scenario::F);

        assert!(instance.reset().unwrap(), "{}", app.name);
        let report = instance.check().unwrap();
        // The record is gone — unpointed — but the residue in the docs is
        // read back from C (spec §reset: residual stays visible, never silent).
        assert_eq!(report.state, State::Unpointed, "{}", app.name);
        assert_eq!(report.target, Some(target()), "{}", app.name);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ---- WAL invariants through the operations ----

#[test]
fn point_record_area_failure_writes_nothing_and_retry_converges() {
    for app in declared() {
        let dir = temp_config_dir(&format!("wal-record-{}", app.name));
        let instance = instance(app, &dir);

        // A directory where the record file would go makes the record area
        // fail for real, before any doc is touched.
        let record_path = instance.record_file.path.clone();
        std::fs::create_dir_all(record_path.parent().unwrap()).unwrap();
        std::fs::create_dir(&record_path).unwrap();

        assert!(instance.point(&target()).is_err(), "{}", app.name);
        std::fs::remove_dir(&record_path).unwrap();
        assert!(!record_complete(&instance), "{}", app.name);
        assert!(
            app.files.iter().all(|f| !instance.file_path(f).exists()),
            "{}: a failed point writes no docs",
            app.name
        );

        instance.point(&target()).unwrap();
        assert!(record_complete(&instance), "{}", app.name);
        assert_pointed(&instance);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
#[cfg(unix)]
fn point_apply_failure_keeps_the_record_and_writes_no_docs() {
    for app in declared() {
        let dir = temp_config_dir(&format!("wal-apply-{}", app.name));
        let instance = instance(app, &dir);

        // The record (in point/) writes first; the first doc's directory goes
        // read-only, so the apply write fails for real before any file lands.
        let parent = instance
            .file_path(&app.files[0])
            .parent()
            .unwrap()
            .to_path_buf();
        std::fs::create_dir_all(&parent).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();

        let err = instance.point(&target()).unwrap_err();
        assert!(
            err.to_string().contains("record kept"),
            "{}: {err}",
            app.name
        );
        assert!(record_complete(&instance), "{}", app.name);
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            app.files.iter().all(|f| !instance.file_path(f).exists()),
            "{}: a failed apply writes no docs",
            app.name
        );

        instance.point(&target()).unwrap();
        assert_pointed(&instance);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
#[cfg(unix)]
fn undo_restore_failure_keeps_the_record_and_resumes() {
    for app in declared() {
        let dir = temp_config_dir(&format!("wal-undo-{}", app.name));
        let instance = instance(app, &dir);
        seed_pointed_docs(&instance, &other_target());
        instance.point(&target()).unwrap();

        // Read-only, so the restore's write fails for real.
        let parent = instance
            .file_path(&app.files[0])
            .parent()
            .unwrap()
            .to_path_buf();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(instance.undo().is_err(), "{}", app.name);
        assert!(record_complete(&instance), "{}", app.name);
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();

        instance.undo().unwrap();
        assert_unpointed(&instance);
        for file in app.files {
            let doc = read_doc_of(&instance, file);
            for patch in file.patches {
                assert_eq!(
                    doc.get(patch.key_path),
                    Some(&patch.value.render(&other_target()).unwrap()),
                    "{}: {}.{}",
                    app.name,
                    file.path,
                    patch.key_path
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
