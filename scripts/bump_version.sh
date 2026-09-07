#!/usr/bin/env bash
# Move the workspace to its next version, the step docs/RELEASING.md opens
# with. Writes the version and nothing else: the roadmap, the generated docs
# and the tag stay yours.
#
# Usage: bump_version.sh [patch|minor|major]   patch by default
#        bump_version.sh --set 0.4.2           an exact version
#        bump_version.sh --dry-run [part]      print the move, write nothing
set -euo pipefail
cd "$(dirname "$0")/.."

part=patch
exact=""
dry=false
while [ $# -gt 0 ]; do
  case $1 in
    major | minor | patch) part=$1 ;;
    --set) exact=${2:?--set needs a version}; shift ;;
    --dry-run | -n) dry=true ;;
    -h | --help) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) printf 'unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

# The one bare `version =` under [workspace.package]; every crate inherits it.
current=$(awk '/^\[workspace\.package\]/{s=1;next} /^\[/{s=0} s&&/^version = /{gsub(/[",]/,"",$3);print $3;exit}' Cargo.toml)
[ -n "$current" ] || { printf 'no [workspace.package] version in Cargo.toml\n' >&2; exit 1; }

if [ -n "$exact" ]; then
  next=$exact
else
  IFS=. read -r major minor patch <<<"$current"
  case $part in
    major) next="$((major + 1)).0.0" ;;
    minor) next="$major.$((minor + 1)).0" ;;
    patch) next="$major.$minor.$((patch + 1))" ;;
  esac
fi

# draft_release.sh compares the tag to this, so a malformed one fails in CI.
[[ $next =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { printf 'not a version: %s\n' "$next" >&2; exit 1; }
[ "$next" != "$current" ] || { printf 'already at %s\n' "$current" >&2; exit 1; }

printf '%s -> %s\n' "$current" "$next"
if [ "$dry" = true ]; then
  printf '(dry run)\n'
  exit 0
fi

# A sibling is pinned by version as well as by path, for publishing, and cargo
# refuses the tree when the two disagree. extension_greeter and the VS Code
# extension sit outside the workspace and pin this engine's version too.
python3 - "$current" "$next" <<'PY'
import re
import sys

current, next_ = sys.argv[1], sys.argv[2]


def rewrite(path, fn):
    text = open(path).read()
    open(path, "w").write(fn(text))


def in_section(text, header, fn):
    found = re.search(rf"\[{re.escape(header)}\][^\[]*", text).group(0)
    return text.replace(found, fn(found), 1)


def bump_cargo(text):
    text = in_section(
        text,
        "workspace.package",
        lambda s: s.replace(f'version = "{current}"', f'version = "{next_}"', 1),
    )
    return in_section(
        text,
        "workspace.dependencies",
        lambda s: pins(s),
    )


def pins(text):
    return re.sub(
        rf'(path = "[^"]*crates/[^"]+", version = ")({re.escape(current)})(")',
        rf"\g<1>{next_}\g<3>",
        text,
    )


rewrite("Cargo.toml", bump_cargo)
rewrite("examples/extension_greeter/Cargo.toml", pins)
rewrite(
    "editors/code/package.json",
    lambda t: t.replace(f'"version": "{current}"', f'"version": "{next_}"', 1),
)
PY

# Only the path dependencies move, so both of these are lockfile edits
# rather than dependency updates.
cargo update -w --quiet
for crate in balaur_plugin balaur_core balaur_script; do
  cargo update --quiet --manifest-path examples/extension_greeter/Cargo.toml -p "$crate"
done

printf '\nwrote Cargo.toml, Cargo.lock, editors/code/package.json,\n'
printf 'and examples/extension_greeter/{Cargo.toml,Cargo.lock}\n'
cat <<EOF

next, per docs/RELEASING.md:
  docs/ROADMAP.md            rewrite the rows this version finished
  python3 scripts/gen_docs.py
  git commit, push, wait for green
  git tag v$next && git push origin v$next
EOF
