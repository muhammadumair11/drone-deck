use super::*;
use crate::backend;
use std::ffi::OsString;

/// A fresh, empty folder under the system temp directory.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("drone-deck-store-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
fn parse_number(bytes: &[u8]) -> Result<u32, String> {
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

#[test]
fn atomic_write_keeps_the_previous_version_as_bak() {
    let dir = scratch("atomic");
    let file = dir.join("state.json");
    write(&file, b"1").unwrap();
    assert!(!backup_path(&file).exists());
    write(&file, b"2").unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"2");
    assert_eq!(fs::read(backup_path(&file)).unwrap(), b"1");
    assert_eq!(names(&dir), ["state.json", "state.json.bak"]);
    write_json(&dir.join("nested/deeper.json"), &[1, 2]).unwrap();
    assert!(dir.join("nested/deeper.json").is_file());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn corrupt_file_is_quarantined_and_backup_restored() {
    let dir = scratch("recover");
    let file = dir.join("state.json");
    write(&file, b"1").unwrap();
    write(&file, b"2").unwrap();
    fs::write(&file, b"{ not json").unwrap();
    let Read::Recovered(value, notice) = load(&file, parse_number) else {
        panic!("expected the backup to be restored");
    };
    assert_eq!(value, 1);
    assert!(notice.contains("state.json was damaged"), "{notice}");
    assert!(notice.contains(".corrupt-"), "{notice}");
    assert_eq!(fs::read(&file).unwrap(), b"1");
    let quarantined: Vec<_> = names(&dir)
        .into_iter()
        .filter(|n| n.starts_with("state.corrupt-") && n.ends_with(".json"))
        .collect();
    assert_eq!(quarantined.len(), 1);
    assert_eq!(fs::read(dir.join(&quarantined[0])).unwrap(), b"{ not json");
    assert!(matches!(load(&file, parse_number), Read::Loaded(1)));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn corrupt_file_without_backup_is_kept_and_reported() {
    let dir = scratch("lost");
    let file = dir.join("state.json");
    fs::write(&file, b"oops").unwrap();
    let Read::Lost(notice) = load(&file, parse_number) else {
        panic!("expected a loss notice");
    };
    assert!(notice.contains("no usable backup"), "{notice}");
    assert!(!file.exists());
    assert_eq!(names(&dir).len(), 1);
    assert!(matches!(load(&file, parse_number), Read::Missing));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn schema_zero_profiles_are_migrated_to_schema_one() {
    let dir = scratch("schema");
    let file = dir.join("profiles.json");
    let mut old = serde_json::to_value(backend::defaults()).unwrap();
    let profiles = old["profiles"].as_array_mut().unwrap();
    for p in profiles.iter_mut() {
        p["download_url"] = "https://example.com/tool.exe".into();
    }
    profiles.push(serde_json::json!({"id":"ai","name":"AI pipeline","category":"AI & data",
        "description":"","program":"wsl","args":[],"cwd":"","probe":"","docs":"","download_url":""}));
    let old = serde_json::to_vec_pretty(&old).unwrap();
    fs::write(&file, &old).unwrap();

    let config = backend::load_at(&file).unwrap();
    assert!(config.profiles.iter().all(|p| p.id != "ai"));
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(saved["schema"], backend::SCHEMA);
    assert!(
        !String::from_utf8(fs::read(&file).unwrap())
            .unwrap()
            .contains("download_url")
    );
    assert_eq!(fs::read(backup_path(&file)).unwrap(), old);

    let before = fs::read(&file).unwrap();
    backend::load_at(&file).unwrap();
    assert_eq!(
        fs::read(&file).unwrap(),
        before,
        "schema 1 loads without rewriting"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn schema_one_drops_retired_profiles_and_keeps_user_profiles() {
    let dir = scratch("retired");
    let file = dir.join("profiles.json");
    let profile = |id: &str, name: &str, args: &str| {
        serde_json::json!({"id":id,"name":name,"category":"Development","description":"",
            "program":"wsl","args":[args],"cwd":"","probe":"","docs":""})
    };
    let old_gazebo = "cd \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\" && test -z \"$(pgrep -x px4)\" && make px4_sitl gz_x500";
    let v1 = serde_json::json!({"schema":1,"distro":"Ubuntu-24.04","profiles":[
        profile("sih", "PX4 / SIH", "make px4_sitl_sih sihsim_quadx"),
        profile("relay", "WSL display relay", "python3 relay.py"),
        profile("jmavsim", "jMAVSim", "-jar jmavsim_run.jar"),
        profile("telemetry", "MAVSDK telemetry", "python telemetry.py"),
        profile("hardware", "Native vehicle monitor", "hardware.ps1"),
        profile("gazebo", "Gazebo Harmonic", old_gazebo),
        profile("ardupilot", "ArduPilot SITL", "cd ~/my-ardupilot && ./custom.sh"),
        profile("relay-2", "My relay", "python3 mine.py"),
    ]});
    fs::write(&file, serde_json::to_vec_pretty(&v1).unwrap()).unwrap();
    let config = backend::load_at(&file).unwrap();
    let ids: Vec<&str> = config.profiles.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        ids,
        ["sih", "gazebo", "ardupilot", "relay-2", "rosgz", "foxglove"]
    );
    let args = |id: &str| {
        config
            .profiles
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .args
            .join(" ")
    };
    assert!(
        args("gazebo").contains("DRONE_DECK_GZ_MODEL"),
        "unedited default upgraded"
    );
    assert_eq!(
        args("ardupilot"),
        "cd ~/my-ardupilot && ./custom.sh",
        "edited command kept"
    );
    assert_eq!(
        args("relay-2"),
        "python3 mine.py",
        "user profiles untouched"
    );
    let (_, notice) = backend::parse_config(&serde_json::to_vec(&v1).unwrap())
        .unwrap()
        .unwrap();
    assert!(
        notice.as_deref().is_some_and(|n| n.contains(
            "Removed retired profiles (WSL display relay, jMAVSim, MAVSDK telemetry, Native vehicle monitor)"
        )),
        "{notice:?}"
    );
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(saved["schema"], backend::SCHEMA);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn schema_one_keeps_an_adopted_mavsdk_interpreter_as_the_mavsdk_profile() {
    let profile = |args: &str| {
        serde_json::json!({"id":"telemetry","name":"MAVSDK telemetry","category":"Development",
            "description":"","program":"wsl","args":[args],"cwd":"","probe":"","docs":""})
    };
    let v1 = |args: &str| {
        let v = serde_json::json!({"schema":1,"distro":"Ubuntu-24.04","profiles":[profile(args)]});
        serde_json::to_vec(&v).unwrap()
    };
    let adopted = "'/home/u/venv/bin/python' -u \"$DRONE_DECK_SCRIPTS/telemetry.py\"";
    let (config, notice) = backend::parse_config(&v1(adopted)).unwrap().unwrap();
    let p = config.profiles.iter().find(|p| p.id == "mavsdk").unwrap();
    assert_eq!(p.name, "MAVSDK Python");
    assert_eq!(
        p.args,
        ["'/home/u/venv/bin/python' -c 'import mavsdk; print(mavsdk.__file__)'"]
    );
    assert_eq!(p.probe, "test -x '/home/u/venv/bin/python'");
    assert!(!config.profiles.iter().any(|p| p.id == "telemetry"));
    assert!(
        notice
            .as_deref()
            .is_some_and(|n| n.contains("adopted MAVSDK interpreter is kept")
                && !n.contains("Removed retired")),
        "{notice:?}"
    );
    // The unadopted built-in named the default location only; it is dropped as retired.
    let default = "\"$HOME/.local/share/drone-deck/tools/mavsdk/bin/python\" -u \"$DRONE_DECK_SCRIPTS/telemetry.py\"";
    let (config, _) = backend::parse_config(&v1(default)).unwrap().unwrap();
    assert!(
        !config
            .profiles
            .iter()
            .any(|p| p.id == "mavsdk" || p.id == "telemetry")
    );
}

#[test]
fn newer_schema_is_refused_and_left_unchanged() {
    let dir = scratch("newer");
    let file = dir.join("profiles.json");
    let body = br#"{"schema": 99, "distro": "Ubuntu-24.04", "profiles": []}"#;
    fs::write(&file, body).unwrap();
    assert!(backend::load_at(&file).unwrap_err().contains("schema 99"));
    assert_eq!(fs::read(&file).unwrap(), body);
    assert_eq!(names(&dir), ["profiles.json", "profiles.json.kept"]);
    assert_eq!(fs::read(dir.join("profiles.json.kept")).unwrap(), body);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_profiles_are_created_with_schema() {
    let dir = scratch("fresh");
    let file = dir.join("profiles.json");
    let config = backend::load_at(&file).unwrap();
    assert!(!config.profiles.is_empty());
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(saved["schema"], backend::SCHEMA);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn defaults_drop_the_ai_profile_downloads_and_software_rendering() {
    let config = backend::defaults();
    assert!(config.profiles.iter().all(|p| p.id != "ai"));
    assert!(config.profiles.iter().any(|p| p.id == "ros2"));
    let qgc = config.profiles.iter().find(|p| p.id == "qgc").unwrap();
    assert!(qgc.args.is_empty());
    assert!(config.profiles.iter().all(|p| p.download_url.is_empty()));
    let json = serde_json::to_string(&config).unwrap();
    assert!(!json.contains("download_url"));
}

#[test]
fn legacy_workspace_is_copied_once() {
    let dir = scratch("legacy");
    let legacy = dir.join("exe/workspace");
    let root = dir.join("local/DroneDeck/workspace");
    fs::create_dir_all(legacy.join("setup/jobs/job-1")).unwrap();
    fs::write(legacy.join("profiles.json"), b"{\"legacy\":1}").unwrap();
    fs::write(legacy.join("activity.json"), b"[]").unwrap();
    fs::write(legacy.join("setup/jobs/job-1/state.json"), b"{}").unwrap();

    assert_eq!(
        backend::migrate_legacy(&legacy, &root).unwrap(),
        Some(Vec::new())
    );
    assert_eq!(
        fs::read(root.join("profiles.json")).unwrap(),
        b"{\"legacy\":1}"
    );
    assert_eq!(fs::read(root.join("activity.json")).unwrap(), b"[]");
    assert!(root.join("setup/jobs/job-1/state.json").is_file());
    let marker = fs::read_to_string(legacy.join("MIGRATED-TO.txt")).unwrap();
    assert!(marker.contains(&root.display().to_string()));
    assert!(!root.join("MIGRATED-TO.txt").exists());
    assert!(
        legacy.join("profiles.json").is_file(),
        "the legacy folder is left in place"
    );

    fs::write(legacy.join("profiles.json"), b"{\"legacy\":2}").unwrap();
    assert_eq!(backend::migrate_legacy(&legacy, &root).unwrap(), None);
    assert_eq!(
        fs::read(root.join("profiles.json")).unwrap(),
        b"{\"legacy\":1}"
    );
    assert_eq!(backend::migrate_legacy(&legacy, &legacy).unwrap(), None);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_workspace_is_not_copied_again_after_a_reset() {
    let dir = scratch("legacy-reset");
    let legacy = dir.join("exe/workspace");
    let root = dir.join("root");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("profiles.json"), b"{}").unwrap();
    assert!(backend::migrate_legacy(&legacy, &root).unwrap().is_some());
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(backend::migrate_legacy(&legacy, &root).unwrap(), None);
    assert!(!root.exists(), "a deleted data root starts fresh");
    let other = dir.join("other");
    assert!(backend::migrate_legacy(&legacy, &other).unwrap().is_some());
    assert_eq!(
        backend::migrate_legacy(&legacy, &legacy.join("inner")).unwrap(),
        None,
        "a root inside the legacy folder is never copied into itself"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn damaged_backup_recovery_keeps_the_good_backup() {
    let dir = scratch("recover-bak");
    let file = dir.join("state.json");
    write(&file, b"1").unwrap();
    write(&file, b"2").unwrap();
    fs::write(&file, b"bad").unwrap();
    assert!(matches!(load(&file, parse_number), Read::Recovered(1, _)));
    assert_eq!(fs::read(backup_path(&file)).unwrap(), b"1");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
#[cfg(windows)]
fn data_root_override_is_trimmed_and_absolute() {
    let exe = Path::new("C:\\app");
    let quoted = backend::resolve_root(Some(" \"D:\\data\" ".into()), exe, None);
    assert_eq!(quoted, PathBuf::from("D:\\data"));
    let relative = backend::resolve_root(Some("data".into()), exe, None);
    assert!(relative.is_absolute(), "{}", relative.display());
    assert!(relative.ends_with("data"));
}

#[test]
fn legacy_migration_needs_a_legacy_profiles_file() {
    let dir = scratch("legacy-empty");
    let legacy = dir.join("exe/workspace");
    let root = dir.join("root");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("activity.json"), b"[]").unwrap();
    assert_eq!(backend::migrate_legacy(&legacy, &root).unwrap(), None);
    assert!(!root.exists());
    assert_eq!(
        backend::migrate_legacy(&dir.join("absent"), &root).unwrap(),
        None
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn data_root_honours_environment_and_portable_overrides() {
    let dir = scratch("root");
    let exe = dir.join("app");
    fs::create_dir_all(&exe).unwrap();
    let local = Some(OsString::from(dir.join("local")));
    assert_eq!(
        backend::resolve_root(None, &exe, local.clone()),
        dir.join("local").join("DroneDeck").join("workspace")
    );
    assert_eq!(
        backend::resolve_root(Some(OsString::new()), &exe, local.clone()),
        dir.join("local").join("DroneDeck").join("workspace"),
        "an empty override is ignored"
    );
    assert_eq!(
        backend::resolve_root(None, &exe, None),
        exe.join("workspace")
    );
    fs::write(exe.join("portable.txt"), b"").unwrap();
    assert_eq!(
        backend::resolve_root(None, &exe, local.clone()),
        exe.join("workspace")
    );
    let custom = dir.join("custom");
    assert_eq!(
        backend::resolve_root(Some(custom.clone().into()), &exe, local),
        custom,
        "the environment wins over portable.txt"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn scripts_fall_back_to_the_source_folder() {
    let dir = scratch("scripts");
    assert_eq!(
        backend::scripts_in(&dir),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts")
    );
    assert!(
        backend::scripts_in(&dir)
            .join("agent")
            .join("agent.py")
            .is_file()
    );
    fs::create_dir_all(dir.join("runtime")).unwrap();
    assert_eq!(backend::scripts_in(&dir), dir.join("runtime"));
    fs::remove_dir_all(dir).unwrap();
}
