/**
 * Skinned equalizer window — HTML/CSS sprite-based approach (like PlaylistWindow).
 *
 * Uses individual sprite data URIs from eqmain.bmp positioned via CSS.
 * Slider frames selected via background-position into the slider sprite sheet.
 * Only the EQ graph curve uses a small canvas overlay.
 *
 * All native Winamp pixel dimensions are multiplied by `scale` for proper sizing.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { SkinData } from "./parser";
import ContextMenu, { type MenuEntry } from "./ContextMenu";

// -- Native Winamp dimensions (before scaling) --

/** X positions for each slider (preamp + 10 bands) in native px. */
const SLIDER_X = [21, 78, 96, 114, 132, 150, 168, 186, 204, 222, 240];
const SLIDER_Y = 38;
const FRAME_W = 15;
const FRAME_H = 65;
const FRAME_COLS = 14;
const THUMB_W = 11;
const THUMB_H = 11;
const SLIDER_TRAVEL = FRAME_H - THUMB_H; // 54px native thumb travel

const GRAPH_X = 86;
const GRAPH_Y = 17;
const GRAPH_W = 113;
const GRAPH_H = 19;

// -- EQ Presets --

interface EqPreset {
  name: string;
  gains: number[];
  preamp: number;
}

const PRESETS: EqPreset[] = [
  { name: "Flat",        gains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0], preamp: 0 },
  { name: "Rock",        gains: [4.8, 2.4, -1.2, -3.6, -1.2, 2.4, 4.8, 7.2, 7.2, 7.2], preamp: 0 },
  { name: "Pop",         gains: [-1.6, 2.8, 4.4, 4.8, 3.2, -0.4, -1.2, -1.6, -1.6, -1.6], preamp: 0 },
  { name: "Jazz",        gains: [0, 0, 0, 3.6, 3.6, 3.6, 0, 1.2, 2.4, 3.6], preamp: 0 },
  { name: "Classical",   gains: [0, 0, 0, 0, 0, 0, -4.4, -4.4, -4.4, -6.0], preamp: 0 },
  { name: "Dance",       gains: [5.6, 4.4, 1.2, 0, 0, -3.6, -4.4, -4.4, 0, 0], preamp: 0 },
  { name: "Full Bass",   gains: [6.0, 6.0, 6.0, 3.6, 1.2, -2.4, -4.8, -6.4, -7.2, -7.2], preamp: 0 },
  { name: "Full Treble", gains: [-6.0, -6.0, -6.0, -2.4, 1.2, 6.8, 9.6, 9.6, 9.6, 10.4], preamp: 0 },
  { name: "Laptop",      gains: [2.4, 6.8, 3.2, -1.2, -1.2, 1.2, 2.4, 5.6, 8.0, 8.8], preamp: 0 },
  { name: "Large Hall",  gains: [6.0, 6.0, 3.2, 3.2, 0, -2.8, -2.8, -2.8, 0, 0], preamp: 0 },
  { name: "Live",        gains: [-2.8, 0, 2.4, 3.2, 3.2, 3.2, 2.4, 1.2, 1.2, 1.2], preamp: 0 },
  { name: "Soft",        gains: [2.4, 0.8, 0, -1.2, 0, 2.4, 4.8, 5.6, 6.8, 7.2], preamp: 0 },
  { name: "Ska",         gains: [-1.2, -2.8, -2.4, 0, 2.4, 3.2, 4.8, 5.6, 6.8, 5.6], preamp: 0 },
  { name: "Reggae",      gains: [0, 0, 0, -3.2, 0, 3.8, 3.8, 0, 0, 0], preamp: 0 },
  { name: "Techno",      gains: [4.8, 3.2, 0, -3.2, -2.8, 0, 4.8, 5.6, 5.6, 4.4], preamp: 0 },
];

// -- Interfaces --

interface EqSettings {
  gains: number[];
  enabled: boolean;
  preamp: number;
}

interface Props {
  skin: SkinData;
  scale: number;
}

// -- Helpers --

function dbToFraction(db: number): number {
  return (12 - db) / 24;
}

function fractionToDb(f: number): number {
  return 12 - f * 24;
}

// -- Component --

