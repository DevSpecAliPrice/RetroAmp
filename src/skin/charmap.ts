/**
 * Character map for text.bmp — maps characters to pixel positions.
 *
 * text.bmp contains a grid of 5x6 pixel characters. Each entry gives
 * the source x,y position in the BMP for that character.
 *
 * Layout from Webamp's FONT_LOOKUP:
 *   Row 0 (y=0):  a-z, then ", @, and space
 *   Row 1 (y=6):  0-9, then symbols
 *   Row 2 (y=12): International chars (Å Ö Ä ? *)
 */

export const CHAR_WIDTH = 5;
export const CHAR_HEIGHT = 6;

/** Marquee display area in the main window.
 * Y=27 is the consensus position across 80+ classic skins — most skins
 * have their text inset starting at y=26..28, so y=27 sits inside the
 * inset for virtually all of them. */
export const MARQUEE_X = 111;
export const MARQUEE_Y = 27;
export const MARQUEE_WIDTH = 154;
export const MARQUEE_HEIGHT = 6;

/** Characters that fit in the marquee. */
export const MARQUEE_CHARS = Math.floor(MARQUEE_WIDTH / CHAR_WIDTH); // 30

/** Source pixel coordinates in text.bmp for each character. */
const CHAR_MAP: Record<string, { x: number; y: number }> = {};

// Row 0: a-z
const letters = "abcdefghijklmnopqrstuvwxyz";
for (let i = 0; i < letters.length; i++) {
  CHAR_MAP[letters[i]] = { x: i * CHAR_WIDTH, y: 0 };
}
// Row 0 continued: special chars
CHAR_MAP['"'] = { x: 130, y: 0 };
CHAR_MAP["@"] = { x: 135, y: 0 };
// Space is at a known position
CHAR_MAP[" "] = { x: 150, y: 0 };

// Row 1: 0-9
const digits = "0123456789";
for (let i = 0; i < digits.length; i++) {
  CHAR_MAP[digits[i]] = { x: i * CHAR_WIDTH, y: 6 };
}
// Row 1 continued: symbols
const row1Symbols: [string, number][] = [
  ["\u2026", 50], // ellipsis
  [".", 55],
  [":", 60],
  ["(", 65],
  [")", 70],
  ["-", 75],
  ["'", 80],
  ["!", 85],
  ["_", 90],
  ["+", 95],
  ["\\", 100],
  ["/", 105],
  ["[", 110],
  ["]", 115],
  ["^", 120],
  ["&", 125],
  ["%", 130],
  [",", 135],
  ["=", 140],
  ["$", 145],
  ["#", 150],
];
for (const [char, x] of row1Symbols) {
  CHAR_MAP[char] = { x, y: 6 };
}

// Row 2: international characters
CHAR_MAP["\u00c5"] = { x: 0, y: 12 }; // Å
CHAR_MAP["\u00d6"] = { x: 5, y: 12 }; // Ö
CHAR_MAP["\u00c4"] = { x: 10, y: 12 }; // Ä
CHAR_MAP["?"] = { x: 15, y: 12 };
CHAR_MAP["*"] = { x: 20, y: 12 };

/**
 * Get the source coordinates for a character. Falls back to space for
 * unknown characters. Input is case-insensitive (text.bmp only has lowercase).
 */
export function getCharCoords(char: string): { x: number; y: number } {
  const lower = char.toLowerCase();
  return CHAR_MAP[lower] ?? CHAR_MAP[" "] ?? { x: 150, y: 0 };
}
