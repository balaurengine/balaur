#!/usr/bin/env bash
# What one engine call from a script costs, in instructions retired, which hold
# still at any load where time does not (PLAN-balaur-script §9): a thousand calls
# a frame over FRAMES frames, less a loop of the same length that calls nothing,
# each after a run that fills the compile cache. macOS's `/usr/bin/time -l` counts.
#
#   scripts/call_cost.sh [path/to/balaur] [frames]
set -euo pipefail
bin=$(cd "$(dirname "${1:-target/debug/balaur}")" && pwd)/$(basename "${1:-target/debug/balaur}")
frames=${2:-200}
project=$(mktemp -d)
trap 'rm -rf "$project"' EXIT
mkdir -p "$project/scripts" "$project/scenes"
cat >"$project/project.toml" <<'TOML'
[application]
name = "call_cost"
main_scene = "scenes/main.toml"
TOML
cat >"$project/scenes/main.toml" <<'TOML'
[[nodes]]
id = "n_main"
name = "Main"
parent = ""
script = { source = "scripts/main.bs" }

[nodes.transform]
position = [1.0, 2.0, 3.0]

[nodes.widget]
kind = "label"
TOML
script() { # script <statement>
  cat >"$project/scripts/main.bs" <<BS
pub class Main {
    node: Node,
    total: float,
}

impl Main {
    fn update(dt: float) {
        let step = 0.0;
        let transform = "transform";
        let widget = "widget";
        for index in 0..1000 {
            step += 1.0;
            $1
        }
        self.total += step;
    }
}
BS
}
retired() {
  /usr/bin/time -l "$bin" run "$project" --headless --frames "$frames" 2>&1 >/dev/null |
    awk '/instructions retired/ {print $1}'
}
script ""
retired >/dev/null
none=$(retired)
printf '%-76s %8s\n' call "a call"
while IFS= read -r statement; do
  script "$statement"
  retired >/dev/null
  with=$(retired)
  awk -v with="$with" -v none="$none" -v frames="$frames" -v statement="$statement" \
    'BEGIN { printf "%-76s %8.0f\n", statement, (with - none) / frames / 1000 }'
done <<'CASES'
let time = engine::time();
self.node.transform.position = vec3(step, 2.0, 3.0);
let at = self.node.transform.position;
self.node.transform.skew = step;
let lean = self.node.transform.skew;
self.node.widget.text = "text";
let text = self.node.widget.text;
self.node.patch_component(transform, { "position": vec3(step, 2.0, 3.0) });
self.node.patch_component(widget, { "text": "text" });
let at = self.node.component_property(transform, "position");
CASES
