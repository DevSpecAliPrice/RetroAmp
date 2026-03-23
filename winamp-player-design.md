# RetroAmp
## Project Design & Architecture Document

---

## Overview

**RetroAmp** is an open-source, cross-platform desktop audio player that faithfully recreates the Winamp 2.x experience — complete with replaceable `.wsz` skins, a spectrum analyser, 10-band equalizer, Milkdrop visualiser, and playlist editor — built on modern open-source tooling. It extends the Winamp model with a full library management layer inspired by Strawberry, including tag editing, ratings, smart playlists, and MusicBrainz integration. Future-proofed to support Spotify and internet radio as additional audio sources.

---

## Starting Point: Webamp (Forked, Not Wrapped)

**Webamp** (github.com/captbaritone/webamp) is an MIT-licensed, near-pixel-perfect Winamp 2.x clone built in TypeScript/React that runs in the browser. It supports real `.wsz` skin files, has a working equalizer, playlist editor, and visualisations.

Webamp's npm package is designed for embedding as a self-contained widget — useful for demos, but too opaque for a full desktop application. Deeply integrating custom drawers, Rust-driven playback state, the skin extension system, and a proper audio pipeline would mean constantly fighting the package's API boundaries. The correct approach is to **fork the Webamp repository and extract its key modules as standalone components** that RetroAmp owns and controls directly.

**Modules to extract from the Webamp fork:**

- **`skinParser.ts`** — unzips `.wsz` files, decodes BMPs, maps every sprite region. Weeks of work, battle-tested against thousands of skins. This is the highest-value extraction.
- **The canvas-based sprite rendering system** — the actual rendering logic that blits sprites from parsed skin data onto canvas elements.
- **Playlist data model** — the underlying data structures for playlist state. The UI will be reimplemented to integrate with RetroAmp's state management and Rust backend.

**Modules we do NOT extract (replaced by Rust backend):**

- Web Audio playback — replaced by Symphonia + CPAL in Rust.
- Web Audio EQ (biquad filters) — replaced by Rust-side biquad EQ in the audio pipeline.
- Web Audio `AnalyserNode` — replaced by Rust-side FFT computation, with data pushed to the WebView.

The extracted modules become first-class source code within the RetroAmp repository, not an external dependency. This means they can be modified freely — adding skin extension support, adapting the parser for `gen_colours.ini`, or changing how sprite data flows to the renderer.

**License:** MIT — use it, modify it, redistribute it, build commercial products on top of it. Keep the license notice in RetroAmp's source for all code derived from Webamp.

---

## Recommended Stack

### Frontend
- **TypeScript + React** — Webamp's native environment; forked Webamp modules live here
- **Canvas / WebGL** — sprite rendering from skin BMPs, Butterchurn visualiser
- **Web Audio API** — used only as a **sink** for receiving FFT data from Rust to drive visualisations in the WebView; not used for playback, EQ, or audio processing

### Desktop Shell
- **Tauri v2 (Rust)** — windowing, file system access, OS media keys, system tray, multi-window support
- Preferred over Electron for bundle size and memory footprint. A Winamp clone should feel *light*.
- Tauri v2 specifically for its improved multi-window API, which the window manager depends on.

### Audio Engine (Rust — Primary)
- **Symphonia** — multi-format decoder (MP3, FLAC, AAC, OGG, WAV, ALAC)
- **CPAL** — cross-platform audio output
- **Rust biquad filters** — 10-band EQ implemented in the audio pipeline, not in the WebView
- **Rust FFT** (via `rustfft` or `realfft` crate) — spectrum analysis computed in Rust, pushed to the WebView as typed arrays for visualisation rendering
- The Rust audio engine is the **sole audio pipeline** — all sources (local files, internet radio, Spotify) decode and output through it. This guarantees consistent behaviour: gapless playback, ReplayGain, EQ, and visualisation data work identically regardless of source.

### Source Abstraction
- A common Rust trait (`AudioSource`) defined from day one — local file playback is the first implementation, but the interface is designed so that internet radio, Spotify, and any future source slot in without touching the audio pipeline, EQ, visualisation, or UI layers.

### Media Library
- **SQLite** via Tauri's plugin — track metadata, play counts, library indexing

---

## The Skin System

This is the most architecturally interesting and labour-intensive piece.

### How Winamp Skins Work

Winamp `.wsz` files are ZIP archives containing:

| File | Purpose |
|---|---|
| `main.bmp` | Main window sprite sheet |
| `cbuttons.bmp` | Control buttons (play, pause, stop, etc.) |
| `shufrep.bmp` | Shuffle and repeat buttons |
| `eq_ex.bmp` | Equalizer sprites |
| `pledit.bmp` | Playlist editor sprites |
| `numbers.bmp` | LED display digit font |
| `text.bmp` | Bitmap text font |
| `viscolor.txt` | 24 colours for the spectrum analyser |
| `region.txt` | Defines the non-rectangular window shape |

