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
      const img = await loadImage(dataUri);
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
