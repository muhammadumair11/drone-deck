use super::*;
fn profile(c: &Config, id: &str) -> Profile {
    resolve(c, c.profiles.iter().find(|p| p.id == id).unwrap())
}
#[test]
fn shared_location_drives_both_simulators_and_probes_per_distribution() {
    let mut c = crate::backend::defaults();
    let path = "/opt/drone's workspace/PX4";
    c.px4_installations.insert(
        c.distro.to_ascii_lowercase(),
        Installation {
            path: path.into(),
            build_target: "px4_sitl".into(),
        },
    );
    for id in ["sih", "gazebo"] {
        let p = profile(&c, id);
        assert!(p.args[0].contains(&quote(path)));
        assert!(p.probe.contains(&quote(path)));
        assert!(!p.args[0].contains(".local/share"));
    }
    assert!(profile(&c, "sih").args[0].contains("make px4_sitl sihsim_quadx"));
    assert!(profile(&c, "gazebo").args[0].contains("DRONE_DECK_GZ_MODEL"));
    c.distro = "OtherUbuntu".into();
    assert!(!profile(&c, "gazebo").args[0].contains("drone's"));
    assert!(profile(&c, "gazebo").args[0].contains("$HOME"));
}
#[test]
fn schema_two_adopted_sih_migrates_the_location_without_touching_custom_commands() {
    let mut c = crate::backend::defaults();
    for p in &mut c.profiles {
        p.px4 = None;
    }
    let path = "/opt/drone's workspace/PX4";
    let sih = c.profiles.iter_mut().find(|p| p.id == "sih").unwrap();
    sih.args = vec![format!(
        "cd {} && test -z \"$(pgrep -x px4)\" && make px4_sitl_sih sihsim_quadx",
        quote(path)
    )];
    sih.probe = format!(
        "test -x {}",
        quote(&format!("{path}/build/px4_sitl_sih/bin/px4"))
    );
    let mut v = serde_json::to_value(c).unwrap();
    v["schema"] = 2.into();
    let (mut c, notice) = crate::backend::parse_config(&serde_json::to_vec(&v).unwrap())
        .unwrap()
        .unwrap();
    assert!(notice.is_some());
    assert!(profile(&c, "gazebo").args[0].contains(&quote(path)));
    let custom = c.profiles.iter_mut().find(|p| p.id == "gazebo").unwrap();
    custom.px4 = None;
    custom.args = vec!["cd /custom && ./launch".into()];
    let original = custom.args.clone();
    assert_eq!(profile(&c, "gazebo").args, original);
    let mut v = serde_json::to_value(&c).unwrap();
    v["schema"] = crate::backend::SCHEMA.into();
    let (roundtrip, _) = crate::backend::parse_config(&serde_json::to_vec(&v).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(profile(&roundtrip, "gazebo").args, original);
}
#[test]
fn migration_never_infers_a_location_from_arbitrary_shell() {
    let mut c = crate::backend::defaults();
    let p = c.profiles.iter_mut().find(|p| p.id == "sih").unwrap();
    p.px4 = None;
    p.args = vec!["cd '/opt/PX4' && ./my-wrapper".into()];
    migrate(&mut c);
    assert!(c.px4_installations.is_empty());
    assert_eq!(profile(&c, "sih").args, ["cd '/opt/PX4' && ./my-wrapper"]);
}
#[test]
fn invalid_installations_are_refused() {
    for path in ["relative/px4", "/opt/px4\ncommand", "/opt/px4\0"] {
        assert!(
            validate(&Installation {
                path: path.into(),
                build_target: "px4_sitl_sih".into()
            })
            .is_err()
        );
    }
    assert!(
        validate(&Installation {
            path: "/opt/px4".into(),
            build_target: "bad; command".into()
        })
        .is_err()
    );
}

#[test]
fn adopting_px4_updates_the_shared_record_and_preserves_custom_launches() {
    let mut s = crate::State::new();
    s.setup.selected = s.setup.recipes.iter().position(|r| r.id == "px4").unwrap();
    s.setup.candidate = Some(crate::setup::Candidate {
        tool: "px4".into(),
        found: true,
        compatible: true,
        path: "/opt/flight stack".into(),
        executable: "/opt/flight stack/build/px4_sitl_sih/bin/px4".into(),
        environment: s.config.distro.clone(),
        build_target: "px4_sitl_sih".into(),
        ..Default::default()
    });
    let custom = s
        .config
        .profiles
        .iter_mut()
        .find(|p| p.id == "sih")
        .unwrap();
    custom.px4 = None;
    custom.args = vec!["./custom-sih".into()];
    crate::setup::profiles::adopt(&mut s);
    assert_eq!(profile(&s.config, "sih").args, ["./custom-sih"]);
    assert!(profile(&s.config, "gazebo").args[0].contains("'/opt/flight stack'"));
    assert!(
        crate::ulog::locations(&s)
            .px4_dirs
            .contains(&"/opt/flight stack".into())
    );
}
