---
name: Reference projects for implementation guidance
description: Webamp and Strawberry source code available locally as reference for how to handle Winamp skin rendering, audio features, and UI behavior
type: reference
---

Two reference projects are available locally to consult when implementing features:

- **Webamp** (Jordan Eldredge's browser-based Winamp clone): `/home/n3o/Software_Projects/WebAmp/webamp`
  - Authoritative for Winamp 2.x skin sprite positions, layout coordinates, and rendering logic
  - JavaScript/TypeScript — check for pixel positions, sprite offsets, and classic Winamp UI behavior

- **Strawberry** (modern C++/Qt audio player): `/home/n3o/Software_Projects/Strawberry/strawberry`
  - Good reference for audio engine patterns, playlist management, and desktop player UX conventions

**How to apply:** When implementing a Winamp skin feature (sprite positions, button behavior, shade mode, EQ layout, etc.), check Webamp first for the canonical coordinates and behavior. When implementing audio engine or playlist features, Strawberry is a useful second opinion on architecture.
