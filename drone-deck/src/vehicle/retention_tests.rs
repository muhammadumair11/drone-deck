use super::*;
use crate::mavlink::fake_vehicle::scratch;
use std::time::Duration;

fn capture(dir: &Path, name: &str, len: usize, age_s: u64) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, vec![0u8; len]).expect("write");
    let at = SystemTime::now() - Duration::from_secs(age_s);
    fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|f| f.set_modified(at))
        .expect("set modified");
    path
}

#[test]
fn the_oldest_captures_go_past_the_count_or_the_budget() {
    let dir = scratch("tlog-retention");
    let newest = capture(&dir, "c.tlog", 10, 10);
    let middle = capture(&dir, "b.tlog", 10, 20);
    let oldest = capture(&dir, "a.tlog", 10, 30);
    let other = capture(&dir, "notes.txt", 10, 40);

    // Room for three captures: two old ones stay beside the new one.
    assert_eq!(prune(&dir, 3, u64::MAX), vec![oldest.clone()]);
    assert!(newest.exists() && middle.exists() && !oldest.exists());

    // A byte budget for one capture keeps only the newest.
    assert_eq!(prune(&dir, 100, 15), vec![middle.clone()]);
    assert!(newest.exists() && !middle.exists());
    assert!(other.exists(), "only .tlog files are touched");
}

#[test]
fn a_missing_folder_removes_nothing() {
    let dir = scratch("tlog-retention-missing").join("absent");
    assert!(prune(&dir, KEEP, BUDGET).is_empty());
}
