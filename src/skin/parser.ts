/**
 * Skin parser — takes raw skin contents from the Rust loader and produces
 * a fully parsed SkinData object ready for rendering.
 */

import { invoke } from "@tauri-apps/api/core";
import { SPRITE_SHEETS, type Sprite } from "./sprites";

// -- Types --

export interface PlaylistStyle {
  normal: string;
  current: string;
  normalbg: string;
  selectedbg: string;
  font: string;
}

export interface SkinData {
  /** Map of sprite name → data URI (individual sprite images). */
  sprites: Record<string, string>;
  /** Full source images by sheet name (for composite rendering). */
  sheets: Record<string, HTMLImageElement>;
  /** 24 visualisation colours from viscolor.txt. */
  colors: string[];
  /** Playlist style from pledit.txt. */
  playlistStyle: PlaylistStyle;
  /** Whether nums_ex.bmp is present (use extended digits). */
  hasNumsEx: boolean;
}

interface SkinContents {
  images: Record<string, string>;
  texts: Record<string, string>;
}

// -- Default values --

const DEFAULT_COLORS: string[] = [
  "rgb(0,0,0)",
  "rgb(24,33,41)",
  "rgb(239,49,16)",
  "rgb(206,41,16)",
  "rgb(214,90,0)",
  "rgb(214,102,0)",
  "rgb(214,115,0)",
  "rgb(198,123,0)",
  "rgb(181,131,0)",
  "rgb(165,139,0)",
  "rgb(148,156,0)",
  "rgb(57,181,16)",
  "rgb(49,156,8)",
  "rgb(41,148,0)",
  "rgb(24,132,8)",
  "rgb(255,255,255)",
  "rgb(214,214,222)",
  "rgb(181,189,189)",
  "rgb(160,170,175)",
  "rgb(148,156,165)",
  "rgb(150,150,150)",
  "rgb(78,88,93)",
  "rgb(0,0,0)",
  "rgb(0,0,0)",
];

const DEFAULT_PLAYLIST_STYLE: PlaylistStyle = {
  normal: "#00FF00",
  current: "#FFFFFF",
  normalbg: "#000000",
  selectedbg: "#0000FF",
  font: "Arial",
};

// -- Parsing functions --

function parseViscolors(text: string): string[] {
  const colors = [...DEFAULT_COLORS];
  const regex = /^\s*(\d+)\s*,?\s*(\d+)\s*,?\s*(\d+)/;
  text
    .split("\n")
    .map((line) => regex.exec(line))
    .filter(Boolean)
    .forEach((match, i) => {
      if (match && i < 24) {
        colors[i] = `rgb(${match[1]},${match[2]},${match[3]})`;
      }
    });
  return colors;
}

