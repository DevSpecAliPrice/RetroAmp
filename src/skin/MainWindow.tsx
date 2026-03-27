/**
 * Skinned main window — renders the Winamp main player using sprites from
 * the loaded skin. All elements are positioned at their exact Winamp pixel
 * coordinates and rendered via canvas or absolutely-positioned images.
 *
 * Winamp main window layout (275x116):
 *   y=0:    Title bar (275x14)
 *   y=14:   Clutterbar area
 *   y=24:   Info area (bitrate, khz, mono/stereo)
 *   y=26:   Time display (digits from numbers.bmp)
 *   y=43:   Visualiser area
 *   y=72:   Position bar (seek bar)
 *   y=88:   Volume + Balance + Transport buttons
 *   y=88:   Control buttons row (prev, play, pause, stop, next, eject)
 *   y=104:  Shuffle/repeat/EQ/PL buttons
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { SkinData } from "./parser";
import { showContextMenu, type NativeMenuEntry } from "../nativeMenu";
import {
  CHAR_WIDTH,
  CHAR_HEIGHT,
  MARQUEE_X,
  MARQUEE_Y,
  MARQUEE_WIDTH,
  MARQUEE_CHARS,
  getCharCoords,
} from "./charmap";

interface EngineStatus {
  state: "Stopped" | "Playing" | "Paused";
  position: number | null;
  duration: number | null;
  metadata: {
    title: string | null;
    artist: string | null;
    sample_rate: number;
    channels: number;
    bitrate: number | null;
  } | null;
  volume: number;
  balance: number;
  can_seek: boolean;
  has_duration: boolean;
  is_stream: boolean;
}

interface PlaylistState {
  shuffle: "Off" | "All";
  repeat: "Off" | "Track" | "Playlist";
  track_count: number;
}

interface Props {
  skin: SkinData;
  scale: number;
  isShade?: boolean;
  onSkinChange?: (path: string) => void;
}

// Winamp main window is exactly 275x116, shade mode is 275x14.
const W = 275;
const H = 116;
const SHADE_H = 14;

/**
 * Positions of interactive elements in the main window.
 * These are the pixel coordinates where each control lives.
 */
const REGIONS = {
  // Title bar drag region
  titleBar: { x: 0, y: 0, w: 254, h: 14 },

  // Title bar buttons
  minimize: { x: 244, y: 3, w: 9, h: 9 },
  shade: { x: 254, y: 3, w: 9, h: 9 },
  close: { x: 264, y: 3, w: 9, h: 9 },

  // Time display region (for toggle elapsed/remaining)
  timeDisplay: { x: 36, y: 26, w: 63, h: 13 },

  // Transport buttons (y=88)
  previous: { x: 16, y: 88, w: 23, h: 18 },
  play: { x: 39, y: 88, w: 23, h: 18 },
  pause: { x: 62, y: 88, w: 23, h: 18 },
  stop: { x: 85, y: 88, w: 23, h: 18 },
  next: { x: 108, y: 88, w: 22, h: 18 },
  eject: { x: 136, y: 89, w: 22, h: 16 },

  // Position/seek bar
  posbar: { x: 16, y: 72, w: 248, h: 10 },

  // Volume slider
  volume: { x: 107, y: 57, w: 68, h: 14 },

  // Balance slider
  balance: { x: 177, y: 57, w: 38, h: 14 },

  // Shuffle/repeat
  shuffle: { x: 164, y: 89, w: 47, h: 15 },
  repeat: { x: 211, y: 89, w: 28, h: 15 },

  // EQ/PL toggle buttons
  eq: { x: 219, y: 58, w: 23, h: 12 },
  pl: { x: 242, y: 58, w: 23, h: 12 },
} as const;

/** Click regions for shade mode (275x14). */
const SHADE_REGIONS = {
  titleBar: { x: 0, y: 0, w: 244, h: 14 },
  minimize: { x: 244, y: 3, w: 9, h: 9 },
  unshade: { x: 254, y: 3, w: 9, h: 9 },
  close: { x: 264, y: 3, w: 9, h: 9 },
  posbar: { x: 226, y: 4, w: 17, h: 7 },
  timeDisplay: { x: 127, y: 4, w: 26, h: 6 },
} as const;

/** Shade mode text area for scrolling track title. */
const SHADE_TEXT_X = 24;
const SHADE_TEXT_Y = 4;
const SHADE_TEXT_W = 100;
interface RecentSkin {
  name: string;
  path: string;
}

