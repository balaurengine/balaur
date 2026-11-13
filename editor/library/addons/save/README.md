# Save games

One table per slot in the player's user data folder, stamped with the version
this build writes. Copy this directory into a project as `addons/save/`; the
editor's Library dock does it, and adds it with the Gamend addon, which keeps
its session and prefs here.

```rust
use addons::save::save;

save::write("slot1", { "level": 4, "coins": 120 });
let data = save::read("slot1");
```

| Function | What it does |
| --- | --- |
| `write(slot, data)` | writes `saves/<slot>.toml`, stamped with `[save] version` |
| `read(slot)` | the table, brought forward to this build's version; nil when there is none |
| `slots()` | every slot written, in name order |
| `remove(slot)` | deletes one, answering whether it was there |
| `migrate_with(step)` | names the `fn(version, data) -> data` that brings a file forward one version |
| `version()`, `directory()` | the version this build writes, and where slots are kept |

- A slot name is letters, digits, `-` and `_`; any other is refused and logged.
- `fs::write` writes beside the file, syncs it and renames it over, so a crash
  mid-save keeps the old one.
- A file older than `[save] version` runs the `migrate_with` step once per
  version, 1 to 2 then 2 to 3. A file a newer build wrote is refused.
- `write` and `remove` emit `save::SAVED` from no node, with a running count.
- A game the editor plays keeps its slots in its own user data folder, the one
  it has when run alone.
- `editor/saves.bs` lists `[save] version` in the editor's Settings window.
