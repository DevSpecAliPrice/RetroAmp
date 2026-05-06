# Spotify Setup Guide

> **Spotify is currently disabled in default builds.** The Dev-Mode registration step described below is too rough an onboarding step for a casual desktop player, and YouTube Music covers the same ground out of the box. The Spotify code path is preserved behind a Cargo feature flag for re-enablement later — build the Tauri side with `cargo tauri build --features spotify` and set `FEATURES.spotify = true` in `src/features.ts` to bring it back. The instructions below remain valid once the feature is enabled.

RetroAmp can stream music from Spotify, but due to Spotify's API restrictions, each user needs to register their own (free) Spotify Developer App. This is a one-time setup that takes about 2 minutes.

## Why is this needed?

Spotify requires all third-party apps to register with their Developer Platform. In February 2026, Spotify tightened their API restrictions significantly — apps in "Development Mode" are limited to 5 authorized users, and getting beyond that requires a registered business with 250,000+ monthly active users. Until RetroAmp reaches that scale, each user needs their own Developer App registration.

This is the same approach used by other open-source Spotify clients.

## Requirements

- A **Spotify Premium** account (required by Spotify for third-party streaming)
- A free **Spotify Developer** account (uses the same Spotify login)

## Setup Steps

### 1. Create a Spotify Developer App

1. Go to [developer.spotify.com/dashboard](https://developer.spotify.com/dashboard)
2. Log in with your Spotify account
3. Click **"Create App"**
4. Fill in the form:
   - **App name:** `RetroAmp` (or any name you like)
   - **App description:** `Desktop music player`
   - **Redirect URI:** `http://127.0.0.1:8898/login`
   - **Which API/SDKs are you planning to use?** Select **Web API**
   - Check the terms agreement
5. Click **"Save"**

### 2. Copy your Client ID

1. On your new app's page, you'll see a **Client ID** — a long string of letters and numbers
2. Copy it to your clipboard

### 3. Configure RetroAmp

1. Open RetroAmp
2. Go to **Preferences** (right-click the player or press `Ctrl+P`)
3. Click the **Spotify** tab
4. Paste your **Client ID** into the Client ID field
5. Click **"Log In with Spotify"**
6. Your browser will open to Spotify's authorization page — click **"Agree"**
7. You'll be redirected back and RetroAmp will confirm the connection

### 4. Add yourself as a test user (if needed)

If you get a 403 error when trying to browse playlists:

1. Go back to [developer.spotify.com/dashboard](https://developer.spotify.com/dashboard)
2. Select your app
3. Go to **Settings** > **User Management**
4. Add your Spotify account email as a test user

## What works

- **Search** — search for tracks, albums, artists, and playlists (limited to 10 results per page in Development Mode)
- **Your playlists** — browse and play tracks from playlists you own or collaborate on
- **Your library** — browse saved albums and liked songs
- **Playback** — stream any track through RetroAmp's audio pipeline with full EQ, spectrum analyser, and skin support
- **Recently played** — see your recently played tracks
- **Auto-reconnect** — credentials are cached, so you only need to log in once

## Known limitations (Development Mode)

These are Spotify-imposed restrictions on Developer Mode apps, not RetroAmp limitations:

| Limitation | Detail |
|---|---|
| 5 users max | Only 5 Spotify accounts can use your Developer App |
| Search limited to 10 results | Pagination available for more results |
| No artist top tracks | Spotify removed this endpoint in Feb 2026 |
| No new releases / browse | Spotify removed these endpoints in Feb 2026 |
| Other users' playlists | Can only view contents of playlists you own or collaborate on |
| Premium required | App owner must have Spotify Premium |

## Future plans

We're actively working on:

- **YouTube Music integration** — free, unrestricted streaming with no developer setup required. This will be the primary streaming path for most users.
- **Extended Quota Mode** — if RetroAmp grows to sufficient scale as a registered organisation, we'll apply for Spotify's Extended Quota Mode which removes the 5-user cap and other restrictions. This would allow shipping a built-in Client ID so users wouldn't need to register their own app.
- **Subsonic/Navidrome support** — connect to self-hosted music servers for streaming your own library over the network.

## Troubleshooting

**"Invalid limit" errors on search:**
This is normal in Development Mode — search is limited to 10 results per page. RetroAmp handles this automatically.

**403 Forbidden on playlists:**
Make sure you've added your Spotify email as a test user in the Developer Dashboard (Settings > User Management).

**Login opens browser but nothing happens:**
Check that the redirect URI in your app settings is exactly `http://127.0.0.1:8898/login` (no trailing slash).

**Auto-reconnect fails after restart:**
Try logging out (Preferences > Spotify > Log Out) and logging back in to refresh the token.
