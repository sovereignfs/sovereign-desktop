# CLAUDE.md

Guidance for Claude Code working in this repository.

## What this is

**sovereign-desktop** — the native desktop shell for
[Sovereign](https://github.com/sovereignfs/sovereign), the modular,
self-hostable workspace runtime. A minimal Tauri 2.x app: on first launch the
user enters their self-hosted instance URL; the shell validates it, persists it,
and loads it in the system WebView. Multiple instances supported. macOS ships
first; Windows and Linux follow from the same codebase.

This repo is a sibling of the `sovereign` monorepo. The specification lives
there: **RFC 0038** (`docs/rfcs/0038-desktop-app-shell.md`), **epic 17**
(`docs/epics/desktop.md`), and **SRS §3.19**. Read them before changing the
shell's scope or behaviour.

## Hard rules — the minimal-shell philosophy

- **No product features in the shell.** Everything user-facing is served by the
  user's instance. If a feature could live in the instance, it must. The shell
  provides only: onboarding, instance persistence/switching, and native glue
  (menu, and post-v1: tray, notifications, deep links, keychain, updater).
- **TypeScript-first; Rust only for native glue** that must survive the webview
  navigating to remote content (e.g. the menu handler in `src-tauri/src/lib.rs`).
  Do not add Rust commands where a JS plugin API exists.
- **Never hardcode an instance URL** anywhere outside tests. The shell is
  universal — one binary for every self-hosted instance.
- **Remote instance content gets no IPC access beyond the one narrow device
  bridge command, deliberately.** The `default` capability
  (`src-tauri/capabilities/default.json`) stays local-only — no `remote`
  field, no `dangerousRemoteDomainIpcAccess` — and applies to the bundled
  onboarding page only. The **sole exception**, added in workstream 0003 leg
  3 (RFC 0083): `src-tauri/capabilities/bridge.json` grants the loaded
  instance's origin (`remote.urls`) exactly one app-defined command,
  `bridge_invoke` (`src-tauri/src/bridge.rs`) — the Tauri transport of
  `@sovereignfs/bridge`, injected as `window.__SOVEREIGN_BRIDGE__` alongside
  the marker below. This is a deliberate, narrowly-scoped amendment to the
  original absolute rule, not a loosening of it: `bridge_invoke` cannot do
  anything a plugin couldn't already do by calling a standard Web API in a
  browser tab (e.g. Web Notifications) — it just gets real native delivery.
  Any _other_ new remote capability grant, or widening `bridge.json`'s own
  permission set beyond `allow-bridge-invoke`, is still the kind of change
  this rule exists to prevent — treat it with the same scrutiny leg 3's own
  PR description documents, not as precedent that remote IPC access is now
  generally fine.
- **Instance validation targets the public `GET /api/instance`** endpoint
  (`200` + `{ "status": "ok", "product": "sovereign", "instanceName": string,
"platformVersion": string }` — sovereign epic task 20.2; supersedes the
  bare `/api/health` liveness probe this shell used before that endpoint
  existed, which couldn't reliably distinguish a genuine Sovereign instance
  from any other server answering `{ "status": "ok" }`, or surface the
  instance's display name). Do not use `/api/admin/health` — that endpoint
  is admin-key-gated.
- **Use `@tauri-apps/plugin-http` for shell→instance requests** — a plain
  `fetch` from the local page is blocked by CORS at the instance.
- **Keep the default macOS menu** when modifying the app menu — without the
  Edit menu, copy/paste shortcuts break inside WKWebView text fields.

## Architecture

```
index.html + src/        bundled onboarding / instance-manager page (Vite)
  main.ts                boot: stored active instance → load it; else onboarding
                         (?manage=1 forces the manager view)
  onboarding.ts          add/switch/remove instances; /api/instance validation
  store.ts               persistence via @tauri-apps/plugin-store (instances.json)
  validate.ts            pure URL/health helpers — unit-tested
src-tauri/               Tauri 2 app
  src/lib.rs             plugins + "Instances → Switch Instance…" menu (⌘⇧I),
                         which navigates the webview back to the local page
  src/bridge.rs          device bridge dispatch — the single `bridge_invoke`
                         command (see "Device bridge" below)
  tauri.conf.json        window, CSP, bundle targets, macOS 13 minimum
  capabilities/          default.json (local page only) + bridge.json (the
                         one narrow remote grant — see "Hard rules" above)
  permissions/           bridge-invoke.toml — the ACL permission bridge.json grants
```

The local page acts as a splash on boot: when an active instance is stored, it
immediately `location.replace()`s to it. After that navigation the shell's JS is
gone — anything that must keep working (the menu) is handled in Rust.

### Shell-detection marker

`src-tauri/src/lib.rs` creates the main window programmatically (not in
`tauri.conf.json`, whose `windows` array is intentionally empty) so it can attach
an `initialization_script`. That script defines a frozen
`window.__SOVEREIGN_DESKTOP__ = { shell: 'desktop', os, version }` on every page —
including the loaded instance — before page scripts run. The web app / SDK
(`sdk.device.*`, monorepo task 17.7) reads it to enable desktop-specific
features. Keep it a pure data marker; never widen it into an IPC bridge. Because
the window is created in Rust, window properties (title, size, min-size) live in
`lib.rs`, not the config.

### Device bridge (`@sovereignfs/bridge`'s Tauri transport, RFC 0083)

A second `initialization_script`, chained after the marker script above,
defines `window.__SOVEREIGN_BRIDGE__` on every page load — the
`InstalledBridge` shape `@sovereignfs/bridge`'s page-side code looks for
(`packages/bridge/src/protocol.ts` in the monorepo). Unlike the marker, this
one **is** a real IPC bridge, by deliberate exception to the hard rule above:
`invoke()` calls `window.__TAURI_INTERNALS__.invoke('bridge_invoke', ...)`,
reaching `src-tauri/src/bridge.rs`'s single command, which the `bridge`
capability grants to the loaded instance's origin.

- **Only advertise a capability this build actually implements.** `bridge.rs`
  dispatches `notifications.native` (real native delivery via
  `tauri-plugin-notification`'s `NotificationExt`, not `window.Notification`
  — the plugin's own JS-side `guest-js` for `requestPermission`/
  `sendNotification` just calls the standard Notification API directly,
  which may not even exist in WKWebView; going through `NotificationExt`
  sidesteps that entirely) and `camera.photo` (native file picker via
  `tauri-plugin-dialog`'s `DialogExt`, **file-picker only — never live
  webcam capture**; the `source: 'camera' | 'library'` field the SDK sends
  is intentionally ignored, since desktop has no equivalent "camera" mode to
  route to) and `biometrics.confirm` (epic task 17.10 — `crate::biometrics`,
  Touch ID/Windows Hello called directly via `objc2-local-authentication`/
  the `windows` crate, since no Tauri plugin covers desktop biometrics at
  all). `haptics.impact` is a deliberate no-op — falling through to
  `unavailable` — per RFC 0083 §7's own table for this transport; do not add
  a fake implementation to "complete" the capability list.
- **`biometrics.confirm` is macOS/Windows only — conditionally advertised,
  not a flat capability list.** `lib.rs`'s `capabilities_list()` omits it on
  Linux (no standard OS biometric primitive exists there) the same way
  `haptics.impact` is omitted everywhere: don't advertise a capability that
  would always resolve `unavailable`. **Windows support is written but
  unverified beyond a cross-compile type-check** (`cargo check --target
x86_64-pc-windows-msvc`) — this repo's CI does not have a Windows runner
  or Windows-C-toolchain access; the full binary can't even be
  cross-compiled here (an unrelated pre-existing dependency, `ring`, needs
  Windows C headers this machine doesn't have). Do not treat
  `src/biometrics/windows.rs` as more verified than that until someone
  builds and runs it on real Windows.
- **Adding a new bridge capability means adding to all three places in
  lockstep**: the `capabilities` array in `bridge_script()`/
  `capabilities_list()` (`lib.rs`), the `match` in `bridge_invoke`
  (`bridge.rs`), and — if it needs new native access `allow-bridge-invoke`
  doesn't already cover — a new permission file under `permissions/`
  referenced from `bridge.json`.
- `getPermission()`/`requestPermission()` on this transport report `'granted'`
  unconditionally (SDK-side, `packages/sdk/src/device-client.ts` in the
  monorepo) — there is no bridge action for a permission pre-check, only the
  one-shot `show`. The OS still gates the real permission when `show()`
  actually runs; that outcome surfaces through `show()`'s own `DeviceResult`.

### Navigation policy (epic task 17.8, RFC 0058)

`WebviewWindowBuilder::on_navigation` (Tauri's equivalent of iOS's
`decidePolicyFor` / Android's `shouldOverrideUrlLoading`) decides same-origin
(allow, stays in the WebView) vs. everything else (deny, reopen via
`tauri_plugin_opener::open_url` — the system default browser) on every
top-level navigation. `is_allowed_navigation` in `lib.rs` is the pure decision
function (unit-tested); `allow_navigation` wraps it with the actual side
effect. "Same-origin" means the loaded instance's own active origin — read
directly from `tauri-plugin-store`'s `instances.json` (`activeUrl` key) via
its Rust API, the exact store `src/store.ts` already writes, so there's one
source of truth. Mirrors mobile's ADR 0007, extending its RFC 0058
requirement to desktop (RFC 0038 never carried it over — see epic task 17.8's
own note in the monorepo). `window.open()` / `target="_blank"` requests go
through a _separate_ Tauri hook, `on_new_window` — also registered, reusing
the same `is_allowed_navigation` decision: same-origin gets a real new
window (`NewWindowResponse::Allow`), anything else is denied and reopened in
the system browser instead, same outcome as `allow_navigation` reaches for a
plain link.

### Deep links (`sovereign://`, epic task 17.3, RFC 0038)

Registered via `tauri.conf.json`'s `plugins.deep-link.desktop.schemes`
(verify with `plutil -p .../Info.plist` after `pnpm build` — look for
`CFBundleURLTypes`). All resolution happens in TypeScript, not Rust: the
`tauri-plugin-deep-link` event (`deep-link://new-url`) has a documented race
on macOS where it can arrive after this app's own `setup()`, so `lib.rs`
never tries to resolve the link itself — it unconditionally forces the
webview back to the local page with the raw URL attached as `?deeplink=`
(cold launch via `get_current()`, warm via `on_open_url`), and `main.ts` /
`src/deep-link.ts` do the actual host-matching against the stored instance
list. No `@tauri-apps/plugin-deep-link` JS dependency and no new
capability/permission grant, since no JS-side plugin command is ever called.
**Known limitation:** Windows/Linux have no single-instance plugin yet, so a
link click while already running opens a second OS process instead of
routing to the existing window.

### Auto-updater (epic task 17.5)

`check_for_updates` in `lib.rs` runs once per launch (end of `.setup()`),
via `tauri-plugin-updater`. On an available update it shows a **native**
dialog (`tauri-plugin-dialog`'s `DialogExt`) — not a WebView banner — since
the WebView may currently be showing the bundled local page or a loaded
instance, and shell UI must not touch either. "Update Now" calls
`Update::download_and_install()` then `AppHandle::restart()`; "Later" (or
any check failure — no endpoint reachable, no update available, an unset
placeholder `pubkey`) all resolve identically: nothing happens, silently.

**Ships disabled-but-wired.** `tauri.conf.json`'s `plugins.updater.pubkey`
is a placeholder (`REPLACE_WITH_YOUR_GENERATED_PUBLIC_KEY`) and
`bundle.createUpdaterArtifacts` is intentionally **absent**, not `false` —
empirically confirmed that setting it to `true` without a matching
`TAURI_SIGNING_PRIVATE_KEY` breaks `tauri build` outright (a hard bundler
error, not a graceful skip, unlike the macOS `APPLE_*` signing secrets).
Do not flip that flag without also completing the rest of the README's
"Enabling auto-updates" checklist — `release.yml` already forwards
`TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)` from repo secrets, but that alone
does nothing until the config flag and real pubkey are also set.

## Conventions

Carried over from the `sovereign` monorepo:

- **Prettier** is the single source of style truth: single quotes, semicolons,
  trailing commas (`all`), print width 100, 2-space indent. Never add overrides.
- **ESLint 9 flat config** (`js.recommended` + `typescript-eslint` strict +
  `eslint-config-prettier`). Never disable rules inline without a comment
  explaining why. Prefix intentionally-unused identifiers with `_`.
- **Branch per change**, from up-to-date `main`: `feat/<slug>`, `fix/<slug>`,
  `docs/<slug>`, `chore/<slug>`.
- **Commits** end with the Claude Code attribution trailer (model-agnostic):
  `Co-Authored-By: Claude Code <noreply@anthropic.com>`
- **PRs** target `main`, created as GitHub drafts first (`gh pr create --draft`);
  bodies end with `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.
- **Merge strategy: rebase and merge** — never squash, never merge commits.
- **Verify before claiming done** — run the checks below and show the output.

### Versioning

Repo semver follows the change type (`fix/` → patch, `feat/` → minor, breaking
→ major). Keep `package.json`, `src-tauri/Cargo.toml`, and
`src-tauri/tauri.conf.json` versions in lockstep — all three carry the app
version. Release tags are `vX.Y.Z`. Version slots for this repo are tracked in
the monorepo's `docs/roadmap.md` under **Desktop** (v0.1.0 = epic task 17.1);
when a roadmap-tracked task ships here, update the monorepo's roadmap/epic in a
monorepo PR.

## Commands

```bash
pnpm install        # install JS deps (Rust deps resolve on first build)
pnpm dev            # tauri dev — run the app with hot reload
pnpm build          # tauri build — .app/.dmg on macOS (unsigned locally)
pnpm test           # vitest (pure helpers in src/__tests__/)
pnpm typecheck      # tsc --noEmit
pnpm lint           # eslint
pnpm lint:fix       # eslint --fix
pnpm format         # prettier --write
pnpm format:check   # prettier --check (CI)

cargo test --manifest-path src-tauri/Cargo.toml   # Rust unit tests (pure helpers, e.g. navigation policy)
```

CI (`.github/workflows/ci.yml`) runs format:check, lint, typecheck, test,
`cargo check`, and `cargo test` on every push/PR. Releases (`release.yml`) run
on `v*` tags and attach `.dmg`/`.msi`/`.exe`/`.AppImage`/`.deb` to a draft
GitHub Release; macOS signing/notarization activates when the `APPLE_*`
secrets are set (see README).

## Testing against a local instance

Run the platform dev server in the `sovereign` monorepo (`pnpm dev`, port 3000)
and add `http://localhost:3000` as an instance. `http://` is accepted when typed
explicitly (LAN/dev instances); bare input defaults to `https://`.

## Post-v1 roadmap (do not implement ahead of assignment)

Epic 17 in the monorepo sequences this repo's work. Shipped: 17.1 shell
scaffold, 17.2 system tray + OS notifications, 17.3 `sovereign://` deep
links, 17.5 auto-updater (mechanism shipped; see its section above — signing
key setup is a separate, manual activation step, not code), 17.8 navigation
policy enforcement. Remaining: 17.4 keychain credential storage — **blocked**,
see [RFC 0072's addendum](https://github.com/sovereignfs/sovereign/blob/main/docs/rfcs/0072-external-oauth-provider.md#addendum-well-known-first-party-client-for-official-native-shells)
in the monorepo before picking this up, 17.6 Mac App Store distribution,
17.7 SDK `"desktop"` environment (`sdk.device.*` — lands in the monorepo,
not here). Tasks are assigned by the developer at session start — do not
infer the next one.