Every UI element is a **sprite blit** — a specific pixel region cut from a BMP. Button states (up/down/hover), LED display digits, volume knob positions — all sprites.

### Desktop-Specific Enhancement

Tauri allows you to use `region.txt` to shape the **actual OS window** into Winamp's curved outline — something a browser-based Webamp can't do. This is what makes the desktop version feel like the real thing.

### Default Skin

Webamp ships with **no skins bundled** — deliberately. The code implements the skin format, but distributing the actual default Winamp skin would mean distributing Nullsoft/AOL's artwork. You need to ship with at least one default skin you have clear rights to.

Options, in rough order of preference:

- **Design your own** — establishes the app's visual identity from day one, completely clean legally. Time-consuming pixel work but worth it.
- **Commission one** — pay a designer to produce a bespoke default skin.
- **Find a community skin with an explicit permissive license** — rare, needs careful checking.
- **Approach the Winamp Skin Museum community** — some authors might be happy to have their work become a default skin for an open-source project.

Designing a custom default skin is also an opportunity to demonstrate the extension system (see below) by shipping a skin that fully supports the new panels out of the box.

**Resource:** The Winamp Skin Museum at skins.webamp.org has ~65,000 skins, all browsable in the browser via Webamp. Invaluable for testing the skin parser against edge cases, and a good community to engage with at launch.

---

## Skin Extension System

The original Winamp skin spec is a closed set of windows — it was never designed with extensibility in mind. Adding a library browser, tag editor, and other new panels creates a problem: how should they look when the user has loaded a skin you've never seen? This breaks into two sub-problems: **automatic colour derivation** for all existing skins, and an **optional extension spec** for skin authors who want to go further.

### Colour Derivation — Making Every Existing Skin Just Work

The skin already contains enough information to produce a coherent palette for new surfaces. The most useful source is **`pledit.bmp`** — the playlist editor. It's the closest existing window to a library browser and carries exactly the values you need: list background, text colour, selected item highlight, and title bar colour. These map almost directly onto what a library panel or tag editor needs.

At skin load time, the parser samples specific known pixel coordinates from the existing BMPs and emits a set of **CSS custom properties** that all new panels consume:

```
From pledit.bmp:
  [0,0]   → --skin-list-bg          (list/panel background)
  [0,1]   → --skin-list-text        (primary text)
  [0,2]   → --skin-selected-bg      (selected item background)
  [0,3]   → --skin-selected-text    (selected item text)

From main.bmp:
  title bar region → --skin-chrome-bg   (window chrome colour)

From viscolor.txt:
  colour[0]   → --skin-vis-bg       (often a good dark accent)
  colour[11]  → --skin-accent       (often a good highlight colour)
```

This means **every existing skin automatically produces a coherent result** in the new panels with no action from the original skin author. It won't be pixel-perfect, but it will be visually consistent — a dark green skin won't produce a light grey library panel.

### The Extension Spec — For Skin Authors Who Want Full Control

For authors who want to explicitly design for the new panels, you define an optional extension layer on top of the `.wsz` format. Since `.wsz` is just a ZIP, adding new files is non-breaking — Winamp itself, and any other `.wsz`-compatible player, simply ignores files it doesn't recognise.

The `gen_` prefix is already Winamp convention for extended/generic windows, so the naming is consistent with the existing format:

| File | Purpose |
|---|---|
| `gen_colours.ini` | Explicit colour overrides for all derived values |
| `gen_library.bmp` | Sprite sheet for library browser chrome |
| `gen_tagedit.bmp` | Sprite sheet for tag editor chrome |
| `gen_ratings.bmp` | Custom star rating sprites |

If these files are present in the `.wsz`, use them. If absent, fall back to derived colours. The result is a graceful capability ladder — basic support requires nothing, full support means designing a handful of extra files.

**`gen_colours.ini`** is the most important and lowest-effort piece. A simple key-value file giving skin authors explicit control without requiring new BMPs:

```ini
[library]
background=#1a1a1a
text=#c8c8c8
selected_bg=#2d5a8e
selected_text=#ffffff
border=#333333

[tagedit]
background=#1a1a1a
text=#c8c8c8
input_bg=#0d0d0d

[ratings]
filled=#f5c518
empty=#444444
```

A skin author can fully support your new panels with a 10-line `gen_colours.ini` and no new artwork at all.

### Publishing the Spec

Document the full extension system in a `SKIN_EXTENSIONS.md` in the repo:

- The derived colour extraction logic (which pixels, which files, the fallback chain)
- The complete `gen_colours.ini` key reference with descriptions
- The optional BMP extension files and their sprite coordinate maps
- A "minimum effort" guide (just `gen_colours.ini`)
- A "full support" guide (all BMP extensions)

Publishing this clearly invites the Winamp skinning community to extend their existing skins for your app. This community is still active and skin authors often enjoy extending their work — it's good for adoption and creates a natural feedback loop at launch.

### User-Facing Skin Loading

The experience of adding skins should be frictionless:

