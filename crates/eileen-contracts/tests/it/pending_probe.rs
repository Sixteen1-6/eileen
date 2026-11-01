//! A throwaway probe for pull request #133, reverted before review: both tests fail on purpose,
//! so the run shows that CI keeps a new file snapshot's `.snap.new` and an inline snapshot's
//! hidden `.pending-snap` in the job's artifact.

#[test]
fn a_new_file_snapshot_is_kept_as_snap_new() {
    insta::assert_snapshot!("pending-probe", "a value no snapshot file pins yet");
}

#[test]
fn a_changed_inline_snapshot_is_kept_as_pending_snap() {
    insta::assert_snapshot!("the value now", @"the value before");
}
