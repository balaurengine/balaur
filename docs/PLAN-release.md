# Binary releases

What is left before every desktop download opens without a warning and
updates itself. `docs/RELEASING.md` holds how a nightly, a version and a
channel are cut.

## Steps

1. **A Linux AppImage.** `scripts/package.sh` makes
   `balaur-editor-<target>.tar.gz` for `linux-x64` and `linux-arm64`, and
   nothing makes an AppImage. Done: each Linux release carries an AppImage
   beside its tarball that starts the editor with no install step, and the
   tarball stays for CI and `balaur update`.