- **Drag and drop a `.wsz` file** onto the player to load it instantly — no file picker needed, though one should exist too
- **Skins directory** in a well-known platform-appropriate location (`~/.config/retroamp/skins/` on Linux, `%APPDATA%\RetroAmp\skins` on Windows, `~/Library/Application Support/RetroAmp/skins` on macOS) — files dropped there appear in the skin browser automatically
- **Built-in skin browser drawer** — a scrollable grid of skin thumbnails with preview on hover, and a "Get More Skins" link pointing to skins.webamp.org
- **Preview before applying** — hovering a skin in the browser shows it applied to the player in real time, consistent with how the original Winamp worked

---

## Architecture

```
┌────────────────────────────────────────────────────────────────┐
│                      Winamp UI (WebView)                       │
│   Skin renderer · LCD display · Playlist · Drawers             │
│   Quick tag edit · Ratings · Skin browser                      │
│                                                                │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │   FFT Data (received from Rust via Tauri events)         │  │
│  │       ↓                        ↓                         │  │
│  │  Spectrum analyser        Butterchurn (Milkdrop)         │  │
│  │  (in skin vis area)       (own Tauri window)             │  │
│  └──────────────────────────────────────────────────────────┘  │
└───────────────────────────────┬────────────────────────────────┘
                                │ Tauri commands / events
┌───────────────────────────────▼────────────────────────────────┐
│                         Rust Backend                           │
├──────────────────┬──────────────────┬──────────────────────────┤
│  Window Manager  │  Library Manager │   Audio Engine           │
│  Tracks all open │  SQLite index    │   ┌──────────────────┐  │
│  secondary wins  │  Tag read/write  │   │  Source Router    │  │
│  Position/snap   │  File watcher    │   │  (AudioSource     │  │
│  state persist   │  MusicBrainz     │   │   trait)          │  │
│  Z-order         │  AcoustID        │   ├────┬───────┬──────┤  │
│                  │                  │   │Local│Spotify│Radio │  │
│                  │                  │   └──┬─┴───────┴──┬───┘  │
│                  │                  │      ▼            ▼      │
│                  │                  │   Symphonia (decode)     │
│                  │                  │      ▼                   │
│                  │                  │   EQ (Rust biquads)      │
│                  │                  │      ▼                   │
│                  │                  │   FFT (rustfft)──→ UI    │
│                  │                  │      ▼                   │
│                  │                  │   CPAL (output)          │
│                  │                  │   └──────────────────┘  │
└──────────────────┴──────────────────┴──────────────────────────┘
```

### Design Principles

**Rust audio engine is the sole pipeline.** Every audio source — local files, internet radio, Spotify — decodes through Symphonia and outputs through CPAL. The EQ, FFT analysis, gapless playback, and ReplayGain all live in this pipeline and work identically regardless of source. There is no parallel Web Audio path.

**Source abstraction from day one.** All sources implement a common `AudioSource` trait that provides: track metadata, decoded PCM frames, seek capability, and stream state. The audio engine consumes this trait, not concrete source types. Adding a new source means implementing the trait — nothing else in the stack changes.

**FFT data flows from Rust to WebView.** The Rust audio engine computes FFT on the decoded PCM stream and pushes frequency data to the frontend via Tauri events. The WebView receives this as typed arrays and feeds it to both the in-skin spectrum analyser and Butterchurn simultaneously. One FFT computation in Rust, multiple visual consumers in the WebView.

**File tags are always authoritative.** The SQLite library is a cache and index, not the source of truth. Your music survives reinstalls, migrations, and other players because everything important is in the file tags themselves.

---

## Component Scope & Effort

Components are grouped by the phase in which they are built. Phase 1 is the foundation — it includes more components than a minimal prototype would, because the goal is to build the correct architecture once and avoid rework in later phases.

### Phase 1 — Core Player & Foundation

| Component | Effort | Notes |
|---|---|---|
| Tauri v2 + React/TS scaffold | Low | Project structure, build pipeline, dev workflow |
| Rust audio engine (Symphonia + CPAL) | High | Primary audio pipeline; all sources flow through this |
| Source abstraction trait (`AudioSource`) | Medium | Common interface — local files implement first; radio, Spotify slot in later |
| Rust EQ (biquad filters) | Medium | 10-band EQ in the audio pipeline, not WebView |
| Rust FFT → WebView bridge | Medium | `rustfft` computes spectrum data, pushed to frontend via Tauri events |
| Gapless playback | Medium | Pre-decode next track; must be in the engine from the start |
| Skin parser + sprite renderer | High | Forked from Webamp; extracted as standalone modules |
| Skin colour derivation | Low–Medium | Sample known pixels → CSS custom properties |
| Skin extension spec (gen_colours.ini) | Low | Parser reads optional extension files from `.wsz` |
| Main player window | Medium | |
| Playlist editor | Medium | State managed between Rust and frontend |
| Equalizer window | Medium | UI controls mapped to Rust biquad parameters |
| Spectrum analyser | Medium | Renders FFT data received from Rust |
| LCD display + bitmap font | Low–Medium | Sprite blit from `numbers.bmp` / `text.bmp` |
| Window manager (Rust) | Medium | Position persistence, snap-to-dock, state restore, Z-order |
| Window shaping (region.txt) | Medium | Tauri v2 borderless window |
| Skin browser + `.wsz` drag-and-drop | Medium | Thumbnail grid, live preview |
| Custom default skin | Medium | Design work; demonstrates extension system |

