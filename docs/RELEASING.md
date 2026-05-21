# Releasing RetroAmp

How to ship a new version. Most of the work is done by GitHub Actions; the
human-facing steps are bumping versions and pushing a tag.

## One-time setup

You only need to do this once per repository. After it's done, releasing is
just `git tag` + `git push`.

### 1. Generate the Tauri updater signing key

Tauri's auto-updater verifies every downloaded update against a public key
baked into the app. The matching private key lives only on your machine and
in GitHub Secrets — never commit it.

```sh
pnpm tauri signer generate -w ~/.tauri/retroamp.key
```

You'll be prompted for a password. Set one (it protects the key file at rest)
and remember it — you need it again in step 2.

The command prints two things:

- A path to the private key file (`~/.tauri/retroamp.key`)
- A **public key** (a long base64 string)

### 2. Add GitHub secrets

In the repo on GitHub: **Settings → Secrets and variables → Actions → New repository secret**.

| Name                                      | Value                                                              |
| ----------------------------------------- | ------------------------------------------------------------------ |
| `TAURI_SIGNING_PRIVATE_KEY`               | The full contents of `~/.tauri/retroamp.key` (paste the file)      |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`      | The password you set in step 1                                     |

### 3. Wire the public key into the app

Open `src-tauri/tauri.conf.json` and replace the `pubkey` placeholder under
`plugins.updater.pubkey` with the public key from step 1. Commit and push.

### 4. Install JS dependencies once

```sh
pnpm install
```

This pulls in the new updater/process plugins and updates `pnpm-lock.yaml`.
Commit the lockfile change.

## Cutting a release

0. (Optional) If you've favourited or unfavourited skins in the running app
   since the last release and want those changes to ship as the seed
   library, run:

   ```sh
   ./scripts/rebuild_skin_seeds.sh
   git add skins/
   ```

   The seed library is what every new install receives on first launch
   (see `src-tauri/src/skin/seed.rs`). Skipping this step is fine — the
   previously bundled set ships unchanged.

1. Bump the version in **all three** of these files (must match):
   - `package.json` — `"version"`
   - `src-tauri/Cargo.toml` — `[package] version`
   - `src-tauri/tauri.conf.json` — top-level `"version"`

2. Commit the bump:

   ```sh
   git commit -am "release: v0.1.1"
   ```

3. Tag and push:

   ```sh
   git tag v0.1.1
   git push origin main --tags
   ```

4. The `Release` workflow runs on three runners in parallel
   (Ubuntu 22.04, macOS, Windows). It takes ~15–25 minutes.

5. When CI finishes, a **draft** GitHub Release will be waiting at
   https://github.com/DevSpecAliPrice/RetroAmp/releases. Edit the notes,
   verify the artifacts attached, then click **Publish**.

That's it — existing installs will pick up the update on next launch.

## What gets built

Each tag produces these artifacts on the release:

| Platform | Files                                                      |
| -------- | ---------------------------------------------------------- |
| Linux    | `.AppImage` (portable), `.deb` (Debian/Ubuntu), `.rpm`     |
| macOS    | `.dmg` (universal — runs on Intel and Apple Silicon)       |
| Windows  | `.msi` (Windows Installer), `-setup.exe` (NSIS installer)  |

Plus a `latest.json` manifest that the in-app updater polls.

## How auto-updates work

On startup, the main window calls `checkForUpdates()` (see `src/updater.ts`).
That hits the endpoint configured in `tauri.conf.json`:

```
https://github.com/DevSpecAliPrice/RetroAmp/releases/latest/download/latest.json
```

GitHub redirects this to whichever release is marked **latest** (so draft and
pre-release versions are skipped). If the version in `latest.json` is newer
than the running app, the user sees a prompt. Approve → download → verify
signature → install → relaunch.

If you ever want to pull a release back, mark it as not-latest in GitHub and
the previous one becomes the served version automatically.

## Code signing (currently unsigned)

Builds are not signed for the OS itself, only for the Tauri updater. This
means:

- **macOS**: First-launch shows "RetroAmp can't be opened because Apple
  cannot check it for malicious software." Users right-click → Open to
  bypass, or run `xattr -dr com.apple.quarantine /Applications/RetroAmp.app`.
- **Windows**: SmartScreen shows "Windows protected your PC." Users click
  "More info" → "Run anyway."
- **Linux**: No signing required; AppImage just runs.

To remove the friction later, the additions are:

- **macOS**: Apple Developer ID + notarization. Requires an Apple Developer
  account ($99/yr) and adds two more secrets to the workflow
  (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
  `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`). `tauri-action` reads these
  automatically.
- **Windows**: Code-signing certificate ($200–400/yr from a CA). Adds
  `WINDOWS_CERTIFICATE` and `WINDOWS_CERTIFICATE_PASSWORD` secrets. Tauri's
  bundler signs the `.exe` and `.msi` automatically when present.

Neither is required to ship — the app works fine, users just get a one-time
warning. Worth revisiting once usage justifies the recurring cost.

## Local builds

You don't need CI to build locally. From the repo root:

```sh
pnpm tauri build
```

The artifacts land in `src-tauri/target/release/bundle/`. Useful for smoke-
testing a release before tagging.
