use super::*;
#[test]
fn name_edits_keep_the_shared_binding_and_command_edits_detach_it() {
    let mut s = State::new();
    s.selected = s
        .config
        .profiles
        .iter()
        .position(|p| p.id == "sih")
        .unwrap();
    s.config.px4_installations.insert(
        s.config.distro.to_ascii_lowercase(),
        crate::px4::Installation {
            path: "/opt/PX4 with space".into(),
            build_target: "px4_sitl_sih".into(),
        },
    );
    open(&mut s);
    assert!(
        s.form.as_ref().unwrap().fields[2]
            .text
            .contains("/opt/PX4 with space")
    );
    s.form.as_mut().unwrap().fields[0] = TextEdit::new("Renamed PX4");
    save(&mut s);
    assert_eq!(
        s.config.profiles[s.selected].px4,
        Some(crate::px4::Mode::Sih)
    );
    open(&mut s);
    s.form.as_mut().unwrap().fields[2] = TextEdit::new("./my-custom-flight");
    save(&mut s);
    let p = &s.config.profiles[s.selected];
    assert!(p.px4.is_none());
    assert_eq!(
        crate::px4::resolve(&s.config, p).args,
        ["./my-custom-flight"]
    );
}