### Phase 2 — Internet Radio

| Component | Effort | Notes |
|---|---|---|
| Radio `AudioSource` implementation | Medium | HTTP stream → Symphonia decode → same pipeline as local |
| ICY metadata parsing | Low–Medium | Now-playing info from Shoutcast/Icecast streams |
| Station browser (Radio Browser API) | Medium | Search, favourites, genre filtering |
| M3U / PLS import | Low | Standard station list formats |

### Phase 3 — Library Management & Tag Editing

| Component | Effort | Notes |
|---|---|---|
| Media library + file scanning | Medium | SQLite via Tauri plugin |
| File tag read/write | Medium | lofty crate in Rust |
| Quick tag edit (compact mode) | Medium | Right-click popover, writes to file |
| Star ratings | Low–Medium | POPM tag + SQLite |
| Full tag editor drawer | Medium | Multi-field, bulk edit support |
| Library browser drawer | Medium | Folder tree, search, genre/rating filters |
| Smart playlists | Medium | SQLite query builder UI |
| MusicBrainz / AcoustID lookup | Medium | Audio fingerprint → auto-fill tags |
| Cover art fetching | Low–Medium | MusicBrainz / Last.fm / Discogs |
| Last.fm / ListenBrainz scrobbling | Low | Simple HTTP API |

### Phase 4 — Spotify

| Component | Effort | Notes |
|---|---|---|
| Spotify `AudioSource` (librespot) | High | Implements `AudioSource` trait; decodes through same Rust pipeline |
| Spotify Web API integration | Medium | OAuth, library browsing, search, playlist sync |

### Phase 5 — Milkdrop & CD Ripping

| Component | Effort | Notes |
|---|---|---|
| Milkdrop visualiser (Butterchurn) | Medium–High | WebGL; receives same FFT data from Rust; own Tauri window |
| Milkdrop preset browser | Medium | Preset folder watching, switching, locking |
| CD ripping | Medium | Wrap cdparanoia + FFmpeg as Tauri shell commands |

**Estimated timeline:** Phase 1 is heavier than a minimal prototype — expect 4–5 months solo. But this investment pays back directly: Phases 2–4 each become substantially lighter because they plug into proven infrastructure (audio engine, source trait, window manager, skin system) rather than requiring architectural rework.

---

## Milkdrop Visualiser

Milkdrop was Ryan Geiss's visualiser that shipped with Winamp from version 2.x onwards — the source of those famously trippy, beat-reactive visual landscapes. It ran in its own detachable window that could float freely, resize, or go fullscreen independently of the main player. It is absolutely worth having, and like Webamp, someone has already done the hard reimplementation work.

### Butterchurn

**Butterchurn** (github.com/jberg/butterchurn) is a WebGL reimplementation of Milkdrop written in JavaScript, MIT licensed. It is actually what the Winamp Skin Museum uses for its own visualisations. It supports the original Milkdrop **preset format** — the `.milk` files that define each visual — so the entire existing preset library works out of the box.

This means you are not writing shader code from scratch. The work is integrating Butterchurn, handling the windowing, and building a preset browser — not reimplementing the renderer. This is why Milkdrop moves from "Very High, separate project" in an unassisted build to **Medium–High** here.

### The Preset Ecosystem

Milkdrop presets are community-created visual programs — thousands of them exist, ranging from subtle geometric patterns to deeply unhinged psychedelic landscapes. They are plain text files describing a shader-like script. The **Butterchurn preset pack** ships with a curated collection of classics. Follow the same pattern as skins: bundle a solid selection, watch a presets folder for user additions, let users drop in `.milk` files freely.

### Window Modes

Milkdrop in the original Winamp had three modes, all of which map cleanly onto Tauri's window model:

**Embedded** — the small visualisation area inside the main player skin (~150×150 pixels). Butterchurn renders into a canvas element within the existing WebView. Always available.

**Windowed** — pops out into a second, freely resizable Tauri window. Borderless, optionally stays on top, snaps to the player if desired. Toggle with a double-click on the embedded vis area or a keyboard shortcut.

**Fullscreen** — calls `set_fullscreen()` on the secondary Tauri window. Press Escape to return to windowed mode. The display goes completely over to the visualiser.

The Milkdrop window also had its own minimal chrome worth replicating: a **preset name overlay** that fades in when a preset changes, a beat detection indicator, and a right-click context menu for switching presets, enabling shuffle, or locking to the current preset.

### Audio Data Pipeline

