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
import { invoke } from "@tauri-apps/api/core";
import type { SkinData } from "./parser";
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
  } | null;
  volume: number;
}

interface PlaylistState {
  shuffle: "Off" | "All";
  repeat: "Off" | "Track" | "Playlist";
  track_count: number;
}

interface Props {
  skin: SkinData;
  scale: number;
}

// Winamp main window is exactly 275x116.
const W = 275;
const H = 116;

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

  // Shuffle/repeat
  shuffle: { x: 164, y: 89, w: 47, h: 15 },
  repeat: { x: 211, y: 89, w: 28, h: 15 },

  // EQ/PL toggle buttons
  eq: { x: 219, y: 58, w: 23, h: 12 },
  pl: { x: 242, y: 58, w: 23, h: 12 },
} as const;

export default function MainWindow({ skin, scale }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [status, setStatus] = useState<EngineStatus>({
    state: "Stopped",
    position: null,
    duration: null,
    metadata: null,
    volume: 1.0,
  });
  const [playlist, setPlaylist] = useState<PlaylistState>({
    shuffle: "Off",
    repeat: "Off",
    track_count: 0,
  });
  const [pressed, setPressed] = useState<string | null>(null);
  const [marqueeOffset, setMarqueeOffset] = useState(0);
  const [fftData, setFftData] = useState<number[]>([]);

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
        const [s, pl] = await Promise.all([
          invoke<EngineStatus>("get_status"),
          invoke<PlaylistState>("get_playlist"),
        ]);
        setStatus(s);
        setPlaylist(pl);
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

  // Render the main window.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Clear.
    ctx.clearRect(0, 0, W, H);

    // 1) Draw the main background.
    const bg = skin.sheets["main"];
    if (bg) {
      ctx.drawImage(bg, 0, 0, W, H, 0, 0, W, H);
    }

    // 2) Draw the active title bar on top.
    const titlebar = skin.sheets["titlebar"];
    if (titlebar) {
      // Selected title bar: x=27, y=0, 275x14 in titlebar.bmp
      ctx.drawImage(titlebar, 27, 0, 275, 14, 0, 0, 275, 14);
    }

    // 2.5) Draw marquee text from text.bmp.
    const textBmp = skin.sheets["text"];
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
    const numbers = skin.sheets["numbers"];
    if (numbers && status.position !== null) {
      const totalSecs = Math.floor(status.position);
      const mins = Math.floor(totalSecs / 60);
      const secs = totalSecs % 60;
      const digits = [
        Math.floor(mins / 10),
        mins % 10,
        Math.floor(secs / 10),
        secs % 10,
      ];
      // Time display starts at x=48, y=26 — two digits, colon gap, two digits.
      const positions = [36, 48, 60, 78, 90]; // min10, min1, (colon), sec10, sec1
      // First digit (tens of minutes) — only draw if > 0
      if (digits[0] > 0) {
        drawDigit(ctx, numbers, digits[0], positions[0], 26);
      }
      drawDigit(ctx, numbers, digits[1], positions[1], 26);
      drawDigit(ctx, numbers, digits[2], positions[3], 26);
      drawDigit(ctx, numbers, digits[3], positions[4], 26);
    }

    // 5) Draw mono/stereo indicator.
    const monoster = skin.sheets["monoster"];
    if (monoster && status.metadata) {
      const isStereo = (status.metadata.channels ?? 0) >= 2;
      // Stereo indicator at x=212, y=41 in main window
      if (isStereo) {
        ctx.drawImage(monoster, 0, 0, 29, 12, 212, 41, 29, 12);
      } else {
        ctx.drawImage(monoster, 29, 0, 27, 12, 241, 41, 27, 12);
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

      // EQ button (inactive for now)
      ctx.drawImage(shufrep, 0, 61, 23, 12, 219, 58, 23, 12);
      // PL button (active since playlist is shown)
      ctx.drawImage(shufrep, 23, 73, 23, 12, 242, 58, 23, 12);
    }

    // 9) Draw volume slider.
    const volumeSheet = skin.sheets["volume"];
    if (volumeSheet) {
      // The volume BMP has 28 frames stacked vertically, each 68x15.
      // Frame index is determined by the current volume level.
      const frame = Math.round(status.volume * 27);
      const srcY = frame * 15;
      ctx.drawImage(volumeSheet, 0, srcY, 68, 14, 107, 57, 68, 14);
      // Draw thumb.
      const thumbX = Math.round(status.volume * (68 - 14));
      ctx.drawImage(volumeSheet, 15, 422, 14, 11, 107 + thumbX, 58, 14, 11);
    }
  }, [skin, status, playlist, pressed, marqueeOffset, scrollText, fftData]);

  // Click handling.
  const handleMouseDown = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      const rect = canvasRef.current?.getBoundingClientRect();
      if (!rect) return;
      const x = Math.round((e.clientX - rect.left) * (W / rect.width));
      const y = Math.round((e.clientY - rect.top) * (H / rect.height));

      const hit = (r: { x: number; y: number; w: number; h: number }) =>
        x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;

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
        // Seek.
        if (status.duration && status.duration > 0) {
          const fraction = (x - REGIONS.posbar.x) / REGIONS.posbar.w;
          const seekTo = fraction * status.duration;
          invoke("seek", { positionSecs: seekTo });
        }
        setPressed("posbar");
      } else if (hit(REGIONS.volume)) {
        const fraction = Math.max(
          0,
          Math.min(1, (x - REGIONS.volume.x) / REGIONS.volume.w),
        );
        invoke("set_volume", { volume: fraction });
      } else if (hit(REGIONS.close)) {
        // TODO: close app
      }
    },
    [status],
  );

  const handleMouseUp = useCallback(() => {
    setPressed(null);
  }, []);

  return (
    <canvas
      ref={canvasRef}
      width={W}
      height={H}
      style={{
        width: W * scale,
        height: H * scale,
        imageRendering: "pixelated",
        cursor: "default",
        flexShrink: 0,
      }}
      onMouseDown={handleMouseDown}
      onMouseUp={handleMouseUp}
      onMouseLeave={handleMouseUp}
    />
  );
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
