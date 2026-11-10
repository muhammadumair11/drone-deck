use super::*;

fn entry(text: &str) -> activity::Entry {
    activity::Entry {
        time: "10:00".into(),
        system: "Test".into(),
        text: text.into(),
    }
}

#[test]
fn only_start_requests_name_what_a_close_would_cut_short() {
    let mut s = State::new();
    s.setup.selected = s.setup.recipe_index("px4").unwrap_or(0);
    let name = s.setup.recipes[s.setup.selected].name.clone();
    assert_eq!(
        crate::setup::starting(&s.setup, "start"),
        Some(format!("the setup job for {name}"))
    );
    for action in ["scan", "online", "detect", "cancel", "browse"] {
        assert_eq!(crate::setup::starting(&s.setup, action), None, "{action}");
    }
    s.studio.repo = TextEdit::new("org/model");
    assert_eq!(
        crate::studio::starting(&s.studio, "start_model").as_deref(),
        Some("the model download of org/model")
    );
    s.studio.preview = Some(serde_json::json!({"repo": "org/reviewed"}));
    assert_eq!(
        crate::studio::starting(&s.studio, "start_model").as_deref(),
        Some("the model download of org/reviewed"),
        "the reviewed repository is what starts"
    );
    s.studio.selected_job = Some("job-7".into());
    assert_eq!(
        crate::studio::starting(&s.studio, "retry_model").as_deref(),
        Some("the retry of model download job-7")
    );
    for action in ["inspect_model", "cancel_model", "models", "save_token"] {
        assert_eq!(crate::studio::starting(&s.studio, action), None, "{action}");
    }
}

#[test]
fn closing_with_a_start_still_in_its_helper_warns_in_the_history() {
    let mut s = State::new();
    warn_on_close(&mut s);
    assert!(
        s.activity.entries.is_empty(),
        "nothing pending, nothing to say"
    );
    s.setup.starting = Some("the setup job for PX4".into());
    s.studio.starting = Some("the model download of org/model".into());
    warn_on_close(&mut s);
    let texts: Vec<_> = s.activity.entries.iter().map(|e| e.text.as_str()).collect();
    assert_eq!(texts.len(), 2);
    assert!(texts[1].contains("preparing the setup job for PX4; it may not have started"));
    assert!(texts[1].contains("Activity → Installations"));
    assert!(texts[0].contains("preparing the model download of org/model; it may not have"));
    assert!(texts[0].contains("Model downloads"));
}

#[test]
fn a_finished_request_no_longer_counts_as_pending_at_close() {
    let mut s = State::new();
    s.setup.pending = true;
    s.setup.starting = Some("the setup job for PX4".into());
    s.setup.tx.send(Err("Setup helper failed".into())).unwrap();
    crate::setup::poll(&mut s);
    assert!(!s.setup.pending);
    assert_eq!(s.setup.starting, None);
    assert!(close_warnings(&s).is_empty());
}

#[test]
fn the_next_start_shows_the_close_warning_once() {
    let mut s = State::new();
    s.setup.starting = Some("the setup job for PX4".into());
    s.studio.starting = Some("the model download of org/model".into());
    warn_on_close(&mut s);
    s.setup.starting = None;
    s.studio.starting = None;
    activity::record(&mut s, "Stopped the session");
    s.notice = "Earlier notice.".into();
    let expected = pending_notice(&s.activity.entries).unwrap();

    assert!(notice_at_start(&mut s));
    assert!(
        s.notice.starts_with("At the last close, Drone Deck stopped the helper still preparing the setup job for PX4; it may not have started."),
        "{}",
        s.notice
    );
    assert!(
        s.notice
            .contains("still preparing the model download of org/model")
    );
    assert!(s.notice.ends_with("Earlier notice."), "{}", s.notice);
    assert_eq!(s.notice, format!("{expected} Earlier notice."));
    assert_eq!(s.activity.entries[0].text, expected);

    // Shown and recorded: a later start says nothing until another close warns again.
    s.notice.clear();
    assert!(!notice_at_start(&mut s));
    assert!(s.notice.is_empty());
}

#[test]
fn an_old_warning_already_shown_is_not_repeated_after_a_new_one() {
    let entries: VecDeque<_> = [
        entry(&format!(
            "{CLOSED}the model download of b; it may not have started."
        )),
        entry(&format!(
            "{STARTED}the setup job for a; it may not have started."
        )),
        entry(&format!(
            "{CLOSED}the setup job for a; it may not have started."
        )),
    ]
    .into();
    assert_eq!(
        pending_notice(&entries).as_deref(),
        Some(format!("{STARTED}the model download of b; it may not have started.").as_str())
    );
    assert_eq!(pending_notice(&VecDeque::new()), None);
}
