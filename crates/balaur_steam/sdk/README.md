# Valve's Steamworks SDK, vendored

From the Steamworks SDK 1.64, as the `steamworks-sys` 0.13.0 crate carries it
(`lib/steam/`):

- `steam_api.json`: Valve's description of the flat API, which
  `scripts/gen_steam.py` turns into `src/sdk/`.
- `redistributable_bin/{linux64,linuxarm64,osx,win64}/`: the `steam_api`
  library a `--steam` export ships beside the game, unchanged.
- `layouts.json`: each struct's size, alignment and field offsets per
  platform, as steamworks-sys's bindgen tests record them
  (`python3 scripts/gen_steam.py --layouts <steamworks-sys dir>`), which the
  generated `src/sdk/layout.rs` asserts at compile time.
- `unmapped.md`: written by the generator, what the script modules leave out.

The redistributables are Valve's, shipped under the Steamworks SDK Access
Agreement, beside a game on Steam.
