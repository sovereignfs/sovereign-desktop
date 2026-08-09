# Sovereign Desktop

A minimal desktop shell for [Sovereign](https://github.com/sovereignfs/sovereign) —
the modular, self-hostable workspace runtime. On first launch you enter your
self-hosted instance URL; the app loads it in the system WebView and remembers it.
All functionality is served by your own instance — this app is only the native
wrapper. Multiple instances are supported.

Built with [Tauri 2](https://v2.tauri.app) (system WebView, ~5 MB binary):
WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux. macOS ships first;
Windows and Linux builds are produced from the same codebase.

Same model as the Nextcloud, Bitwarden, and Element desktop clients.

## Install

Download the installer for your platform from
[GitHub Releases](https://github.com/sovereignfs/sovereign-desktop/releases):
`.dmg` (macOS 13+), `.msi`/`.exe` (Windows), `.AppImage`/`.deb` (Linux).

On launch, enter your instance URL (e.g. `my.sovereign.example`). The app checks
it is a reachable Sovereign instance, then loads it. Use
**Instances → Switch Instance…** (⌘⇧I) to add, switch, or remove instances.

Links to your own instance stay inside the app. A link to anywhere else —
a third-party site, an OAuth provider, external docs — opens in your system's
default browser instead of taking over the app's window.

Sovereign runs from a tray/menu-bar icon: closing the window hides it rather
than quitting the app, so it stays reachable for notifications. Use the tray
icon's **Open** to bring the window back, or **Quit** to fully exit.

Sovereign also registers the `sovereign://` URL scheme. A link like
`sovereign://my.instance.example/plugins/console` opens the app and navigates
straight to `/plugins/console` on that instance; if it isn't one of your
stored instances yet, the app prompts you to add it first. On Windows/Linux,
clicking a `sovereign://` link while the app is already running currently
opens a second instance of the app rather than routing to the existing
window — a known limitation of not yet bundling the single-instance plugin.

The app checks for updates once per launch. If a newer signed release is
available, a native dialog offers **Update Now** (downloads, installs, and
restarts the app) or **Later** (dismisses until the next launch). Nothing
happens if you're already up to date.

## Detecting the desktop shell

The shell injects a frozen marker into every page it loads — including your
instance — before the page's own scripts run. Web apps and plugins can read it to
enable desktop-specific behaviour:

```js
if (window.__SOVEREIGN_DESKTOP__) {
  // { shell: 'desktop', os: 'macos' | 'windows' | 'linux', version: '0.1.0' }
  const { os, version } = window.__SOVEREIGN_DESKTOP__;
}
```

The marker is a plain, read-only data object — **not** a bridge to native APIs
(remote instance content never gets Tauri IPC access). It is the substrate the
SDK's `sdk.device.*` environment detection consumes to report the `"desktop"`
environment (Sovereign monorepo epic task 17.7). Plugins should prefer
`sdk.device.*` over reading the global directly once that lands.

## Development

Prerequisites: [Node.js](https://nodejs.org) ≥ 20, [pnpm](https://pnpm.io) 11,
[Rust](https://rustup.rs) (stable). On macOS you also need Xcode command-line tools.

```bash
pnpm install
pnpm dev            # tauri dev — opens the app with hot reload
pnpm build          # tauri build — produces .app + .dmg (unsigned locally)

pnpm test           # unit tests (vitest)
pnpm typecheck      # tsc --noEmit
pnpm lint           # eslint
pnpm format:check   # prettier
```

To test against a local Sovereign instance, run the platform dev server
(`pnpm dev` in the `sovereign` repo, port 3000) and add `http://localhost:3000`
as an instance.

## Releases

Pushing a `v*` tag triggers `.github/workflows/release.yml`, which builds all
platform installers and attaches them to a draft GitHub Release. The macOS build
is signed and notarized when these repository secrets are configured:
`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
`APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`. Without them the `.dmg` is
unsigned (Gatekeeper will warn on macOS 10.15+).

The app icons in `src-tauri/icons/` are generated from the Sovereign brand mark.
To regenerate them from an updated source, run `pnpm tauri icon <source.png>`
(1024×1024 PNG with transparency).

### Enabling auto-updates

The in-app update check (epic task 17.5) ships disabled-but-wired: it's safe
to build and run today, but won't find or install anything until you
complete this one-time setup. **Do these in order** —
`bundle.createUpdaterArtifacts` requires the signing key to already exist,
and enabling it before the key is configured breaks every build, `pnpm
build` included, not just releases:

1. Generate a signing keypair:
   `pnpm tauri signer generate -w ~/.tauri/sovereign-desktop.key` (set a
   password when prompted — recommended, not required).
2. Add two repository secrets (Settings → Secrets and variables → Actions):
   `TAURI_SIGNING_PRIVATE_KEY` (the private key file's contents) and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (blank if you skipped the password).
   `release.yml` already forwards both to the build — nothing to edit there.
3. Put the **public** key (the `.pub` file's contents, not secret) into
   `src-tauri/tauri.conf.json`'s `plugins.updater.pubkey`, replacing the
   `REPLACE_WITH_YOUR_GENERATED_PUBLIC_KEY` placeholder.
4. Set `src-tauri/tauri.conf.json`'s `bundle.createUpdaterArtifacts` to
   `true`.

From the next tagged release onward, `release.yml` produces a signed
`latest.json` update manifest alongside the installers, and the app's update
check (pointed at `.../releases/latest/download/latest.json` by default)
starts finding real releases.

## Troubleshooting

### A full-page 404 appears when opening an app (Account/Console), often right after launch

This is served by your **instance**, not the shell — the desktop app is a thin
WebView that renders whatever the instance returns. It is a known Next.js overlay
cold-start / transient-fetch issue on the platform side; reloading the page loads
the full view and clears it. See the platform troubleshooting guide for the cause
and current protection: `docs/troubleshooting.md` in the
[sovereign](https://github.com/sovereignfs/sovereign) repo ("Full-page 404 opening
an overlay route"). If it persists, confirm your instance is on a build dated
2026-06-28 or later.

> Note: the shell does not yet wire a reload shortcut, so the "just reload"
> workaround isn't available in-app. A reload menu item is planned.

## License

[AGPL-3.0-or-later](LICENSE)