function normalizeColor(color: string): string {
  const c = color.trim().replace(/^#/, "");
  if (c.length === 6) return `#${c}`;
  if (c.length === 3)
    return `#${c[0]}${c[0]}${c[1]}${c[1]}${c[2]}${c[2]}`;
  return `#${c}`;
}

function parsePledit(text: string): PlaylistStyle {
  const style = { ...DEFAULT_PLAYLIST_STYLE };
  const lines = text.split(/[\r\n]+/);
  // Some skins have [text] section, some don't. Accept both.
  let inTextSection = false;
  let hasAnySectionHeader = false;

  // Pre-scan to see if the file uses section headers at all.
  for (const line of lines) {
    if (line.trim().match(/^\[.+\]$/)) {
      hasAnySectionHeader = true;
      break;
    }
  }

  // If no section headers, treat entire file as the text section.
  if (!hasAnySectionHeader) {
    inTextSection = true;
  }

  for (const line of lines) {
    const trimmed = line.trim().toLowerCase();
    if (trimmed === "[text]") {
      inTextSection = true;
      continue;
    }
    if (trimmed.startsWith("[")) {
      inTextSection = false;
      continue;
    }
    if (!inTextSection) continue;

    const match = line.match(/^\s*([^;=]+?)\s*=\s*(.+?)\s*$/);
    if (!match) continue;

    const key = match[1].trim().toLowerCase();
    const value = match[2].trim();

    switch (key) {
      case "normal":
        style.normal = normalizeColor(value);
        break;
      case "current":
        style.current = normalizeColor(value);
        break;
      case "normalbg":
        style.normalbg = normalizeColor(value);
        break;
      case "selectedbg":
        style.selectedbg = normalizeColor(value);
        break;
      case "font":
        style.font = value.replace(/^["']|["']$/g, "");
        break;
    }
  }
  return style;
}

/** Load an image from a data URI. */
function loadImage(dataUri: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = (e) => reject(e);
    img.src = dataUri;
  });
}

/**
 * Apply Winamp-style color key transparency.
 *
 * Winamp skins use BMP files which don't support alpha channels, so skin
 * authors mark transparent pixels with a "magic" mask colour. Two mechanisms
 * are combined:
 *
 *  1. Magenta (#FF00FF) is ALWAYS stripped — it is the de-facto universal
 *     Winamp mask colour, chosen specifically because no skin artwork uses
 *     that exact value.
 *
 *  2. The pixel at (0,0) of each bitmap is checked as a per-image mask key
 *     (the standard Winamp convention). We accept it when every RGB channel
 *     is 0 or 255 (a saturated primary/secondary, excluding black/white)
 *     AND the colour covers less than 20% of the image — ruling out skins
 *     that legitimately use that colour as artwork.
 */
async function applyColorKeyTransparency(
  img: HTMLImageElement,
): Promise<HTMLImageElement> {
  const canvas = document.createElement("canvas");
  canvas.width = img.width;
  canvas.height = img.height;
  const ctx = canvas.getContext("2d")!;
  ctx.drawImage(img, 0, 0);

  const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
  const data = imageData.data;
  const totalPixels = canvas.width * canvas.height;

  // Detect per-bitmap mask colour from (0,0) pixel.
  // Must be a saturated primary/secondary (each channel 0 or 255, not
  // black, not white), non-magenta (magenta is always stripped below),
  // and cover less than 20% of the image (ruling out artwork colours).
  const mr = data[0], mg = data[1], mb = data[2];
  const bitmapKeyIsMagenta = mr === 255 && mg === 0 && mb === 255;
  let hasBitmapKey = false;
  if (
    !bitmapKeyIsMagenta &&
    (mr === 0 || mr === 255) &&
    (mg === 0 || mg === 255) &&
    (mb === 0 || mb === 255) &&
    (mr | mg | mb) !== 0 &&
    (mr & mg & mb) !== 255
  ) {
    let count = 0;
    for (let i = 0; i < data.length; i += 4) {
      if (data[i] === mr && data[i + 1] === mg && data[i + 2] === mb) {
        count++;
      }
    }
    hasBitmapKey = count <= totalPixels * 0.2;
  }

  // Replace mask pixels with full transparency.
  // Magenta is always stripped regardless of (0,0) or coverage.
  let modified = false;
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i], g = data[i + 1], b = data[i + 2];
    if (
      (r === 255 && g === 0 && b === 255) ||
      (hasBitmapKey && r === mr && g === mg && b === mb)
    ) {
      data[i + 3] = 0;
      modified = true;
    }
  }

  if (!modified) return img;

  ctx.putImageData(imageData, 0, 0);
  return loadImage(canvas.toDataURL());
}

/** Extract individual sprite images from a sprite sheet using canvas. */
function extractSprites(
  img: HTMLImageElement,
  sprites: Sprite[],
): Record<string, string> {
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d")!;
  const result: Record<string, string> = {};

  for (const sprite of sprites) {
    canvas.width = sprite.width;
    canvas.height = sprite.height;
    ctx.clearRect(0, 0, sprite.width, sprite.height);
    ctx.drawImage(img, -sprite.x, -sprite.y);
    result[sprite.name] = canvas.toDataURL();
  }

  return result;
}

// -- Main parser --

/**
 * Load and parse a .wsz skin file.
 *
 * Calls the Rust backend to unzip the file, then parses the contents
 * in the frontend using browser-native BMP loading and canvas sprite
 * extraction.
 */
export async function loadSkin(path: string): Promise<SkinData> {
  // Get raw skin contents from Rust.
  const contents = await invoke<SkinContents>("load_skin", { path });

  // Load all sprite sheet images.
  const sheets: Record<string, HTMLImageElement> = {};
  const allSprites: Record<string, string> = {};

  for (const [key, dataUri] of Object.entries(contents.images)) {
    try {
      const raw = await loadImage(dataUri);
      const img = await applyColorKeyTransparency(raw);
      sheets[key] = img;

      // Extract individual sprites if we have definitions for this sheet.
      const spriteDefs = SPRITE_SHEETS[key];
      if (spriteDefs) {
        const sprites = extractSprites(img, spriteDefs);
        Object.assign(allSprites, sprites);
      }
    } catch (e) {
      console.warn(`Failed to load skin image: ${key}`, e);
    }
  }

  // Apply fallbacks for missing sprite sheets.
  // Balance falls back to Volume (many skins don't include balance.bmp).
  if (!sheets["balance"] && sheets["volume"]) {
    sheets["balance"] = sheets["volume"];
  }

  // Check if nums_ex is available (overrides numbers for time display).
  const hasNumsEx = "nums_ex" in sheets;

  // Parse text files.
  const colors = contents.texts["viscolor"]
    ? parseViscolors(contents.texts["viscolor"])
    : DEFAULT_COLORS;

  const playlistStyle = contents.texts["pledit"]
    ? parsePledit(contents.texts["pledit"])
    : DEFAULT_PLAYLIST_STYLE;

  return {
    sprites: allSprites,
    sheets,
    colors,
    playlistStyle,
    hasNumsEx,
  };
}
