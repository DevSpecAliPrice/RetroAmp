import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import butterchurnRaw from "butterchurn";
import butterchurnPresetsRaw from "butterchurn-presets";

// CJS/ESM interop — the default import may be the module wrapper or the class directly
const butterchurn = (butterchurnRaw as any).default ?? butterchurnRaw;
const butterchurnPresets = (butterchurnPresetsRaw as any).default ?? butterchurnPresetsRaw;
import type { SkinData } from "../skin/parser";
import { AudioAdapter, type FftData } from "./AudioAdapter";
import { showContextMenu, type NativeMenuEntry } from "../nativeMenu";

interface Props {
  skin: SkinData;
  scale: number;
}

const PRESET_CYCLE_SECS = 30;
const BLEND_SECS = 2.0;
const RESIZE_EDGE = 5;

export default function VisualizerWindow({ skin, scale }: Props) {
  const s = scale || Math.max(1, Math.round(window.innerWidth / 275));
  const sp = skin.sprites;

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const adapterRef = useRef<AudioAdapter | null>(null);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const visualizerRef = useRef<any>(null);
  const rafRef = useRef<number>(0);
  const fetchRafRef = useRef<number>(0);
  const cycleTimerRef = useRef<ReturnType<typeof setInterval>>(0 as unknown as ReturnType<typeof setInterval>);
  const initDoneRef = useRef(false);

  const [presetName, setPresetName] = useState("");
  const [showPresetName, setShowPresetName] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const presetNamesRef = useRef<string[]>([]);
  const presetIndexRef = useRef(0);
  const fadeTimerRef = useRef<ReturnType<typeof setTimeout>>(0 as unknown as ReturnType<typeof setTimeout>);

  const bg = (name: string) => ({
    backgroundImage: sp[name] ? `url(${sp[name]})` : "none",
    backgroundRepeat: "no-repeat" as const,
    backgroundSize: "100% 100%",
  });

  const bgTile = (name: string, dir: "repeat-x" | "repeat-y") => ({
    backgroundImage: sp[name] ? `url(${sp[name]})` : "none",
    backgroundRepeat: dir,
    backgroundSize: dir === "repeat-y" ? "100% auto" : "auto 100%",
  });

  const loadPresetByIndex = useCallback((index: number, blend: number) => {
    const names = presetNamesRef.current;
    if (!names.length || !visualizerRef.current) return;
    const wrapped = ((index % names.length) + names.length) % names.length;
    presetIndexRef.current = wrapped;
    const name = names[wrapped];
    const presets = butterchurnPresets.getPresets();
    visualizerRef.current.loadPreset(presets[name], blend);
    setPresetName(name);
    setShowPresetName(true);
    clearTimeout(fadeTimerRef.current);
    fadeTimerRef.current = setTimeout(() => setShowPresetName(false), 3000);
    // Persist for next session
    invoke("set_last_visualizer_preset", { preset: name }).catch(() => {});
  }, []);

  const loadPresetByName = useCallback((name: string, blend: number) => {
    const names = presetNamesRef.current;
    const idx = names.indexOf(name);
    if (idx >= 0) loadPresetByIndex(idx, blend);
  }, [loadPresetByIndex]);

  const nextPreset = useCallback(() => {
    loadPresetByIndex(presetIndexRef.current + 1, BLEND_SECS);
  }, [loadPresetByIndex]);

  const prevPreset = useCallback(() => {
    loadPresetByIndex(presetIndexRef.current - 1, BLEND_SECS);
  }, [loadPresetByIndex]);

  const randomPreset = useCallback(() => {
    const names = presetNamesRef.current;
    if (!names.length) return;
    loadPresetByIndex(Math.floor(Math.random() * names.length), BLEND_SECS);
  }, [loadPresetByIndex]);

  /**
   * Initialise Butterchurn. Called once the canvas has real dimensions
   * (deferred until the window is visible — hidden windows have 0x0 layout).
   */
  const initVisualizer = useCallback(async (canvas: HTMLCanvasElement, width: number, height: number) => {
    if (initDoneRef.current) return;
    initDoneRef.current = true;

    try {
      // Check WebGL2 support on a throwaway canvas (not the real one,
      // since getContext returns the same context and we can't reset it).
      const probe = document.createElement("canvas");
      const testGl = probe.getContext("webgl2");
      if (!testGl) {
        setError("WebGL2 is not available in this WebView. Visualizer requires WebGL2 support.");
        console.error("[visualizer] WebGL2 not available");
        return;
      }
      testGl.getExtension("WEBGL_lose_context")?.loseContext();

      const adapter = new AudioAdapter();
      adapterRef.current = adapter;
      await adapter.resume();

      canvas.width = width;
      canvas.height = height;

      console.log(`[visualizer] creating butterchurn: ${width}x${height}`);
      const viz = butterchurn.createVisualizer(adapter.audioContext, canvas, {
        width,
        height,
      });

      // Wire up the dummy audio node so Butterchurn creates its internal audio graph
      viz.connectAudio(adapter.audioNode);

      // Patch the internal AnalyserNodes to use our Rust-sourced data.
      // Butterchurn reads ONLY getByteTimeDomainData (then does its own FFT),
      // so patching time-domain on all three analysers is what matters.
      if (viz.audio?.analyser) {
        adapter.patchAnalyserNode(viz.audio.analyser);
        console.log("[visualizer] patched main analyser");
      }
      if (viz.audio?.analyserL) {
        adapter.patchAnalyserNode(viz.audio.analyserL);
      }
      if (viz.audio?.analyserR) {
        adapter.patchAnalyserNode(viz.audio.analyserR);
      }

      visualizerRef.current = viz;

      // Load presets
      const presets = butterchurnPresets.getPresets();
      const names = Object.keys(presets).sort();
      presetNamesRef.current = names;
      console.log(`[visualizer] loaded ${names.length} presets`);

      // Restore last preset, or pick a random one on first launch
      let startIndex = Math.floor(Math.random() * names.length);
      try {
        const lastPreset = await invoke<string | null>("get_last_visualizer_preset");
        if (lastPreset) {
          const saved = names.indexOf(lastPreset);
          if (saved >= 0) startIndex = saved;
        }
      } catch { /* first launch — use random */ }

      if (names.length > 0) {
        presetIndexRef.current = startIndex;
        viz.loadPreset(presets[names[startIndex]], 0);
        setPresetName(names[startIndex]);
        setShowPresetName(true);
        fadeTimerRef.current = setTimeout(() => setShowPresetName(false), 3000);
        invoke("set_last_visualizer_preset", { preset: names[startIndex] }).catch(() => {});
      }

      // Render loop — always runs at display refresh rate
      const renderLoop = () => {
        if (visualizerRef.current) {
          visualizerRef.current.render();
        }
        rafRef.current = requestAnimationFrame(renderLoop);
      };
      rafRef.current = requestAnimationFrame(renderLoop);

      // Data fetch loop — async, decoupled from render
      const fetchLoop = () => {
        invoke<FftData>("get_fft_data")
          .then((data) => {
            if (adapterRef.current) {
              adapterRef.current.update(data);
            }
          })
          .catch(() => {});
        fetchRafRef.current = requestAnimationFrame(fetchLoop);
      };
      fetchRafRef.current = requestAnimationFrame(fetchLoop);

      // Auto-cycle presets
      cycleTimerRef.current = setInterval(() => {
        const names = presetNamesRef.current;
        if (names.length > 0) {
          const next = Math.floor(Math.random() * names.length);
          loadPresetByIndex(next, BLEND_SECS);
        }
      }, PRESET_CYCLE_SECS * 1000);

      console.log("[visualizer] init complete");
    } catch (e) {
      console.error("[visualizer] init failed:", e);
      setError(`Visualizer init failed: ${e}`);
      initDoneRef.current = false; // allow retry
    }
  }, [loadPresetByIndex]);

  // Use ResizeObserver to detect when the canvas first gets real dimensions
  // (i.e. when the window becomes visible) and to handle subsequent resizes.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const { width, height } = entry.contentRect;
      const w = Math.max(1, Math.floor(width));
      const h = Math.max(1, Math.floor(height));

      if (w <= 1 || h <= 1) return; // still hidden / no layout

      const canvas = canvasRef.current;
      if (!canvas) return;

      if (!initDoneRef.current) {
        // First time we have real dimensions — init Butterchurn
        initVisualizer(canvas, w, h);
      } else {
        // Subsequent resize
        canvas.width = w;
        canvas.height = h;
        if (visualizerRef.current) {
          visualizerRef.current.setRendererSize(w, h);
        }
      }
    });

    observer.observe(container);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(rafRef.current);
      cancelAnimationFrame(fetchRafRef.current);
      clearInterval(cycleTimerRef.current);
      clearTimeout(fadeTimerRef.current);
      if (adapterRef.current) {
        adapterRef.current.dispose();
        adapterRef.current = null;
      }
      visualizerRef.current = null;
      initDoneRef.current = false;
    };
  }, [initVisualizer]);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      switch (e.key) {
        case " ":
        case "ArrowRight":
          e.preventDefault();
          nextPreset();
          break;
        case "ArrowLeft":
          e.preventDefault();
          prevPreset();
          break;
        case "r":
          e.preventDefault();
          randomPreset();
          break;
        case "Escape":
          invoke("toggle_window", { windowId: "Visualizer" }).catch(console.error);
          break;
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [nextPreset, prevPreset, randomPreset]);

  // Build preset submenu items grouped by first character
  const buildPresetMenu = useCallback((): NativeMenuEntry => {
    const names = presetNamesRef.current;
    const current = names[presetIndexRef.current] ?? "";

    // Group by first character (uppercased). Symbols go under "#".
    const groups: Record<string, NativeMenuEntry[]> = {};
    for (const name of names) {
      const first = name[0]?.toUpperCase() ?? "#";
      const key = /[A-Z]/.test(first) ? first : "#";
      if (!groups[key]) groups[key] = [];
      const label = name === current ? `● ${name}` : name;
      groups[key].push({ type: "item", id: `preset:${name}`, label });
    }

    const sortedKeys = Object.keys(groups).sort((a, b) =>
      a === "#" ? -1 : b === "#" ? 1 : a.localeCompare(b)
    );

    const submenus: NativeMenuEntry[] = sortedKeys.map((key) => ({
      type: "submenu" as const,
      label: key === "#" ? "# Symbols" : key,
      items: groups[key],
    }));

    return { type: "submenu", label: "Presets", items: submenus };
  }, []);

  // Context menu
  const handleContextMenu = useCallback(
    async (e: React.MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const items: NativeMenuEntry[] = [
        { type: "item", id: "next", label: "Next Preset →" },
        { type: "item", id: "prev", label: "← Previous Preset" },
        { type: "item", id: "random", label: "Random Preset" },
        { type: "separator" },
        buildPresetMenu(),
        { type: "separator" },
        { type: "item", id: "close", label: "Close Visualizer" },
      ];

      const selected = await showContextMenu(items, e.clientX, e.clientY);
      if (!selected) return;

      if (selected === "next") nextPreset();
      else if (selected === "prev") prevPreset();
      else if (selected === "random") randomPreset();
      else if (selected === "close") invoke("toggle_window", { windowId: "Visualizer" }).catch(console.error);
      else if (selected.startsWith("preset:")) loadPresetByName(selected.slice(7), BLEND_SECS);
    },
    [nextPreset, prevPreset, randomPreset, buildPresetMenu, loadPresetByName]
  );

  // Edge resize (top/bottom edges)
  const handleEdgeMouseDown = useCallback((e: React.MouseEvent) => {
    const h = window.innerHeight;
    const y = e.clientY;
    if (y < RESIZE_EDGE) {
      e.preventDefault(); e.stopPropagation();
      getCurrentWindow().startResizeDragging("North" as any);
    } else if (y > h - RESIZE_EDGE) {
      e.preventDefault(); e.stopPropagation();
      getCurrentWindow().startResizeDragging("South" as any);
    }
  }, []);

  const ps = skin.playlistStyle;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100vh",
        overflow: "hidden",
        userSelect: "none",
        imageRendering: "pixelated" as never,
      }}
      onMouseDown={handleEdgeMouseDown}
      onContextMenu={handleContextMenu}
    >
      {/* ── TOP BAR ── */}
      <div
        style={{
          display: "flex", height: 20 * s, minHeight: 20 * s, flexShrink: 0, cursor: "move",
        }}
        onMouseDown={(e) => {
          if ((e.target as HTMLElement).closest("[data-action]")) return;
          e.stopPropagation();
          getCurrentWindow().startDragging();
        }}
      >
        <div style={{ width: 25 * s, height: 20 * s, flexShrink: 0, ...bg("PL_TOP_LEFT_SELECTED") }} />
        <div style={{ flex: 1, ...bgTile("PL_TOP_TILE_SELECTED", "repeat-x") }} />
        <div style={{
          width: 25 * s, height: 20 * s, flexShrink: 0, position: "relative",
          ...bg("PL_TOP_RIGHT_SELECTED"),
        }}>
          <div
            data-action="close"
            style={{
              position: "absolute", right: 3 * s, top: 3 * s,
              width: 9 * s, height: 9 * s, cursor: "pointer",
            }}
            onClick={() => invoke("toggle_window", { windowId: "Visualizer" }).catch(console.error)}
          />
        </div>
      </div>

      {/* ── MIDDLE (left border + content + right border) ── */}
      <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
        <div style={{ width: 12 * s, flexShrink: 0, ...bgTile("PL_LEFT_TILE", "repeat-y") }} />

        {/* Content area — canvas fills the skinned interior */}
        <div
          ref={containerRef}
          style={{
            flex: 1, position: "relative", overflow: "hidden",
            background: ps?.normalbg ?? "#000",
          }}
        >
          <canvas
            ref={canvasRef}
            style={{ width: "100%", height: "100%", display: "block" }}
          />

          {/* Error overlay */}
          {error && (
            <div
              style={{
                position: "absolute", inset: 0,
                display: "flex", alignItems: "center", justifyContent: "center",
                color: "#ff4444", fontFamily: "monospace", fontSize: 13,
                padding: 20, textAlign: "center",
              }}
            >
              {error}
            </div>
          )}

          {/* Preset name overlay */}
          {!error && (
            <div
              style={{
                position: "absolute", bottom: 12, left: 0, right: 0,
                textAlign: "center", pointerEvents: "none",
                opacity: showPresetName ? 1 : 0,
                transition: "opacity 0.5s ease-out",
              }}
            >
              <span
                style={{
                  color: "#fff", fontFamily: "monospace", fontSize: 13,
                  textShadow: "0 1px 4px rgba(0,0,0,0.8)",
                  background: "rgba(0,0,0,0.4)",
                  padding: "4px 10px", borderRadius: 4,
                }}
              >
                {presetName}
              </span>
            </div>
          )}
        </div>

        <div style={{
          width: 20 * s, flexShrink: 0,
          ...bgTile("PL_RIGHT_TILE", "repeat-y"),
        }} />
      </div>

      {/* ── BOTTOM BAR — flipped top sprites for clean corner transitions ── */}
      <div style={{ display: "flex", height: 20 * s, minHeight: 20 * s, flexShrink: 0 }}>
        <div style={{ width: 25 * s, flexShrink: 0, ...bg("PL_TOP_LEFT_SELECTED"), transform: "scaleY(-1)" }} />
        <div style={{ flex: 1, minWidth: 0, overflow: "hidden", ...bgTile("PL_TOP_TILE_SELECTED", "repeat-x"), transform: "scaleY(-1)" }} />
        <div style={{ width: 25 * s, flexShrink: 0, position: "relative", ...bg("PL_TOP_RIGHT_SELECTED"), transform: "scaleY(-1)" }}>
          <div
            style={{ position: "absolute", right: 0, top: 0, width: 20 * s, height: 20 * s, cursor: "se-resize" }}
            onMouseDown={(e) => { e.preventDefault(); e.stopPropagation(); getCurrentWindow().startResizeDragging("SouthEast" as any); }}
          />
        </div>
      </div>
    </div>
  );
}