Butterchurn needs FFT data in real time. The Rust audio engine already computes this for the in-skin spectrum analyser and pushes it to the WebView via Tauri events. Butterchurn consumes the same FFT data — one computation in Rust, multiple visual consumers in the frontend. The FFT typed arrays are delivered as Tauri event payloads at ~60fps, which both the spectrum analyser canvas and Butterchurn's WebGL renderer read from a shared frontend state store.

If Butterchurn requires a Web Audio `AnalyserNode` specifically (some versions of the library expect this interface), a thin adapter in the WebView can feed the Rust-sourced FFT data into a Web Audio graph as a pass-through — the data still originates from the Rust pipeline, not from Web Audio decoding.

### The One Caveat

The original Milkdrop presets use a proprietary scripting language that Butterchurn reverse-engineered. Support is very good but occasionally a preset designed for the original DirectX-based Milkdrop will render slightly differently in WebGL. For the vast majority of presets this is unnoticeable.

---

## Window Manager

The Milkdrop window is the most dramatic example of a pattern that runs through the whole application — multiple secondary windows that each have their own position, visibility state, and relationship to the main player. The EQ, playlist, library browser, tag editor, skin browser, and Milkdrop window all follow this pattern. Without a unified approach, each becomes a one-off with its own position persistence and snap logic bolted on separately.

A **Window Manager** in the Rust backend is the right answer. It owns the state of all secondary windows and is the single place where window behaviour is defined:

```
Window Manager responsibilities:
  - Track open/closed state of every secondary window
  - Persist window positions across sessions (SQLite)
  - Snap behaviour — windows magnetise to each other and to screen edges
  - Bring-all-to-front when the main player is focused
  - Restore full layout on launch
  - Z-order management for floating windows
```

This mirrors how the original Winamp actually worked — it had a window manager internally that kept the EQ and playlist snapped to the main player as you dragged it around. Getting this right makes the whole app feel coherent rather than like a collection of independent panels. It's also the kind of thing that's much easier to design once upfront than to retrofit later when you have six window types all doing their own thing.

---

## Internet Radio

Low complexity, no legal complications, and very on-brand — Winamp popularised internet radio in the late 90s via SHOUTcast (also built by Nullsoft).

### How It Works

Internet radio streams are HTTP audio streams (MP3, AAC, or Ogg Vorbis delivered continuously). In RetroAmp, a Radio `AudioSource` implementation handles the HTTP connection in Rust, feeds the continuous byte stream into Symphonia for decoding, and outputs PCM through the same audio pipeline as local files. This means internet radio automatically gets EQ, spectrum analysis, and Milkdrop visualisation — no special cases needed.

### Ecosystem

- **ICY metadata** — the Shoutcast/Icecast protocol for embedding "now playing" track info inline in the stream. Libraries exist for parsing this.
- **Radio Browser API** (radio-browser.info) — community-maintained, open API with ~30,000 stations. Free, no auth required. Perfect for a built-in station browser.
- **M3U / PLS playlists** — standard formats for radio station URLs, worth supporting for importing station lists.

---

## Spotify Integration

Two distinct integration paths with different tradeoffs:

### Option A: Spotify Connect via librespot

**librespot** is an open-source Spotify client library written in Rust (MIT licensed) used by most open-source Spotify clients. It makes your app appear as a playback target in Spotify's "Connect to a device" list.

- Requires Spotify Premium
- Slots naturally into a Tauri/Rust backend
- Operates in a **legal grey area** — Spotify doesn't officially sanction third-party clients using their streaming infrastructure. This is a known risk accepted by the open-source community, but worth understanding.

### Option B: Spotify Web API (Official)

OAuth login gives access to search, browse, user library, and playback control. However, **audio streaming is not available** — you can only control playback on official Spotify apps/devices.

Useful for: pulling playlist data, showing what's currently playing, controlling a phone that's doing the actual playback.

### Recommended Approach

Combine both: **Web API for metadata and library browsing, librespot for audio streaming.** This is the pattern used by most open-source Spotify clients (e.g. Spotifyd, Auryo).

In RetroAmp's architecture, librespot implements the `AudioSource` trait — it provides decoded PCM frames that flow through the same Rust audio pipeline as local files and internet radio. The EQ, FFT analysis, gapless transitions, and visualisations all work identically. The Web API integration is a separate concern: OAuth login, library browsing, search, and playlist sync feed into the UI layer, while the audio path goes through the source abstraction.

The Winamp UI maps onto Spotify naturally — the playlist editor becomes the queue, the main display shows track info, and a simple panel handles library browsing and search.

---

## Recommended Build Sequence

The guiding principle: **build the correct architecture once.** Phase 1 is deliberately heavier than a minimum viable prototype because it establishes every foundational system — audio engine, source abstraction, window manager, skin system — so that later phases plug in cleanly rather than requiring rework.

### Phase 1: Core Player & Foundation
The Rust audio engine (Symphonia → EQ → FFT → CPAL), the `AudioSource` trait with a local file implementation, the window manager, and the forked Webamp skin system. This is not a thin shell around a WebView playing audio — it is the full desktop audio pipeline with the skin-based UI on top. By the end of Phase 1, the player has: local file playback through Rust, a working 10-band EQ, spectrum analyser driven by Rust FFT data, `.wsz` skin support with the colour derivation system, playlist management, window snapping and position persistence, and a skin browser. Every subsequent phase builds on this without replacing any of it.

