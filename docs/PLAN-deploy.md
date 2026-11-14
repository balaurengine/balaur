# One-click deploy

`balaur deploy`, and a Deploy button in the editor: one command or one click
from a project to a URL a player can open, or to a phone with the game on it.
`docs/PLAN-release.md` ships the engine; this ships a game.

## Steps

Each step leaves something a developer would use, and the first two need no
credential.

1. The verb, and a directory: `crates/balaur_deploy`, a `deploy` script
   module over it, `balaur deploy [name]`, and `[deploy.<name>]` tables in
   `project.toml` naming a `kind` and everything that is not a secret. The
   rules every later step keeps:
   - Deploy runs `balaur check --strict`, then `balaur export`, then places
     the output. It owns no template lookup, pack format or signing flag.
   - A secret comes from the OS keychain or the environment, never from
     `project.toml`. A missing one is named, with where to put it.
   - The upload runs through `ExternalIo`, so progress lands on a tick
     boundary as an event a script handles, and `replay::suppressed` keeps a
     replayed session from uploading.
   - Where a vendor has a CLI (`butler`, `steamcmd`, `adb`, `xcrun`), deploy
     calls it and says how to install it when absent. A plain JSON API goes
     through `balaur_http`.

   Destinations: a directory, `rsync` and `scp`. Done: a desktop build copied
   to a server by one command, and a recorded session that deploys replays
   without deploying.
2. The web: a `--target web` export placed as static files with the headers
   a wasm page needs. itch.io through `butler push`, GitHub Pages as a commit
   to a branch, and one `static` kind over the CLIs of S3, R2, Netlify and
   Vercel. Done: a link that plays the game, and a directory and an `rsync`
   to localhost deployed end to end in CI.
3. Gamend hosting: a project bound to a Gamend account, uploaded through its
   REST API, served at a stable URL with the previous version kept, and a QR
   code for the URL. Done: a playable URL from a developer who set up no
   hosting.
4. A phone on the cable: `adb install -r`, `xcrun devicectl device install
   app`, and the simulator and emulator variants. Done: an edit reaches a
   plugged-in phone in one command, and a macOS runner installs on the
   simulator in CI.
5. The tester tracks: TestFlight through the App Store Connect API, and Play
   internal testing through the Play Developer API taking the AAB `balaur
   export` writes. Build numbers bumped and remembered. Done: a second deploy
   uploads a second build rather than a refused duplicate.
6. The stores that take a binary: SteamPipe through `steamcmd`, which
   `docs/PLAN-steam.md` calls, and the signed desktop downloads. Done: a
   build reaches a Steam branch from `balaur deploy`.
7. The button: a palette command in `editor/scripts/palette.bs`, a
   destination picker over the same tables, a progress bar from the deploy
   events, and the URL or the failure in the Output panel. Done: the editor
   deploys with what the command line does, and nothing else.

Not planned: App Store and Play production review, since a submission is a
decision with a form attached. Consoles wait on the roadmap's console row.
Microsoft Store, the Mac App Store, Epic and GOG each become a `kind` when a
game asks.

## Open questions

1. Where does a credential live on Linux? `secret-service` needs a session
   bus a headless machine lacks. The environment is the fallback, so decide
   whether a file under the user data directory is ever supported.
2. Does the editor's upload run in a subprocess? An upload runs for minutes
   and can fail halfway, which a subprocess survives better. A browser tab
   has no subprocess. Decide it once, with the web editor in view.
3. What does a deploy record, and where? A build number, a URL, a content
   hash and a time per destination. `project.toml` merges badly, and a
   sibling lock file is a second file to explain.
4. Is a build-number bump part of a deploy? TestFlight and Play refuse a
   duplicate. A monotonic build number apart from the game's version, kept in
   the record of question 3, is the likely answer.
