/**
 * Sprite coordinate definitions for Winamp 2.x skins.
 *
 * Each sprite maps to a region within a BMP sprite sheet. The source BMP
 * is identified by the object key (e.g. CBUTTONS sprites come from cbuttons.bmp).
 *
 * Coordinates ported from Webamp's skinSprites.ts.
 */

export interface Sprite {
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

// -- MAIN.BMP --
export const MAIN: Sprite[] = [
  { name: "MAIN_WINDOW_BACKGROUND", x: 0, y: 0, width: 275, height: 116 },
];

// -- TITLEBAR.BMP --
export const TITLEBAR: Sprite[] = [
  { name: "MAIN_TITLE_BAR", x: 27, y: 15, width: 275, height: 14 },
  { name: "MAIN_TITLE_BAR_SELECTED", x: 27, y: 0, width: 275, height: 14 },
  { name: "MAIN_SHADE_BACKGROUND", x: 27, y: 29, width: 275, height: 14 },
  { name: "MAIN_SHADE_BACKGROUND_SELECTED", x: 27, y: 42, width: 275, height: 14 },
  // Title bar buttons (selected = active/pressed)
  { name: "MAIN_OPTIONS_BUTTON", x: 0, y: 0, width: 9, height: 9 },
  { name: "MAIN_OPTIONS_BUTTON_SELECTED", x: 0, y: 9, width: 9, height: 9 },
  { name: "MAIN_MINIMIZE_BUTTON", x: 9, y: 0, width: 9, height: 9 },
  { name: "MAIN_MINIMIZE_BUTTON_SELECTED", x: 9, y: 9, width: 9, height: 9 },
  { name: "MAIN_SHADE_BUTTON", x: 0, y: 18, width: 9, height: 9 },
  { name: "MAIN_SHADE_BUTTON_SELECTED", x: 9, y: 18, width: 9, height: 9 },
  { name: "MAIN_CLOSE_BUTTON", x: 18, y: 0, width: 9, height: 9 },
  { name: "MAIN_CLOSE_BUTTON_SELECTED", x: 18, y: 9, width: 9, height: 9 },
];

// -- CBUTTONS.BMP -- Control buttons
export const CBUTTONS: Sprite[] = [
  // Previous
  { name: "MAIN_PREVIOUS_BUTTON", x: 0, y: 0, width: 23, height: 18 },
  { name: "MAIN_PREVIOUS_BUTTON_SELECTED", x: 0, y: 18, width: 23, height: 18 },
  // Play
  { name: "MAIN_PLAY_BUTTON", x: 23, y: 0, width: 23, height: 18 },
  { name: "MAIN_PLAY_BUTTON_SELECTED", x: 23, y: 18, width: 23, height: 18 },
  // Pause
  { name: "MAIN_PAUSE_BUTTON", x: 46, y: 0, width: 23, height: 18 },
  { name: "MAIN_PAUSE_BUTTON_SELECTED", x: 46, y: 18, width: 23, height: 18 },
  // Stop
  { name: "MAIN_STOP_BUTTON", x: 69, y: 0, width: 23, height: 18 },
  { name: "MAIN_STOP_BUTTON_SELECTED", x: 69, y: 18, width: 23, height: 18 },
  // Next
  { name: "MAIN_NEXT_BUTTON", x: 92, y: 0, width: 22, height: 18 },
  { name: "MAIN_NEXT_BUTTON_SELECTED", x: 92, y: 18, width: 22, height: 18 },
  // Eject
  { name: "MAIN_EJECT_BUTTON", x: 114, y: 0, width: 22, height: 16 },
  { name: "MAIN_EJECT_BUTTON_SELECTED", x: 114, y: 16, width: 22, height: 16 },
];

// -- NUMBERS.BMP -- Time display digits
export const NUMBERS: Sprite[] = [
  { name: "DIGIT_0", x: 0, y: 0, width: 9, height: 13 },
  { name: "DIGIT_1", x: 9, y: 0, width: 9, height: 13 },
  { name: "DIGIT_2", x: 18, y: 0, width: 9, height: 13 },
  { name: "DIGIT_3", x: 27, y: 0, width: 9, height: 13 },
  { name: "DIGIT_4", x: 36, y: 0, width: 9, height: 13 },
  { name: "DIGIT_5", x: 45, y: 0, width: 9, height: 13 },
  { name: "DIGIT_6", x: 54, y: 0, width: 9, height: 13 },
  { name: "DIGIT_7", x: 63, y: 0, width: 9, height: 13 },
  { name: "DIGIT_8", x: 72, y: 0, width: 9, height: 13 },
  { name: "DIGIT_9", x: 81, y: 0, width: 9, height: 13 },
  { name: "MINUS_SIGN", x: 20, y: 6, width: 5, height: 1 },
  { name: "NO_MINUS_SIGN", x: 9, y: 6, width: 5, height: 1 },
];

// -- NUMS_EX.BMP -- Extended time display digits (overrides NUMBERS if present)
export const NUMS_EX: Sprite[] = [
  { name: "DIGIT_0_EX", x: 0, y: 0, width: 9, height: 13 },
  { name: "DIGIT_1_EX", x: 9, y: 0, width: 9, height: 13 },
  { name: "DIGIT_2_EX", x: 18, y: 0, width: 9, height: 13 },
  { name: "DIGIT_3_EX", x: 27, y: 0, width: 9, height: 13 },
  { name: "DIGIT_4_EX", x: 36, y: 0, width: 9, height: 13 },
  { name: "DIGIT_5_EX", x: 45, y: 0, width: 9, height: 13 },
  { name: "DIGIT_6_EX", x: 54, y: 0, width: 9, height: 13 },
  { name: "DIGIT_7_EX", x: 63, y: 0, width: 9, height: 13 },
  { name: "DIGIT_8_EX", x: 72, y: 0, width: 9, height: 13 },
  { name: "DIGIT_9_EX", x: 81, y: 0, width: 9, height: 13 },
  { name: "MINUS_SIGN_EX", x: 90, y: 0, width: 9, height: 13 },
  { name: "NO_MINUS_SIGN_EX", x: 99, y: 0, width: 9, height: 13 },
];

// -- PLAYPAUS.BMP -- Play/pause/stop indicators
export const PLAYPAUS: Sprite[] = [
  { name: "MAIN_PLAYING_INDICATOR", x: 0, y: 0, width: 9, height: 9 },
  { name: "MAIN_PAUSED_INDICATOR", x: 9, y: 0, width: 9, height: 9 },
  { name: "MAIN_STOPPED_INDICATOR", x: 18, y: 0, width: 9, height: 9 },
  { name: "MAIN_NOT_WORKING_INDICATOR", x: 27, y: 0, width: 3, height: 9 },
  { name: "MAIN_WORKING_INDICATOR", x: 30, y: 0, width: 3, height: 9 },
];

// -- POSBAR.BMP -- Position/seek bar
export const POSBAR: Sprite[] = [
  { name: "MAIN_POSITION_SLIDER_BACKGROUND", x: 0, y: 0, width: 248, height: 10 },
  { name: "MAIN_POSITION_SLIDER_THUMB", x: 248, y: 0, width: 29, height: 10 },
  { name: "MAIN_POSITION_SLIDER_THUMB_SELECTED", x: 278, y: 0, width: 29, height: 10 },
];

// -- VOLUME.BMP -- Volume slider
export const VOLUME: Sprite[] = [
  { name: "MAIN_VOLUME_BACKGROUND", x: 0, y: 0, width: 68, height: 420 },
  { name: "MAIN_VOLUME_THUMB", x: 15, y: 422, width: 14, height: 11 },
  { name: "MAIN_VOLUME_THUMB_SELECTED", x: 0, y: 422, width: 14, height: 11 },
];

// -- BALANCE.BMP -- Balance slider
export const BALANCE: Sprite[] = [
  { name: "MAIN_BALANCE_BACKGROUND", x: 9, y: 0, width: 38, height: 420 },
  { name: "MAIN_BALANCE_THUMB", x: 15, y: 422, width: 14, height: 11 },
  { name: "MAIN_BALANCE_THUMB_SELECTED", x: 0, y: 422, width: 14, height: 11 },
];

// -- SHUFREP.BMP -- Shuffle, repeat, EQ, playlist buttons
export const SHUFREP: Sprite[] = [
  // Shuffle button states
  { name: "MAIN_SHUFFLE_BUTTON", x: 28, y: 0, width: 47, height: 15 },
  { name: "MAIN_SHUFFLE_BUTTON_SELECTED", x: 28, y: 15, width: 47, height: 15 },
  { name: "MAIN_SHUFFLE_BUTTON_ACTIVE", x: 28, y: 30, width: 47, height: 15 },
  { name: "MAIN_SHUFFLE_BUTTON_ACTIVE_SELECTED", x: 28, y: 45, width: 47, height: 15 },
  // Repeat button states
  { name: "MAIN_REPEAT_BUTTON", x: 0, y: 0, width: 28, height: 15 },
  { name: "MAIN_REPEAT_BUTTON_SELECTED", x: 0, y: 15, width: 28, height: 15 },
  { name: "MAIN_REPEAT_BUTTON_ACTIVE", x: 0, y: 30, width: 28, height: 15 },
  { name: "MAIN_REPEAT_BUTTON_ACTIVE_SELECTED", x: 0, y: 45, width: 28, height: 15 },
  // EQ button
  { name: "MAIN_EQ_BUTTON", x: 0, y: 61, width: 23, height: 12 },
  { name: "MAIN_EQ_BUTTON_SELECTED", x: 46, y: 61, width: 23, height: 12 },
  { name: "MAIN_EQ_BUTTON_ACTIVE", x: 0, y: 73, width: 23, height: 12 },
  { name: "MAIN_EQ_BUTTON_ACTIVE_SELECTED", x: 46, y: 73, width: 23, height: 12 },
  // Playlist button
  { name: "MAIN_PLAYLIST_BUTTON", x: 23, y: 61, width: 23, height: 12 },
  { name: "MAIN_PLAYLIST_BUTTON_SELECTED", x: 69, y: 61, width: 23, height: 12 },
  { name: "MAIN_PLAYLIST_BUTTON_ACTIVE", x: 23, y: 73, width: 23, height: 12 },
  { name: "MAIN_PLAYLIST_BUTTON_ACTIVE_SELECTED", x: 69, y: 73, width: 23, height: 12 },
];

// -- MONOSTER.BMP --
export const MONOSTER: Sprite[] = [
  { name: "MAIN_STEREO", x: 0, y: 12, width: 29, height: 12 },
  { name: "MAIN_STEREO_SELECTED", x: 0, y: 0, width: 29, height: 12 },
  { name: "MAIN_MONO", x: 29, y: 12, width: 27, height: 12 },
  { name: "MAIN_MONO_SELECTED", x: 29, y: 0, width: 27, height: 12 },
];

// -- EQMAIN.BMP -- Equalizer window
export const EQMAIN: Sprite[] = [
  // Background
  { name: "EQ_WINDOW_BACKGROUND", x: 0, y: 0, width: 275, height: 116 },
  // Title bar
  { name: "EQ_TITLE_BAR", x: 0, y: 149, width: 275, height: 14 },
  { name: "EQ_TITLE_BAR_SELECTED", x: 0, y: 134, width: 275, height: 14 },
  // Title bar buttons
  { name: "EQ_CLOSE_BUTTON", x: 0, y: 116, width: 9, height: 9 },
  { name: "EQ_CLOSE_BUTTON_ACTIVE", x: 0, y: 125, width: 9, height: 9 },
  // ON button (4 states)
  { name: "EQ_ON_BUTTON", x: 10, y: 119, width: 26, height: 12 },
  { name: "EQ_ON_BUTTON_DEPRESSED", x: 128, y: 119, width: 26, height: 12 },
  { name: "EQ_ON_BUTTON_SELECTED", x: 69, y: 119, width: 26, height: 12 },
  { name: "EQ_ON_BUTTON_SELECTED_DEPRESSED", x: 187, y: 119, width: 26, height: 12 },
  // AUTO button (4 states)
  { name: "EQ_AUTO_BUTTON", x: 36, y: 119, width: 32, height: 12 },
  { name: "EQ_AUTO_BUTTON_DEPRESSED", x: 154, y: 119, width: 32, height: 12 },
  { name: "EQ_AUTO_BUTTON_SELECTED", x: 95, y: 119, width: 32, height: 12 },
  { name: "EQ_AUTO_BUTTON_SELECTED_DEPRESSED", x: 213, y: 119, width: 32, height: 12 },
  // Slider sprite sheet (28 frames: 14 columns × 2 rows, each 15×65)
  { name: "EQ_SLIDER_BACKGROUND", x: 13, y: 164, width: 209, height: 129 },
  // Slider thumb
  { name: "EQ_SLIDER_THUMB", x: 0, y: 164, width: 11, height: 11 },
  { name: "EQ_SLIDER_THUMB_SELECTED", x: 0, y: 176, width: 11, height: 11 },
  // Presets button
  { name: "EQ_PRESETS_BUTTON", x: 224, y: 164, width: 44, height: 12 },
  { name: "EQ_PRESETS_BUTTON_SELECTED", x: 224, y: 176, width: 44, height: 12 },
  // EQ graph
  { name: "EQ_GRAPH_BACKGROUND", x: 0, y: 294, width: 113, height: 19 },
  { name: "EQ_GRAPH_LINE_COLORS", x: 115, y: 294, width: 1, height: 19 },
  { name: "EQ_PREAMP_LINE", x: 0, y: 314, width: 113, height: 1 },
];

// -- PLEDIT.BMP -- Playlist window
export const PLEDIT: Sprite[] = [
  // Title bar — selected (active/focused)
  { name: "PL_TOP_LEFT_SELECTED", x: 0, y: 0, width: 25, height: 20 },
  { name: "PL_TITLE_BAR_SELECTED", x: 26, y: 0, width: 100, height: 20 },
  { name: "PL_TOP_TILE_SELECTED", x: 127, y: 0, width: 25, height: 20 },
  { name: "PL_TOP_RIGHT_SELECTED", x: 153, y: 0, width: 25, height: 20 },
  // Title bar — inactive
  { name: "PL_TOP_LEFT", x: 0, y: 21, width: 25, height: 20 },
  { name: "PL_TITLE_BAR", x: 26, y: 21, width: 100, height: 20 },
  { name: "PL_TOP_TILE", x: 127, y: 21, width: 25, height: 20 },
  { name: "PL_TOP_RIGHT", x: 153, y: 21, width: 25, height: 20 },
  // Left/right edge tiles (tile vertically)
  { name: "PL_LEFT_TILE", x: 0, y: 42, width: 12, height: 29 },
  { name: "PL_RIGHT_TILE", x: 31, y: 42, width: 20, height: 29 },
  // Scrollbar handle
  { name: "PL_SCROLL_HANDLE", x: 52, y: 53, width: 8, height: 18 },
  { name: "PL_SCROLL_HANDLE_SELECTED", x: 61, y: 53, width: 8, height: 18 },
  // Window control buttons (selected state)
  { name: "PL_CLOSE_SELECTED", x: 52, y: 42, width: 9, height: 9 },
  { name: "PL_SHADE_SELECTED", x: 62, y: 42, width: 9, height: 9 },
  // Bottom bar
  { name: "PL_BOTTOM_LEFT", x: 0, y: 72, width: 125, height: 38 },
  { name: "PL_BOTTOM_RIGHT", x: 126, y: 72, width: 150, height: 38 },
  { name: "PL_BOTTOM_TILE", x: 179, y: 0, width: 25, height: 38 },
];

/**
 * Map from BMP filename key (lowercase, no extension) to its sprite definitions.
 */
export const SPRITE_SHEETS: Record<string, Sprite[]> = {
  main: MAIN,
  titlebar: TITLEBAR,
  cbuttons: CBUTTONS,
  numbers: NUMBERS,
  nums_ex: NUMS_EX,
  playpaus: PLAYPAUS,
  posbar: POSBAR,
  volume: VOLUME,
  balance: BALANCE,
  shufrep: SHUFREP,
  monoster: MONOSTER,
  eqmain: EQMAIN,
  pledit: PLEDIT,
};
