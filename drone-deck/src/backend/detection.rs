use super::*;
pub(super) fn detect(config: Config, tx: SyncSender<Event>) {
    let (linux, native): (Vec<_>, Vec<_>) = config
        .profiles
        .iter()
        .map(|p| crate::px4::resolve(&config, p))
        .partition(|p| p.program == "wsl");
    for p in native {
        let found = !p.probe.is_empty() && Path::new(&p.probe).exists();
        let _ = tx.send(Event::Detected(p.id, found));
    }
    for (id, found) in crate::agent::probe(&config.distro, &linux) {
        let _ = tx.send(Event::Detected(id, found));
    }
}