export default function MainWindow({ skin, isShade = false, onSkinChange }: Props) {
  // Derive scale from window width. The canvas is rendered at this higher
  // resolution (W*s × H*s pixels) then CSS-stretched to fill the window,
  // giving crisp integer-scaled pixels with no gaps.
  const s = Math.max(1, Math.round(window.innerWidth / W));
  console.log(`[MainWindow] window.innerWidth=${window.innerWidth} window.innerHeight=${window.innerHeight} → scale=${s} → skin=${W*s}x${(isShade?14:116)*s}`);

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [status, setStatus] = useState<EngineStatus>({
    state: "Stopped",
    position: null,
    duration: null,
    metadata: null,
    volume: 1.0,
    balance: 0.0,
    can_seek: false,
    has_duration: false,
    is_stream: false,
  });
  const [windowStates, setWindowStates] = useState<Record<string, { visible: boolean }>>({});
  const [playlist, setPlaylist] = useState<PlaylistState>({
    shuffle: "Off",
    repeat: "Off",
    track_count: 0,
  });
  const [pressed, setPressed] = useState<string | null>(null);
  const [marqueeOffset, setMarqueeOffset] = useState(0);
  const [fftData, setFftData] = useState<number[]>([]);
  const [showRemaining, setShowRemaining] = useState(false);
  const [tooltip, setTooltip] = useState<{ text: string; x: number; y: number } | null>(null);
  const tooltipTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const tooltipText = useRef("");
  const dragging = useRef<"volume" | "balance" | "posbar" | null>(null);

  // Shorthand for playlist style (used by context menu theming).


  // Build the marquee text from current metadata.
  const meta = status.metadata;
  const marqueeText = (() => {
    const artist = meta?.artist ?? "";
    const title = meta?.title ?? "";
    if (artist && title) return `${artist} - ${title}`;
    if (title) return title;
    if (status.state === "Stopped") return "RetroAmp";
    return "";
  })();

  // Marquee scroll animation — 5px (one character) every 220ms.
  // Only scrolls if the text is longer than the visible area.
  const needsScroll = marqueeText.length > MARQUEE_CHARS;
  const scrollText = needsScroll
    ? marqueeText + "  ***  " + marqueeText
    : marqueeText;

  useEffect(() => {
    if (!needsScroll) {
      setMarqueeOffset(0);
      return;
    }
    const interval = setInterval(() => {
      setMarqueeOffset((prev) => {
        const totalWidth = (marqueeText.length + 7) * CHAR_WIDTH; // text + separator
        const next = prev + CHAR_WIDTH;
        return next >= totalWidth ? 0 : next;
      });
    }, 220);
    return () => clearInterval(interval);
  }, [needsScroll, marqueeText]);

  // Reset scroll when track changes.
  useEffect(() => {
    setMarqueeOffset(0);
  }, [marqueeText]);

  // Poll engine and playlist state.
  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const [s, pl, ws] = await Promise.all([
          invoke<EngineStatus>("get_status"),
          invoke<PlaylistState>("get_playlist"),
          invoke<{ windows: Record<string, { visible: boolean }> }>("get_window_states"),
        ]);
        setStatus(s);
        setPlaylist(pl);
        setWindowStates(ws.windows);
        if (s.state === "Playing") {
          const fft = await invoke<{ magnitudes: number[] }>("get_fft_data");
          setFftData(fft.magnitudes);
        } else {
          setFftData([]);
        }
      } catch (e) {
        console.error(e);
      }
    }, 50);
    return () => clearInterval(interval);
  }, []);

  // Current canvas height depends on shade mode.
  const canvasH = isShade ? SHADE_H : H;

  // Render the main window.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const titlebar = skin.sheets["titlebar"];
    const textBmp = skin.sheets["text"];

    // Scale all drawing to the higher-resolution canvas. All coordinates
    // below remain in native 275x116 space — ctx.scale handles the rest.
    ctx.setTransform(s, 0, 0, s, 0, 0);

    // Clear.
    ctx.clearRect(0, 0, W, canvasH);

    // ── SHADE MODE ──
    if (isShade) {
      // 1) Shade background from titlebar.bmp — active/focused at (27, 29).
      if (titlebar) {
        ctx.drawImage(titlebar, 27, 29, 275, 14, 0, 0, 275, 14);
      }

      // 2) Scrolling track title using text.bmp.
      if (textBmp && scrollText.length > 0) {
        ctx.save();
        ctx.beginPath();
        ctx.rect(SHADE_TEXT_X, SHADE_TEXT_Y, SHADE_TEXT_W, CHAR_HEIGHT);
        ctx.clip();
        for (let i = 0; i < scrollText.length; i++) {
          const destX = SHADE_TEXT_X + i * CHAR_WIDTH - marqueeOffset;
          if (destX + CHAR_WIDTH < SHADE_TEXT_X || destX > SHADE_TEXT_X + SHADE_TEXT_W) continue;
          const { x: sx, y: sy } = getCharCoords(scrollText[i]);
          ctx.drawImage(textBmp, sx, sy, CHAR_WIDTH, CHAR_HEIGHT, destX, SHADE_TEXT_Y, CHAR_WIDTH, CHAR_HEIGHT);
        }
        ctx.restore();
      }

      // 3) Mini time display using text.bmp at (127, 4).
      if (textBmp && status.position !== null) {
        let displaySecs: number;
        let prefix = "";
        if (showRemaining && status.duration && status.duration > 0) {
          displaySecs = Math.floor(status.duration - status.position);
          if (displaySecs < 0) displaySecs = 0;
          prefix = "-";
        } else {
          displaySecs = Math.floor(status.position);
        }
        const m = Math.floor(displaySecs / 60);
        const s = displaySecs % 60;
        const timeStr = `${prefix}${m}:${s.toString().padStart(2, "0")}`;
        for (let i = 0; i < timeStr.length; i++) {
          const { x: sx, y: sy } = getCharCoords(timeStr[i]);
          ctx.drawImage(textBmp, sx, sy, CHAR_WIDTH, CHAR_HEIGHT, 127 + i * CHAR_WIDTH, 4, CHAR_WIDTH, CHAR_HEIGHT);
        }
      }

      // 4) Mini position bar from titlebar.bmp.
      if (titlebar) {
        // Background: (0, 36, 17, 7) → dest (226, 4).
        ctx.drawImage(titlebar, 0, 36, 17, 7, 226, 4, 17, 7);
        // Thumb: 3px wide.
        if (status.position !== null && status.duration && status.duration > 0) {
          const fraction = status.position / status.duration;
          const thumbX = Math.round(fraction * (17 - 3));
          ctx.drawImage(titlebar, 20, 36, 3, 7, 226 + thumbX, 4, 3, 7);
        }
      }

      // 5) Title bar buttons — minimize, unshade, close.
      // These are part of the shade background; draw pressed states on top.
      if (titlebar) {
        if (pressed === "minimize") ctx.drawImage(titlebar, 9, 9, 9, 9, 244, 3, 9, 9);
        if (pressed === "unshade") ctx.drawImage(titlebar, 9, 27, 9, 9, 254, 3, 9, 9);
        if (pressed === "close") ctx.drawImage(titlebar, 18, 9, 9, 9, 264, 3, 9, 9);
      }

      // Done — skip normal rendering.
    } else {

    // ── NORMAL MODE ──

    // 1) Draw the main background.
    const bg = skin.sheets["main"];
    if (bg) {
      ctx.drawImage(bg, 0, 0, W, H, 0, 0, W, H);
    }

    // 2) Draw the active title bar on top.
    if (titlebar) {
      // Selected title bar: x=27, y=0, 275x14 in titlebar.bmp
      ctx.drawImage(titlebar, 27, 0, 275, 14, 0, 0, 275, 14);
    }

    // 2.5) Draw marquee text from text.bmp.
    if (textBmp && scrollText.length > 0) {
      // Save context and clip to the marquee area.
      ctx.save();
      ctx.beginPath();
      ctx.rect(MARQUEE_X, MARQUEE_Y, MARQUEE_WIDTH, CHAR_HEIGHT);
      ctx.clip();

      // Draw each character, offset by the scroll position.
      for (let i = 0; i < scrollText.length; i++) {
        const destX = MARQUEE_X + i * CHAR_WIDTH - marqueeOffset;
        // Skip characters that are fully outside the visible area.
        if (destX + CHAR_WIDTH < MARQUEE_X || destX > MARQUEE_X + MARQUEE_WIDTH) {
          continue;
        }
        const { x: srcX, y: srcY } = getCharCoords(scrollText[i]);
        ctx.drawImage(textBmp, srcX, srcY, CHAR_WIDTH, CHAR_HEIGHT, destX, MARQUEE_Y, CHAR_WIDTH, CHAR_HEIGHT);
      }

      ctx.restore();
    }

    // 2.7) Draw spectrum analyser in the vis area.
    // Position: x=24, y=43, 75px wide, 16px tall.
    // Uses viscolor.txt colours: 0 = background, 2-17 = bar gradient (bottom to top), 23 = peaks.
    const VIS_X = 24;
    const VIS_Y = 43;
    const VIS_W = 75;
    const VIS_H = 16;
    const NUM_BARS = 19; // Classic Winamp "wide" bar mode: 19 bars, each 3px wide + 1px gap
    const BAR_W = 3;
    const BAR_GAP = 1;

    // Fill vis background.
    ctx.fillStyle = skin.colors[0] ?? "rgb(0,0,0)";
    ctx.fillRect(VIS_X, VIS_Y, VIS_W, VIS_H);

    if (fftData.length > 0) {
      for (let i = 0; i < NUM_BARS; i++) {
        // Map bar index to FFT bin — use lower bins where music content lives.
        // Use logarithmic mapping for more musically useful distribution.
        const binIndex = Math.floor(Math.pow(i / NUM_BARS, 1.5) * 80) + 2;
        const magnitude = fftData[binIndex] ?? 0;

        // Convert magnitude to bar height (0 to VIS_H pixels).
        const barHeight = Math.min(Math.round(magnitude * VIS_H * 5), VIS_H);

        const barX = VIS_X + i * (BAR_W + BAR_GAP);

        // Draw bar from bottom up, one pixel row at a time with gradient colours.
        // Colours 2-17 map to the 16 pixel rows (bottom = green/index 17, top = red/index 2).
        for (let row = 0; row < barHeight; row++) {
          const colorIndex = 17 - Math.floor((row / VIS_H) * 15);
          ctx.fillStyle = skin.colors[colorIndex] ?? "rgb(0,255,0)";
          ctx.fillRect(barX, VIS_Y + VIS_H - 1 - row, BAR_W, 1);
        }
      }
    }

    // 3) Draw play/pause/stop indicator.
    const playpaus = skin.sheets["playpaus"];
    if (playpaus) {
      let sx = 18; // stopped
      if (status.state === "Playing") sx = 0;
      else if (status.state === "Paused") sx = 9;
      ctx.drawImage(playpaus, sx, 0, 9, 9, 24, 28, 9, 9);
    }

    // 4) Draw time display digits.
    // Prefer nums_ex.bmp if available (many skins only include this).
    const numbers = skin.sheets["nums_ex"] ?? skin.sheets["numbers"];
    if (numbers && status.position !== null) {
      let displaySecs: number;
      let isNegative = false;

      if (showRemaining && status.duration && status.duration > 0) {
        displaySecs = Math.floor(status.duration - status.position);
        if (displaySecs < 0) displaySecs = 0;
        isNegative = true;
      } else {
        displaySecs = Math.floor(status.position);
      }

      const mins = Math.floor(displaySecs / 60);
      const secs = displaySecs % 60;
      const digits = [
        Math.floor(mins / 10),
        mins % 10,
        Math.floor(secs / 10),
        secs % 10,
      ];
      // Time display starts at x=48, y=26 — two digits, colon gap, two digits.
      const positions = [36, 48, 60, 78, 90]; // min10, min1, (colon), sec10, sec1

      // Draw minus sign for remaining time (index 10 in the numbers strip, at srcX=90).
      if (isNegative) {
        ctx.drawImage(numbers, 90, 0, 9, 13, 27, 26, 9, 13);
      }
      // First digit (tens of minutes) — only draw if > 0 or showing remaining
      if (digits[0] > 0 || isNegative) {
        drawDigit(ctx, numbers, digits[0], positions[0], 26);
      }
      drawDigit(ctx, numbers, digits[1], positions[1], 26);
      drawDigit(ctx, numbers, digits[2], positions[3], 26);
      drawDigit(ctx, numbers, digits[3], positions[4], 26);
    }

    // 5) Draw mono/stereo indicators.
    // Both are always drawn: the active mode uses the lit sprite (top row, y=0)
    // and the inactive mode uses the dim sprite (bottom row, y=rowH).
    // In MONOSTER.BMP, STEREO is the left 29px and MONO is the right 27px.
    // In the window, they display in REVERSE order: MONO on the left at x=212,
    // STEREO on the right at x=239. They are perfectly adjacent (212+27=239).
    // BMP widths vary (56, 58, 54, etc.): mono source starts at max(29, w-27)
    // to skip any gap in wider BMPs without overlapping the stereo region.
    const monoster = skin.sheets["monoster"];
    if (monoster && status.metadata) {
      const isStereo = (status.metadata.channels ?? 0) >= 2;
      const rowH = Math.floor(monoster.height / 2);
      if (rowH > 0) {
        const stereoW = Math.min(29, monoster.width);
        const monoSrcX = Math.max(29, monoster.width - 27);
        const monoW = Math.max(0, monoster.width - monoSrcX);
        // Mono at (212, 41) LEFT: dim if stereo, lit if mono
        if (monoW > 0) {
          ctx.drawImage(monoster, monoSrcX, isStereo ? rowH : 0, monoW, rowH, 212, 41, monoW, rowH);
        }
        // Stereo at (239, 41) RIGHT: lit if stereo, dim if mono
        ctx.drawImage(monoster, 0, isStereo ? 0 : rowH, stereoW, rowH, 239, 41, stereoW, rowH);
      }
    }

    // 5b) Draw bitrate (kbps) and sample rate (kHz) using text.bmp.
    // Classic Winamp positions: kbps at x=111, y=43; kHz at x=156, y=43.
    if (textBmp && status.metadata) {
      const bitrate = status.metadata.bitrate;
      if (bitrate !== null && bitrate !== undefined) {
        const kbpsStr = String(Math.min(bitrate, 999)).padStart(3, " ");
        for (let i = 0; i < kbpsStr.length; i++) {
          const { x: sx, y: sy } = getCharCoords(kbpsStr[i]);
          ctx.drawImage(textBmp, sx, sy, CHAR_WIDTH, CHAR_HEIGHT, 111 + i * CHAR_WIDTH, 43, CHAR_WIDTH, CHAR_HEIGHT);
        }
      }

      const khz = Math.floor(status.metadata.sample_rate / 1000);
      const khzStr = String(khz).padStart(2, " ");
      for (let i = 0; i < khzStr.length; i++) {
        const { x: sx, y: sy } = getCharCoords(khzStr[i]);
        ctx.drawImage(textBmp, sx, sy, CHAR_WIDTH, CHAR_HEIGHT, 156 + i * CHAR_WIDTH, 43, CHAR_WIDTH, CHAR_HEIGHT);
      }
    }

    // 6) Draw position bar.
    const posbar = skin.sheets["posbar"];
    if (posbar) {
      ctx.drawImage(posbar, 0, 0, 248, 10, 16, 72, 248, 10);
      // Draw thumb.
      if (status.position !== null && status.duration && status.duration > 0) {
        const fraction = status.position / status.duration;
        const thumbX = Math.floor(fraction * (248 - 29));
        const isPressed = pressed === "posbar";
        const thumbSrcX = isPressed ? 278 : 248;
        ctx.drawImage(posbar, thumbSrcX, 0, 29, 10, 16 + thumbX, 72, 29, 10);
      }
    }

    // 7) Draw transport buttons.
    const cbuttons = skin.sheets["cbuttons"];
    if (cbuttons) {
      const drawBtn = (name: string, sx: number, sy: number, sw: number, sh: number, dx: number, dy: number) => {
        const srcY = pressed === name ? sy + sh : sy;
        ctx.drawImage(cbuttons, sx, srcY, sw, sh, dx, dy, sw, sh);
      };
      drawBtn("previous", 0, 0, 23, 18, 16, 88);
      drawBtn("play", 23, 0, 23, 18, 39, 88);
      drawBtn("pause", 46, 0, 23, 18, 62, 88);
      drawBtn("stop", 69, 0, 23, 18, 85, 88);
      drawBtn("next", 92, 0, 22, 18, 108, 88);
      drawBtn("eject", 114, 0, 22, 16, 136, 89);
    }

    // 8) Draw shuffle/repeat buttons.
    const shufrep = skin.sheets["shufrep"];
    if (shufrep) {
      // Shuffle: active row is y=30, inactive y=0, pressed adds 15
      const shufActive = playlist.shuffle !== "Off";
      const shufBaseY = shufActive ? 30 : 0;
      const shufY = pressed === "shuffle" ? shufBaseY + 15 : shufBaseY;
      ctx.drawImage(shufrep, 28, shufY, 47, 15, 164, 89, 47, 15);

      // Repeat: same pattern
      const repActive = playlist.repeat !== "Off";
      const repBaseY = repActive ? 30 : 0;
      const repY = pressed === "repeat" ? repBaseY + 15 : repBaseY;
      ctx.drawImage(shufrep, 0, repY, 28, 15, 211, 89, 28, 15);

      // EQ button — active when EQ window is visible
      const eqActive = windowStates["equalizer"]?.visible ?? false;
      const eqBaseY = eqActive ? 73 : 61;
      const eqY = pressed === "eq" ? (eqActive ? 61 : 73) : eqBaseY;
      ctx.drawImage(shufrep, 0, eqY, 23, 12, 219, 58, 23, 12);

      // PL button — active when playlist window is visible
      const plActive = windowStates["playlist"]?.visible ?? false;
      const plBaseY = plActive ? 73 : 61;
      const plY = pressed === "pl" ? (plActive ? 61 : 73) : plBaseY;
      ctx.drawImage(shufrep, 23, plY, 23, 12, 242, 58, 23, 12);
    }

    // 9) Draw volume slider.
    // Volume BMPs vary in height: 433 (standard with thumbs), 418-422 (frames
    // only, no thumb area), etc. Only draw if the source region fits.
    const volumeSheet = skin.sheets["volume"];
    if (volumeSheet) {
      const frame = Math.round(status.volume * 27);
      const srcY = frame * 15;
      if (srcY + 14 <= volumeSheet.height) {
        ctx.drawImage(volumeSheet, 0, srcY, 68, 14, 107, 57, 68, 14);
      }
      // Draw thumb (standard position y=422, needs 11px).
      const thumbX = Math.round(status.volume * (68 - 14));
      const volThumbSrcX = pressed === "volume" ? 0 : 15;
      if (422 + 11 <= volumeSheet.height) {
        ctx.drawImage(volumeSheet, volThumbSrcX, 422, 14, 11, 107 + thumbX, 58, 14, 11);
      }
    }

    // 10) Draw balance slider.
    // Balance BMPs vary widely: 68×433 (standard), 47×433 (common — visible
    // 38px starts at x=9), 47×13 or 38×13 (thumb-only, no usable frames).
    const balanceSheet = skin.sheets["balance"];
    if (balanceSheet) {
      const balFraction = (status.balance + 1) / 2;
      const balFrame = Math.round(balFraction * 27);
      const balSrcY = balFrame * 15;
      // Detect source X: standard 68px uses x=9, 47px also uses x=9 (47-9=38),
      // pre-cropped 38px uses x=0.
      const balSrcX = balanceSheet.width >= 47 ? 9 : Math.max(0, balanceSheet.width - 38);
      const balDrawW = Math.min(38, balanceSheet.width - balSrcX);
      if (balDrawW > 0 && balSrcY + 14 <= balanceSheet.height) {
        ctx.drawImage(balanceSheet, balSrcX, balSrcY, balDrawW, 14, 177, 57, balDrawW, 14);
      }
      // Draw thumb — try balance sheet first, fall back to volume sheet.
      const balThumbX = Math.round(balFraction * (38 - 14));
      const balThumbSrcX = pressed === "balance" ? 0 : 15;
      if (422 + 11 <= balanceSheet.height) {
        ctx.drawImage(balanceSheet, balThumbSrcX, 422, 14, 11, 177 + balThumbX, 58, 14, 11);
      } else if (volumeSheet && 422 + 11 <= volumeSheet.height) {
        ctx.drawImage(volumeSheet, balThumbSrcX, 422, 14, 11, 177 + balThumbX, 58, 14, 11);
      }
    }

    } // end normal mode
  }, [skin, status, playlist, pressed, marqueeOffset, scrollText, fftData, windowStates, showRemaining, isShade, canvasH, s]);

  // Slider drag helper — converts pixel x to the appropriate invoke call.
  const applySlider = useCallback(
    (type: "volume" | "balance" | "posbar", x: number) => {
      if (type === "volume") {
        const fraction = Math.max(0, Math.min(1, (x - REGIONS.volume.x) / REGIONS.volume.w));
        invoke("set_volume", { volume: fraction });
      } else if (type === "balance") {
        // Balance: 0.0 at left edge → 1.0 at right edge, map to -1.0..1.0
        const fraction = Math.max(0, Math.min(1, (x - REGIONS.balance.x) / REGIONS.balance.w));
        invoke("set_balance", { balance: fraction * 2 - 1 });
      } else if (type === "posbar") {
        if (status.duration && status.duration > 0) {
          const fraction = Math.max(0, Math.min(1, (x - REGIONS.posbar.x) / REGIONS.posbar.w));
          invoke("seek", { positionSecs: fraction * status.duration });
        }
      }
    },
    [status.duration],
  );

  // Global drag listeners for smooth slider dragging.
  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const x = Math.round((e.clientX - rect.left) * (W / rect.width));
      applySlider(dragging.current, x);
    };
    const onMouseUp = () => {
      if (dragging.current) {
        dragging.current = null;
        setPressed(null);
      }
    };
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [applySlider]);

  // Click handling.
  const handleMouseDown = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const x = Math.round((e.clientX - rect.left) * (W / rect.width));
      const y = Math.round((e.clientY - rect.top) * (canvasH / rect.height));

      const hit = (r: { x: number; y: number; w: number; h: number }) =>
        x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;

      // ── SHADE MODE CLICKS ──
      if (isShade) {
        if (hit(SHADE_REGIONS.titleBar) && !hit(SHADE_REGIONS.minimize) && !hit(SHADE_REGIONS.unshade) && !hit(SHADE_REGIONS.close)) {
          getCurrentWindow().startDragging();
          return;
        }
        if (hit(SHADE_REGIONS.minimize)) {
          setPressed("minimize");
          getCurrentWindow().minimize();
          return;
        }
        if (hit(SHADE_REGIONS.unshade)) {
          setPressed("unshade");
          invoke("exit_shade");
          return;
        }
        if (hit(SHADE_REGIONS.close)) {
          getCurrentWindow().close();
          return;
        }
        if (hit(SHADE_REGIONS.timeDisplay)) {
          setShowRemaining((prev) => !prev);
          return;
        }
        if (hit(SHADE_REGIONS.posbar)) {
          if (status.duration && status.duration > 0) {
            const fraction = Math.max(0, Math.min(1, (x - SHADE_REGIONS.posbar.x) / SHADE_REGIONS.posbar.w));
            invoke("seek", { positionSecs: fraction * status.duration });
          }
          return;
        }
        return;
      }

      // ── NORMAL MODE CLICKS ──

      // Title bar drag — must be called synchronously from mousedown
      // for Wayland to accept the drag initiation.
      if (hit(REGIONS.titleBar) && !hit(REGIONS.minimize) && !hit(REGIONS.shade) && !hit(REGIONS.close)) {
        getCurrentWindow().startDragging();
        return;
      }

      // Minimize button
      if (hit(REGIONS.minimize)) {
        setPressed("minimize");
        getCurrentWindow().minimize();
        return;
      }

      // Shade button
      if (hit(REGIONS.shade)) {
        setPressed("shade");
        invoke("enter_shade");
        return;
      }

      // Close button
      if (hit(REGIONS.close)) {
        getCurrentWindow().close();
        return;
      }

      // Time display — toggle elapsed/remaining.
      if (hit(REGIONS.timeDisplay)) {
        setShowRemaining((prev) => !prev);
        return;
      }

      if (hit(REGIONS.previous)) {
        setPressed("previous");
        invoke("previous_track");
      } else if (hit(REGIONS.play)) {
        setPressed("play");
        if (status.state === "Paused") invoke("resume");
        else if (status.state === "Stopped" && playlist.track_count > 0)
          invoke("playlist_play_index", { index: 0 });
      } else if (hit(REGIONS.pause)) {
        setPressed("pause");
        if (status.state === "Playing") invoke("pause");
        else invoke("resume");
      } else if (hit(REGIONS.stop)) {
        setPressed("stop");
        invoke("stop");
      } else if (hit(REGIONS.next)) {
        setPressed("next");
        invoke("next_track");
      } else if (hit(REGIONS.shuffle)) {
        setPressed("shuffle");
        invoke("toggle_shuffle");
      } else if (hit(REGIONS.repeat)) {
        setPressed("repeat");
        invoke("cycle_repeat");
      } else if (hit(REGIONS.posbar)) {
        setPressed("posbar");
        dragging.current = "posbar";
        applySlider("posbar", x);
      } else if (hit(REGIONS.volume)) {
        setPressed("volume");
        dragging.current = "volume";
        applySlider("volume", x);
      } else if (hit(REGIONS.balance)) {
        setPressed("balance");
        dragging.current = "balance";
        applySlider("balance", x);
      } else if (hit(REGIONS.eject)) {
        setPressed("eject");
        import("@tauri-apps/plugin-dialog").then(({ open: openDialog }) => {
          openDialog({
            multiple: true,
            filters: [{ name: "Audio", extensions: ["mp3", "flac", "ogg", "wav", "aac", "m4a"] }],
          }).then((selected) => {
            if (selected) {
              const paths = Array.isArray(selected) ? selected : [selected];
              invoke("playlist_add_files", { paths });
            }
          });
        });
      } else if (hit(REGIONS.pl)) {
        setPressed("pl");
        invoke("toggle_window", { windowId: "Playlist" });
      } else if (hit(REGIONS.eq)) {
        setPressed("eq");
        invoke("toggle_window", { windowId: "Equalizer" });
      }
    },
    [status, playlist, applySlider, isShade, canvasH],
  );

  const handleMouseUp = useCallback(() => {
    setPressed(null);
  }, []);

  // Tooltip — custom delayed tooltip on hover.
  const handleMouseMove = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const x = Math.round((e.clientX - rect.left) * (W / rect.width));
      const y = Math.round((e.clientY - rect.top) * (canvasH / rect.height));

      const hit = (r: { x: number; y: number; w: number; h: number }) =>
        x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;

      let tip = "";
      if (isShade) {
        if (hit(SHADE_REGIONS.minimize)) tip = "Minimize";
        else if (hit(SHADE_REGIONS.unshade)) tip = "Toggle Window Shade";
        else if (hit(SHADE_REGIONS.close)) tip = "Close";
        else if (hit(SHADE_REGIONS.posbar)) tip = "Seek";
        else if (hit(SHADE_REGIONS.timeDisplay)) tip = "Toggle Elapsed/Remaining Time";
      } else {
        if (hit(REGIONS.minimize)) tip = "Minimize";
        else if (hit(REGIONS.shade)) tip = "Toggle Window Shade";
        else if (hit(REGIONS.close)) tip = "Close";
        else if (hit(REGIONS.previous)) tip = "Previous Track";
        else if (hit(REGIONS.play)) tip = "Play";
        else if (hit(REGIONS.pause)) tip = "Pause";
        else if (hit(REGIONS.stop)) tip = "Stop";
        else if (hit(REGIONS.next)) tip = "Next Track";
        else if (hit(REGIONS.eject)) tip = "Open File(s)";
        else if (hit(REGIONS.shuffle)) tip = `Shuffle ${playlist.shuffle === "Off" ? "Off" : "On"}`;
        else if (hit(REGIONS.repeat)) tip = `Repeat ${playlist.repeat}`;
        else if (hit(REGIONS.eq)) tip = "Toggle Equalizer";
        else if (hit(REGIONS.pl)) tip = "Toggle Playlist";
        else if (hit(REGIONS.volume)) tip = `Volume: ${Math.round(status.volume * 100)}%`;
        else if (hit(REGIONS.balance)) {
          const bal = Math.round(status.balance * 100);
          tip = bal === 0 ? "Balance: Center" : bal < 0 ? `Balance: ${-bal}% Left` : `Balance: ${bal}% Right`;
        }
        else if (hit(REGIONS.posbar) && status.position !== null && status.duration) {
          const pos = formatTime(status.position);
          const dur = formatTime(status.duration);
          tip = `${pos} / ${dur}`;
        }
        else if (hit(REGIONS.timeDisplay)) tip = "Toggle Elapsed/Remaining Time";
      }

      // If the tip text changed, reset the delay timer.
      if (tip !== tooltipText.current) {
        tooltipText.current = tip;
        setTooltip(null);
        if (tooltipTimer.current) clearTimeout(tooltipTimer.current);
        if (tip) {
          const cx = e.clientX;
          const cy = e.clientY;
          tooltipTimer.current = setTimeout(() => {
            setTooltip({ text: tip, x: cx, y: cy });
          }, 800);
        }
      }
    },
    [status, playlist, isShade, canvasH],
  );

  const handleMouseLeave = useCallback(() => {
    setPressed(null);
    setTooltip(null);
    tooltipText.current = "";
    if (tooltipTimer.current) clearTimeout(tooltipTimer.current);
  }, []);

  // Double-click resets volume (100%) or balance (center).
  const handleDoubleClick = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      if (isShade) return;
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const x = Math.round((e.clientX - rect.left) * (W / rect.width));
      const y = Math.round((e.clientY - rect.top) * (canvasH / rect.height));
      const hit = (r: { x: number; y: number; w: number; h: number }) =>
        x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;

      if (hit(REGIONS.volume)) {
        invoke("set_volume", { volume: 1.0 });
      } else if (hit(REGIONS.balance)) {
        invoke("set_balance", { balance: 0.0 });
      }
    },
    [isShade, canvasH],
  );

  // Right-click context menu (native OS menu).
  const handleContextMenu = useCallback(async (e: React.MouseEvent) => {
    e.preventDefault();

    // Fetch recent skins for the submenu.
    let skinItems: NativeMenuEntry[] = [];
    try {
      const recent = await invoke<RecentSkin[]>("get_recent_skins", { limit: 5 });
      skinItems = recent.map((s) => ({
        type: "item" as const, id: `skin:${s.path}`, label: s.name,
      }));
    } catch { /* ignore */ }

    const items: NativeMenuEntry[] = [
      { type: "item", id: "toggle_playlist", label: "Toggle Playlist" },
      { type: "item", id: "toggle_equalizer", label: "Toggle Equalizer" },
      { type: "separator" },
      { type: "item", id: "add_files", label: "Add Files..." },
      { type: "item", id: "radio_browser", label: "Radio Browser..." },
      { type: "item", id: "media_library", label: "Media Library..." },
      { type: "separator" },
      {
        type: "submenu", label: "Skins", items: [
          ...skinItems,
          ...(skinItems.length > 0 ? [{ type: "separator" as const }] : []),
          { type: "item" as const, id: "skins_browse", label: "Browse All..." },
        ],
      },
      { type: "separator" },
      { type: "item", id: "preferences", label: "Preferences..." },
    ];

    const selected = await showContextMenu(items, e.clientX, e.clientY);
    if (!selected) return;

    if (selected === "toggle_playlist") invoke("toggle_window", { windowId: "Playlist" });
    else if (selected === "toggle_equalizer") invoke("toggle_window", { windowId: "Equalizer" });
    else if (selected === "add_files") {
      const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
      const sel = await openDialog({ multiple: true, filters: [{ name: "Audio", extensions: ["mp3", "flac", "ogg", "wav", "aac", "m4a", "m3u", "m3u8", "pls"] }] });
      if (sel) invoke("playlist_add_files", { paths: Array.isArray(sel) ? sel : [sel] });
    }
    else if (selected === "radio_browser") invoke("toggle_window", { windowId: "RadioBrowser" });
    else if (selected === "media_library") invoke("toggle_window", { windowId: "LibraryBrowser" });
    else if (selected === "skins_browse") invoke("open_settings");
    else if (selected === "preferences") invoke("open_settings");
    else if (selected.startsWith("skin:")) onSkinChange?.(selected.slice(5));
  }, [onSkinChange]);

  return (
    <div style={{
      width: W * s,
      height: canvasH * s,
      position: "relative",
      background: "#000",
    }}>
      <canvas
        ref={canvasRef}
        width={W * s}
        height={canvasH * s}
        style={{
          width: W * s,
          height: canvasH * s,
          imageRendering: "pixelated",
          cursor: "default",
          display: "block",
        }}
        onMouseDown={handleMouseDown}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseLeave}
        onMouseMove={handleMouseMove}
        onDoubleClick={handleDoubleClick}
        onContextMenu={handleContextMenu}
      />

      {/* Tooltip */}
      {tooltip && createPortal(
        <div
          style={{
            position: "fixed",
            left: tooltip.x + 12,
            top: tooltip.y + 16,
            background: "#ffffe1",
            border: "1px solid #000",
            padding: "2px 6px",
            fontFamily: "system-ui, sans-serif",
            fontSize: "11px",
            color: "#000",
            whiteSpace: "nowrap",
            pointerEvents: "none",
            zIndex: 2000,
          }}
        >
          {tooltip.text}
        </div>,
        document.body
      )}

    </div>
  );
}

/** Format seconds as M:SS or MM:SS. */
function formatTime(secs: number): string {
  const total = Math.floor(secs);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/** Draw a single digit from the numbers sprite sheet. */
function drawDigit(
  ctx: CanvasRenderingContext2D,
  numbersImg: HTMLImageElement,
  digit: number,
  x: number,
  y: number,
) {
  // Each digit is 9px wide, 13px tall, laid out horizontally in numbers.bmp.
  ctx.drawImage(numbersImg, digit * 9, 0, 9, 13, x, y, 9, 13);
}