export default function EqualizerWindow({ skin }: Props) {
  const s = Math.max(1, Math.round(window.innerWidth / 275));
  const ps = skin.playlistStyle;
  const sp = skin.sprites;

  const graphCanvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [settings, setSettings] = useState<EqSettings>({
    gains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    enabled: true,
    preamp: 0,
  });
  const [pressed, setPressed] = useState<string | null>(null);
  const dragging = useRef<{ sliderIndex: number } | null>(null);
  const [presetsMenu, setPresetsMenu] = useState<{ x: number; y: number } | null>(null);
  const [eqContextMenu, setEqContextMenu] = useState<{ x: number; y: number } | null>(null);

  // Fetch current EQ settings on mount.
  useEffect(() => {
    invoke<EqSettings>("get_eq").then(setSettings).catch(console.error);
  }, []);

  const applySettings = useCallback((newSettings: EqSettings) => {
    setSettings(newSettings);
    invoke("set_eq", { settings: newSettings });
  }, []);

  // -- EQ graph rendering (small canvas) --

  useEffect(() => {
    const canvas = graphCanvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    ctx.clearRect(0, 0, GRAPH_W, GRAPH_H);

    // Graph background from sprite.
    const eq = skin.sheets["eqmain"];
    if (eq) {
      ctx.drawImage(eq, 0, 294, GRAPH_W, GRAPH_H, 0, 0, GRAPH_W, GRAPH_H);
    }

    if (settings.enabled) {
      const points = settings.gains.map((db, i) => ({
        x: Math.round(i * (GRAPH_W - 1) / 9),
        y: Math.round(GRAPH_H / 2 - (db / 12) * (GRAPH_H / 2 - 1)),
      }));

      ctx.strokeStyle = "#00ff00";
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (let i = 0; i < points.length; i++) {
        if (i === 0) ctx.moveTo(points[i].x, points[i].y);
        else ctx.lineTo(points[i].x, points[i].y);
      }
      ctx.stroke();

      // Preamp line.
      const preampY = Math.round(GRAPH_H / 2 - (settings.preamp / 12) * (GRAPH_H / 2 - 1));
      ctx.strokeStyle = "#ff8800";
      ctx.setLineDash([2, 2]);
      ctx.beginPath();
      ctx.moveTo(0, preampY);
      ctx.lineTo(GRAPH_W, preampY);
      ctx.stroke();
      ctx.setLineDash([]);
    }
  }, [skin, settings]);

  // -- Convert mouse event to native EQ coordinates --

  const getNativePos = useCallback((e: React.MouseEvent | MouseEvent) => {
    const el = containerRef.current;
    if (!el) return null;
    const rect = el.getBoundingClientRect();
    return {
      x: (e.clientX - rect.left) / s,
      y: (e.clientY - rect.top) / s,
    };
  }, [s]);

  // -- Slider value from mouse Y --

  const fractionFromY = useCallback((nativeY: number) => {
    return Math.max(0, Math.min(1, (nativeY - SLIDER_Y - THUMB_H / 2) / SLIDER_TRAVEL));
  }, []);

  const applySliderValue = useCallback(
    (index: number, fraction: number) => {
      const snapped = Math.abs(fraction - 0.5) < 0.04 ? 0.5 : fraction;
      const db = fractionToDb(snapped);
      const clamped = Math.round(db * 10) / 10;

      if (index === 0) {
        applySettings({ ...settings, preamp: clamped });
      } else {
        const newGains = [...settings.gains];
        newGains[index - 1] = clamped;
        applySettings({ ...settings, gains: newGains });
      }
    },
    [settings, applySettings],
  );

  // -- Mouse handlers --

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      const pos = getNativePos(e);
      if (!pos) return;
      const { x, y } = pos;

      // Check slider hits first (most common interaction).
      for (let i = 0; i < 11; i++) {
        const sx = SLIDER_X[i];
        if (x >= sx && x < sx + FRAME_W && y >= SLIDER_Y && y < SLIDER_Y + FRAME_H) {
          dragging.current = { sliderIndex: i };
          applySliderValue(i, fractionFromY(y));
          return;
        }
      }
    },
    [getNativePos, applySliderValue, fractionFromY],
  );

  // Global drag listeners.
  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      const pos = getNativePos(e);
      if (!pos) return;
      applySliderValue(dragging.current.sliderIndex, fractionFromY(pos.y));
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
  }, [getNativePos, applySliderValue, fractionFromY]);

  const handleDoubleClick = useCallback(
    (e: React.MouseEvent) => {
      const pos = getNativePos(e);
      if (!pos) return;
      const { x, y } = pos;
      for (let i = 0; i < 11; i++) {
        const sx = SLIDER_X[i];
        if (x >= sx && x < sx + FRAME_W && y >= SLIDER_Y && y < SLIDER_Y + FRAME_H) {
          if (i === 0) {
            applySettings({ ...settings, preamp: 0 });
          } else {
            const newGains = [...settings.gains];
            newGains[i - 1] = 0;
            applySettings({ ...settings, gains: newGains });
          }
          return;
        }
      }
    },
    [settings, getNativePos, applySettings],
  );

  // Close presets menu.
  useEffect(() => {
    if (!presetsMenu) return;
    const close = () => setPresetsMenu(null);
    const timer = setTimeout(() => window.addEventListener("mousedown", close), 0);
    return () => { clearTimeout(timer); window.removeEventListener("mousedown", close); };
  }, [presetsMenu]);

  // -- Sprite helpers --

  const bgSprite = (name: string) => sp[name]
    ? { backgroundImage: `url(${sp[name]})`, backgroundSize: "100% 100%", backgroundRepeat: "no-repeat" as const }
    : {};

  /** Select a frame from the slider sprite sheet by index (0-27). */
  const sliderFrameBg = (frameIndex: number) => {
    const col = frameIndex % FRAME_COLS;
    const row = Math.floor(frameIndex / FRAME_COLS);
    const sheetUri = sp["EQ_SLIDER_BACKGROUND"];
    if (!sheetUri) return {};
    // The sprite sheet is 209x129 native. Each frame is 15x65.
    // CSS background-position offsets into the scaled sheet.
    return {
      backgroundImage: `url(${sheetUri})`,
      backgroundSize: `${209 * s}px ${129 * s}px`,
      backgroundPosition: `${-col * FRAME_W * s}px ${-row * FRAME_H * s}px`,
      backgroundRepeat: "no-repeat" as const,
    };
  };

  // -- Build slider values --
  const allValues = [settings.preamp, ...settings.gains];

  // -- ON button sprite selection --
  const onBtnSprite = (() => {
    const active = settings.enabled;
    const isPressed = pressed === "on";
    if (active && isPressed) return "EQ_ON_BUTTON_SELECTED_DEPRESSED";
    if (active) return "EQ_ON_BUTTON_SELECTED";
    if (isPressed) return "EQ_ON_BUTTON_DEPRESSED";
    return "EQ_ON_BUTTON";
  })();

  return (
    <div
      ref={containerRef}
      style={{
        width: 275 * s,
        height: 116 * s,
        position: "relative",
        imageRendering: "pixelated" as any,
        overflow: "hidden",
      }}
      onMouseDown={handleMouseDown}
      onDoubleClick={handleDoubleClick}
      onMouseUp={() => setPressed(null)}
      onContextMenu={(e) => {
        e.preventDefault();
        setEqContextMenu({ x: e.clientX, y: e.clientY });
      }}
    >
      {/* 1) Full EQ background */}
      <div style={{
        position: "absolute", left: 0, top: 0,
        width: 275 * s, height: 116 * s,
        ...bgSprite("EQ_WINDOW_BACKGROUND"),
      }} />

      {/* 2) Active title bar overlay */}
      <div
        style={{
          position: "absolute", left: 0, top: 0,
          width: 275 * s, height: 14 * s,
          ...bgSprite("EQ_TITLE_BAR_SELECTED"),
          cursor: "move",
        }}
        onMouseDown={(e) => {
          // Don't drag from buttons.
          if ((e.target as HTMLElement).dataset.action) return;
          e.stopPropagation();
          getCurrentWindow().startDragging();
        }}
      >
        {/* Close button */}
        <div
          data-action="close"
          style={{
            position: "absolute", right: 3 * s, top: 3 * s,
            width: 9 * s, height: 9 * s, cursor: "pointer",
          }}
          onMouseDown={(e) => {
            e.stopPropagation();
            setPressed("close");
            invoke("toggle_window", { windowId: "Equalizer" });
          }}
        />
      </div>

      {/* 3) ON button */}
      <div
        style={{
          position: "absolute", left: 14 * s, top: 18 * s,
          width: 26 * s, height: 12 * s, cursor: "pointer",
          ...bgSprite(onBtnSprite),
        }}
        onMouseDown={(e) => {
          e.stopPropagation();
          setPressed("on");
          applySettings({ ...settings, enabled: !settings.enabled });
        }}
      />

      {/* 4) AUTO button */}
      <div
        style={{
          position: "absolute", left: 40 * s, top: 18 * s,
          width: 32 * s, height: 12 * s, cursor: "pointer",
          ...bgSprite(pressed === "auto" ? "EQ_AUTO_BUTTON_DEPRESSED" : "EQ_AUTO_BUTTON"),
        }}
        onMouseDown={(e) => {
          e.stopPropagation();
          setPressed("auto");
        }}
      />

      {/* 5) Presets button */}
      <div
        style={{
          position: "absolute", left: 217 * s, top: 18 * s,
          width: 44 * s, height: 12 * s, cursor: "pointer",
          ...bgSprite(pressed === "presets" ? "EQ_PRESETS_BUTTON_SELECTED" : "EQ_PRESETS_BUTTON"),
        }}
        onMouseDown={(e) => {
          e.stopPropagation();
          setPressed("presets");
          setPresetsMenu({ x: e.clientX, y: e.clientY });
        }}
      />

      {/* 6) EQ graph (small canvas overlay) */}
      <canvas
        ref={graphCanvasRef}
        width={GRAPH_W}
        height={GRAPH_H}
        style={{
          position: "absolute",
          left: GRAPH_X * s,
          top: GRAPH_Y * s,
          width: GRAPH_W * s,
          height: GRAPH_H * s,
          imageRendering: "pixelated",
          pointerEvents: "none",
        }}
      />

      {/* 7) Sliders (preamp + 10 bands) */}
      {allValues.map((db, i) => {
        const fraction = dbToFraction(db);
        const frameIndex = Math.round(fraction * 27);
        const thumbY = 1 + Math.round(fraction * SLIDER_TRAVEL);

        return (
          <div key={i} style={{
            position: "absolute",
            left: SLIDER_X[i] * s,
            top: SLIDER_Y * s,
            width: FRAME_W * s,
            height: FRAME_H * s,
          }}>
            {/* Slider background frame */}
            <div style={{
              position: "absolute", left: 0, top: 0,
              width: FRAME_W * s, height: FRAME_H * s,
              ...sliderFrameBg(frameIndex),
            }} />
            {/* Thumb */}
            <div style={{
              position: "absolute",
              left: 2 * s,
              top: thumbY * s,
              width: THUMB_W * s,
              height: THUMB_H * s,
              ...bgSprite(dragging.current?.sliderIndex === i
                ? "EQ_SLIDER_THUMB_SELECTED"
                : "EQ_SLIDER_THUMB"),
            }} />
          </div>
        );
      })}

      {/* Presets context menu */}
      {presetsMenu && createPortal(
        <div
          style={{
            position: "fixed",
            left: presetsMenu.x,
            top: presetsMenu.y,
            background: ps.normalbg,
            border: `1px solid ${ps.selectedbg}`,
            padding: "4px 0",
            zIndex: 1000,
            fontFamily: `"${ps.font}", system-ui, sans-serif`,
            fontSize: 12,
            color: ps.normal,
            minWidth: 160,
            maxHeight: 400,
            overflowY: "auto",
            boxShadow: "2px 2px 8px rgba(0,0,0,0.5)",
          }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          <PresetItem
            label="Reset (Flat)"
            hoverBg={ps.selectedbg}
            onClick={() => {
              applySettings({ ...settings, gains: [0,0,0,0,0,0,0,0,0,0], preamp: 0 });
              setPresetsMenu(null);
            }}
          />
          <div style={{ height: 1, background: ps.selectedbg, margin: "4px 0" }} />
          {PRESETS.map((p) => (
            <PresetItem
              key={p.name}
              label={p.name}
              hoverBg={ps.selectedbg}
              onClick={() => {
                applySettings({ ...settings, gains: [...p.gains], preamp: p.preamp });
                setPresetsMenu(null);
              }}
            />
          ))}
        </div>,
        document.body,
      )}

      {eqContextMenu && (
        <ContextMenu
          x={eqContextMenu.x}
          y={eqContextMenu.y}
          colors={ps}
          onClose={() => setEqContextMenu(null)}
          items={[
            {
              label: settings.enabled ? "Disable EQ" : "Enable EQ",
              onClick: () => applySettings({ ...settings, enabled: !settings.enabled }),
            },
            {
              label: "Reset to Flat",
              onClick: () => applySettings({ ...settings, gains: [0,0,0,0,0,0,0,0,0,0], preamp: 0 }),
            },
            "separator",
            { label: "Preferences...", onClick: () => invoke("open_settings") },
          ] satisfies MenuEntry[]}
        />
      )}
    </div>
  );
}

function PresetItem({ label, onClick, hoverBg }: { label: string; onClick: () => void; hoverBg: string }) {
  return (
    <div
      style={{ padding: "5px 12px", cursor: "pointer" }}
      onMouseEnter={(e) => ((e.target as HTMLElement).style.background = hoverBg)}
      onMouseLeave={(e) => ((e.target as HTMLElement).style.background = "transparent")}
      onMouseDown={(e) => {
        e.stopPropagation();
        onClick();
      }}
    >
      {label}
    </div>
  );
}