### Phase 2: Internet Radio
A new `AudioSource` implementation for HTTP streams. The Rust audio engine already handles decode, EQ, FFT, and output — the radio source just provides a continuous byte stream. Add ICY metadata parsing for now-playing info and a station browser backed by the Radio Browser API. Low complexity because the infrastructure is already in place.

### Phase 3: Library Management & Tag Editing
Tag editing, ratings, library browser, smart playlists, cover art, scrobbling, MusicBrainz lookup. The Strawberry-equivalent layer that turns a player into a proper music manager. The window manager already handles the new drawers (library browser, tag editor). The skin colour derivation system already provides theming.

### Phase 4: Spotify
A Spotify `AudioSource` implementation via librespot, plus Web API integration for library browsing and search. The audio flows through the same Rust pipeline — same EQ, same visualiser, same gapless behaviour. Most complex due to legal nuance and Premium account requirement, but architecturally straightforward because the source abstraction is already proven.

### Phase 5: Milkdrop & CD Ripping
Butterchurn integration in its own Tauri window, consuming the FFT data the Rust engine already produces. Preset browser with folder watching. CD ripping via cdparanoia + FFmpeg as Tauri shell commands. Milkdrop is deferred not because it is architecturally complex — the FFT bridge exists from Phase 1 — but because it is a large, independent feature that benefits from a stable player.

---

## Library Management & Tag Editing

This is the Strawberry-inspired layer that turns a player into a proper music manager. The guiding philosophy: **file tags are always the source of truth.** The SQLite library is a cache and index, not where metadata lives. Your music survives reinstalls, migrations, and other players because everything important is in the files themselves.

### Tag Editing — Core Principles

- **Always write to file.** Not just to SQLite. Tags travel with the music.
- **Write on confirm, never on keystroke.** Explicit, never surprising.
- **Atomic writes.** Write to a temp file, verify integrity, then replace. Tag write failures that corrupt files are unforgivable.
- **Backup before writing.** At minimum keep the original tags in memory for undo. Optionally offer `.bak` sidecar files for the cautious.
- **Format awareness.** ID3v2.3, ID3v2.4, Vorbis Comments, MP4 atoms, APEv2 — the **lofty** crate (Rust) handles all of these. The UI should surface which format a file uses and flag when a field isn't supported by that format.

**Ratings** are the one nuance: ID3 has a POPM frame but it's poorly standardised across players. Best approach is to store ratings in both the file (POPM) and SQLite, with the file as authoritative and the DB as the fast query layer.

### Compact Mode — Quick Editing in the Main Player

The main Winamp window is small, but that's no reason to lock out tag editing. The interaction pattern is right-click and hover rather than opening a full panel:

- **Right-click on the track title** → inline popover for the most common fields: title, artist, album, genre, year, rating. Writes to file on confirm. Often faster than opening a full editor for a quick fix.
- **Star rating widget** in the main display — five small stars, always visible or on hover, adjacent to the time display. Click to rate, writes POPM to file immediately.
- **Genre badge** — a small tappable pill showing the current genre. Click to change via a compact dropdown.
- **Scrolling title cycling** — the existing Winamp marquee can cycle through title → artist → album → file path, configurable per user preference.

The design goal: a power user managing a library should never *need* to open the full editor for routine edits. The compact interactions handle 80% of day-to-day tagging without breaking flow.

### The Expandable Drawer System

Drawers snap onto the player in the same way Winamp's EQ and playlist always have — each independently toggleable, stackable, or detachable as floating windows. The skin system already supports this since EQ and playlist had their own BMP sprite sheets.

```
┌─────────────────────┐
│   Main Player       │  ← always present; ratings + quick tag edit in compact mode
├─────────────────────┤
│   Playlist          │  ← snaps below (existing Winamp behaviour)
├─────────────────────┤
│   Library Browser   │  ← new drawer: folder tree, search, filter by genre/rating/year
├─────────────────────┤
│   Tag Editor        │  ← new drawer: full multi-field editor, bulk edit, MusicBrainz
├─────────────────────┤
│   Skin Browser      │  ← new drawer: thumbnail grid, live preview, drag-and-drop loading
└─────────────────────┘
```

### Full Tag Editor Drawer

For when you need more than a quick fix:

- Multi-select in the playlist or library view feeds the editor
- **Field-level "apply to selection" toggles** — set album and year across 12 tracks without wiping their individual titles
- Auto-capitalisation and track number sequencing for batch operations
- MusicBrainz lookup on selection — fetch and preview before committing any writes
- Cover art: embed into file or store as `folder.jpg` / `cover.jpg` alongside the files (configurable)

### Library Browser Drawer

