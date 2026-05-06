// Frontend feature flags. Mirrors the Cargo features on the Rust side so the
// UI hides functionality whose backend isn't compiled in.
//
// Spotify is currently disabled because Spotify's Web API requires every user
// to register their own Developer App (Dev Mode), which is too much friction
// for a casual desktop player. YouTube Music covers the same use case without
// that step. The code path is preserved for when (or if) Spotify relaxes the
// restriction — flip this to `true` and rebuild the Tauri side with
// `--features spotify` to bring it back.
export const FEATURES = {
  spotify: false,
} as const;
