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
- **Remote instance content must never get Tauri IPC access.** Capabilities in
  `src-tauri/capabilities/` apply to the bundled onboarding page only; do not
  add remote URLs to capability windows or enable `dangerousRemoteDomainIpcAccess`.
  The one thing injected into remote pages is the `window.__SOVEREIGN_DESKTOP__`
  marker (see below) — a plain frozen data object, never a capability bridge.
- **Instance validation targets the public `GET /api/health`** liveness probe
  (`200` + `{ "status": "ok" }`). Do not use `/api/admin/health` — that endpoint
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
  onboarding.ts          add/switch/remove instances; /api/health validation
  store.ts               persistence via @tauri-apps/plugin-store (instances.json)
  validate.ts            pure URL/health helpers — unit-tested
src-tauri/               Tauri 2 app
  src/lib.rs             plugins + "Instances → Switch Instance…" menu (⌘⇧I),
                         which navigates the webview back to the local page
  tauri.conf.json        window, CSP, bundle targets, macOS 13 minimum
  capabilities/          IPC permissions for the local page only
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
```

CI (`.github/workflows/ci.yml`) runs format:check, lint, typecheck, test, and
`cargo check` on every push/PR. Releases (`release.yml`) run on `v*` tags and
attach `.dmg`/`.msi`/`.exe`/`.AppImage`/`.deb` to a draft GitHub Release; macOS
signing/notarization activates when the `APPLE_*` secrets are set (see README).

## Testing against a local instance

Run the platform dev server in the `sovereign` monorepo (`pnpm dev`, port 3000)
and add `http://localhost:3000` as an instance. `http://` is accepted when typed
explicitly (LAN/dev instances); bare input defaults to `https://`.

## Post-v1 roadmap (do not implement ahead of assignment)

Epic 17 in the monorepo sequences the follow-ups: 17.2 system tray + OS
notifications, 17.3 `sovereign://` deep links, 17.4 keychain credential storage,
17.5 auto-updater, 17.6 Mac App Store distribution, 17.7 SDK `"desktop"`
environment (`sdk.device.*` — lands in the monorepo, not here). Tasks are
assigned by the developer at session start — do not infer the next one.
