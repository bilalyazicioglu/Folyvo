# Core: OS assumptions taken from memory

Status of every entry: **assumed from knowledge, not yet observed**. Replace the status with the observed result (and the OS and filesystem it was observed on) when a spike or characterisation test confirms or refutes it.

| ID | Assumption | Pinned by | How to confirm |
|---|---|---|---|
| F-DATALESS-1 | iCloud placeholder files carry `SF_DATALESS = 0x4000_0000` in `st_flags` on macOS | `dataless::tests::only_the_dataless_bit_counts` (pure helper only) | Evict a file in iCloud Drive, then run `stat -f %Xf <file>` in spike S11 |
| F-APFS-1 | `clonefile` (via `reflink_copy::reflink`) works inside one APFS volume | `cas::tests::apfs_can_clone_a_file_within_a_volume` (macOS only) | Runs on every macOS CI job |
| F-RESTORE-1 | Restore does not preserve the creation date; extended attributes (including Finder tags) are preserved | `restore::tests::extended_attributes_such_as_finder_tags_survive_a_restore` (xattrs only) | Compare `stat -f %B` before and after a restore in Plan 3 |
| F-CASE-1 | Default macOS and Windows volumes are case-insensitive; `CaseRule::for_platform` stands in for per-volume detection | `canonical::tests::fold_rule_ignores_case_and_preserve_rule_does_not` | Plan 3 detects case sensitivity per volume |
| F-EXCEL-1 | Excel for Mac saves through an extensionless 8-hex-character temp file, deletes the original, then renames the temp into place | `ignore::tests::junk_is_ignored`, `debounce::tests::removed_then_changed_is_an_ingest_the_excel_pattern` | Save an `.xlsx` with Excel while `fs_usage` or a watcher logs events (Plan 3) |
| F-FSEVENTS-1 | FSEvents delivers renames as two unpaired events, so renames are matched by content, not by event pairing | `reconcile::tests::*` (matcher only) | Log raw `notify` events for a rename in Plan 2 |
