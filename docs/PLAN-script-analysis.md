# Plan: what `balaur check` reports

`balaur check` compiles every script and builds every scene with the run's
own loader, so a bad call or key is an error before anything runs. One defect
in how it names a place is left.

## Steps

1. **An error in a used file names that file.** An error in a file a script
   `use`s is filed under the script and the used file's line and column:
   `scripts/ball.bs:2:12: error: in scripts/helper.bs: unknown name …`. The
   finding should carry `scripts/helper.bs:2:12`, so the editor's Problems
   list and the language server open the right place. Where:
   `check_project_using` in `crates/balaur/src/lib.rs` and
   `BsHost::check_attached` in `crates/balaur_script_bs/src/host.rs`. Done: a
   case in `crates/balaur/tests/suite/script_check.rs` asserts the used file's
   path and line.