- Folder tree view and/or artist/album/genre hierarchy
- Full-text search across all indexed fields
- Filter by genre, rating, year, play count, date added
- **Smart playlists** — dynamic playlists driven by SQLite rules, e.g. "genre = Jazz AND rating ≥ 4 AND not played in 30 days". Saved and auto-updating.
- File watching — monitors library folders for changes and updates the index automatically

### Additional Library Features

**MusicBrainz / AcoustID integration** — identify tracks by audio fingerprint and auto-fill tags from MusicBrainz. The AcoustID API is free and open. Invaluable for messy or untagged libraries.

**Cover art fetching** — pull from MusicBrainz, Last.fm, or Discogs. Store locally in the library database or embed in files.

**Scrobbling** — Last.fm and/or ListenBrainz (the open-source alternative, arguably more aligned with an open-source project's values). Simple HTTP API.

**Lyrics** — fetch from an open source like lrclib.net. Could render as a scrolling panel, or even as a separate Winamp-style shade window.

---

## Packaging & Distribution

### What Tauri Builds Automatically

Running `tauri build` on any platform produces native packages for that platform with no extra configuration:

| Platform | Output formats |
|---|---|
| Linux | `.deb`, `.rpm`, `.AppImage` |
| Windows | NSIS installer (`.exe`), `.msi` |
| macOS | `.dmg`, `.app` bundle |

All formats land in `target/release/bundle/` and are ready to distribute.

### Cross-Platform Builds via GitHub Actions

You cannot cross-compile for macOS from Linux — Apple's toolchain is tied to macOS and the SDK has legal restrictions around redistribution. You can cross-compile for Windows from Linux using `cargo-xwin`, but it's fiddly. The clean solution, and the standard approach for open-source Tauri projects, is **GitHub Actions.**

Tauri maintains an official GitHub Actions workflow. Set it up once and every release tag triggers three parallel runners:

```
git tag v1.0.0 && git push --tags

GitHub Actions matrix:
  ubuntu-latest   → .deb, .rpm, .AppImage
  windows-latest  → .exe, .msi
  macos-latest    → .dmg, .app
```

All three artifacts are automatically attached to the GitHub Release. Users go to the Releases page and download whatever suits their platform. **This is free for public open-source repositories** — runner minutes cost nothing. The workflow file is about 50 lines of YAML; Tauri's documentation includes a ready-made copy-paste version.

### Linux

Three formats out of the box, serving different audiences:

**AppImage** — a single self-contained file that runs on any distro with no install. Users download, `chmod +x`, and run. No root required, no package manager involved. Best for users who want to try it without committing, and the most universal Linux download.

**.deb** — for Debian/Ubuntu/Mint users. Installs properly, appears in the app menu, uninstalls cleanly via the package manager.

**.rpm** — for Fedora/openSUSE/RHEL users.

**Flatpak / Flathub** — not built into Tauri but very achievable. Write a Flatpak manifest wrapping the AppImage or binary, test locally with `flatpak-builder`, then submit to Flathub. Flathub is the closest thing Linux has to a universal app store — it covers all major distros and handles automatic updates. Worth targeting once the app is stable. The submission process involves a review but is not onerous for a well-behaved app.

**Snap** — Tauri supports it but Snap is increasingly unpopular in the Linux community. Probably not worth the effort.

### Windows

The NSIS installer Tauri produces is the standard `.exe` experience — next, next, finish, desktop shortcut, uninstall from control panel. Covers the vast majority of users. The `.msi` is available for enterprise environments that require it.

**SmartScreen warning:** Windows will show a "Windows protected your PC" dialog for unsigned binaries from unknown publishers. A code signing certificate (around £60–£200/year from a certificate authority) eliminates this. Not required for an early open-source release, but worth addressing before wide distribution.

**Microsoft Store** (MSIX format) is optional and requires a developer account. Not worth the overhead early on.

### macOS

The `.dmg` Tauri produces gives users the standard drag-to-Applications experience. However, **Gatekeeper** will warn that the app is from an unidentified developer unless it is signed and notarised. Users can bypass the warning, but it's friction.

To avoid it you need an **Apple Developer account (£79/year)**. Tauri's GitHub Actions workflow has built-in support for signing and notarisation — add your certificates as GitHub secrets and the build process handles the rest automatically.

**Homebrew Cask** is worth adding once you have a stable release. It lets macOS users install with `brew install --cask retroamp` and is straightforward to submit for open-source projects.

### Local Development Workflow

For day-to-day development on Linux:

```bash
tauri dev        # hot-reloading dev mode — WebView + Rust backend live together
tauri build      # produces .deb, .rpm, .AppImage in target/release/bundle/
```

The GitHub Actions pipeline is only needed for cutting actual releases. Local builds give you Linux packages immediately. The `.deb` output can be installed directly with `dpkg -i` to test the full installed experience on your own machine.

### Distribution Summary

| Platform | Primary format | Distribution channel | Signing cost |
|---|---|---|---|
| Linux | AppImage + .deb | GitHub Releases + Flathub | Free |
| Windows | NSIS .exe | GitHub Releases | ~£100/yr (optional) |
| macOS | .dmg | GitHub Releases + Homebrew Cask | £79/yr (Apple) |

---

## Legal Considerations

### The Name — RetroAmp

"RetroAmp" is clean from a trademark perspective. The core test in trademark law is likelihood of consumer confusion — would someone mistake RetroAmp for Winamp as a product? The names are sufficiently distinct that this is very unlikely. What to avoid is anything incorporating "Winamp" directly (e.g. "WinAmp Classic", "WinPlayer") or anything confusingly similar to it. Radionomy, who currently own the Winamp trademark, have an actively maintained product and an active trademark.

**Before fully committing to the name, run a trademark search** — it takes about 20 minutes and covers the main risk:

- **UK IPO** — ipo.gov.uk/tmtext
- **EUIPO** — euipo.europa.eu/eSearch
- **USPTO** — tmsearch.uspto.gov

Describing format compatibility as "supports Winamp `.wsz` skins" in documentation and marketing is fine — that is descriptive use, not trademark use.

### Visual Similarity and UI Layout

The overall look of a player with a main window, EQ, and playlist — the general proportions and layout language — is not protectable. You cannot copyright a functional interface layout. There is a long history of media players borrowing Winamp's general aesthetic (XMMS, BMP, Audacious) and none have faced successful legal action over it. Winamp itself borrowed from hardware equalizer and stereo system aesthetics.

The specific default skin artwork — the pixel art of the original Winamp skin, its exact button designs, the specific green-on-black colour scheme — is copyrightable as original artistic work. This is why RetroAmp ships with its own original default skin rather than Winamp's. As long as the default skin is original work, you are fully clear even if it follows the same general visual vocabulary.

### The Skin Format

The `.wsz` format — the ZIP structure, BMP sprite convention, file naming — is a file format, not a copyrightable work. Implementing a parser for it is equivalent to implementing an MP3 decoder. You are not copying code; you are implementing a documented, reverse-engineered specification. This is well-established in software law.

### The Webamp Code

MIT licensed. Use it, modify it, redistribute it, build on it. The only obligation is keeping the MIT license notice in RetroAmp's source. This is already standard practice in the project.

### Winamp's Current Status

Winamp is an actively maintained product as of 2026, owned by Radionomy. It is not abandoned software. This means the trademark is actively maintained and should be treated accordingly — but it has no bearing on RetroAmp's right to exist as an independent open-source project that supports the same skin format.

### Passing Off

Include a clear statement in the README and About screen that RetroAmp is an independent open-source project, not affiliated with or endorsed by Winamp or Radionomy. This eliminates any passing-off risk immediately and is standard practice for projects in this space. Something like:

> *RetroAmp is an independent open-source project and is not affiliated with, endorsed by, or connected to Winamp or Radionomy. Winamp is a trademark of Radionomy. RetroAmp supports the Winamp `.wsz` skin format for compatibility with community-created skins.*

### Summary

| Area | Status | Action required |
|---|---|---|
| Name (RetroAmp) | Clean in principle | Run trademark searches before committing |
| UI layout / design language | Not protectable | None |
| Default skin artwork | Must be original | Design own default skin ✓ |
| `.wsz` skin format | File format, not copyrightable | None |
| Webamp code | MIT licensed | Keep license notice in source ✓ |
| Winamp trademark in marketing | Descriptive use only | ✓ as long as not passing off |
| Affiliation disclaimer | Best practice | Add to README and About screen |

---

## Key References

| Resource | URL | Notes |
|---|---|---|
| Butterchurn | github.com/jberg/butterchurn | WebGL Milkdrop reimplementation |
| Webamp | github.com/captbaritone/webamp | Forked for skin parser + sprite renderer |
| Winamp Skin Museum | skins.webamp.org | ~65,000 skins for testing |
| Radio Browser | radio-browser.info | Open station directory API |
| librespot | github.com/librespot-org/librespot | Rust Spotify client library |
| Symphonia | github.com/pdeljanov/Symphonia | Rust audio decoder (primary audio pipeline) |
| CPAL | github.com/RustAudio/cpal | Cross-platform audio output |
| rustfft | github.com/ejmahler/RustFFT | FFT computation for spectrum/visualisation data |
| Tauri v2 | tauri.app | Desktop shell framework (v2 for multi-window support) |
| lofty | github.com/Serial-ATA/lofty-rs | Rust tag read/write (ID3, Vorbis, MP4, APE) |
| AcoustID | acoustid.org | Audio fingerprint API for track identification |
| MusicBrainz | musicbrainz.org | Open music metadata database |
| ListenBrainz | listenbrainz.org | Open-source scrobbling |
| lrclib | lrclib.net | Open lyrics API |
| Flathub | flathub.org | Linux universal app store — target for stable release |
| Tauri GitHub Actions | tauri.app/guides/distribution/publishing | Official CI/CD workflow reference |

---

*RetroAmp design document. Last updated: 22 March 2026.*
