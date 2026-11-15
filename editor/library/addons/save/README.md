# Save games

One table per slot in the player's user data folder. Copy this directory into
a project as `addons/save/`; the editor's Library dock does it, and adds it
with the Gamend addon, which keeps its session and prefs here.

```rust
use addons::save::save;

save::write("slot1", { "level": 4, "coins": 120 });
let data = save::read("slot1");
```

| Function | What it does |
| --- | --- |
| `write(slot, data)` | writes `saves/<slot>.toml` |
| `read(slot)` | what `write` put there; nil when there is none |
| `slots()` | every slot written, in name order |
| `remove(slot)` | deletes one, answering whether it was there |
| `directory()` | where slots are kept |

- A slot name is letters, digits, `-` and `_`; any other is refused and logged.
- `fs::write` writes beside the file, syncs it and renames it over, so a crash
  mid-save keeps the old one.
- `write` and `remove` emit `save::SAVED` from no node, with the slot's name.
- A game the editor plays keeps its slots in its own user data folder, the one
  it has when run alone.
- `editor/saves.bs` refills the editor's docks when the game saves.
