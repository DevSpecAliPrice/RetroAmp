import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open } from "@tauri-apps/plugin-dialog";

interface TrackMetadata {
  title: string | null;
  artist: string | null;
  album: string | null;
  duration: number | null;
  sample_rate: number;
  channels: number;
  genre: string | null;
  year: number | null;
  track_number: number | null;
}

interface EngineStatus {
  state: "Stopped" | "Playing" | "Paused";
  position: number | null;
  duration: number | null;
  metadata: TrackMetadata | null;
  volume: number;
}

interface FftData {
  magnitudes: number[];
  sample_rate: number;
}

function formatTime(seconds: number | null): string {
  if (seconds === null) return "--:--";
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

function App() {
  const [status, setStatus] = useState<EngineStatus>({
    state: "Stopped",
    position: null,
    duration: null,
    metadata: null,
    volume: 1.0,
  });
  const [fftData, setFftData] = useState<number[]>([]);
  const [error, setError] = useState<string | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  // Poll engine status and FFT data.
  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const s = await invoke<EngineStatus>("get_status");
        setStatus(s);

        if (s.state === "Playing") {
          const fft = await invoke<FftData>("get_fft_data");
          setFftData(fft.magnitudes);
        } else {
          setFftData([]);
        }
      } catch (e) {
        console.error("status poll error:", e);
      }
    }, 50);

    return () => clearInterval(interval);
  }, []);

  // Listen for Tauri drag-and-drop events.
  useEffect(() => {
    const webview = getCurrentWebviewWindow();
    const unlisten = webview.onDragDropEvent(async (event) => {
      if (event.payload.type === "drop") {
        const paths = event.payload.paths;
        if (paths.length > 0) {
          try {
            setError(null);
            await invoke("play_file", { path: paths[0] });
          } catch (e) {
            setError(String(e));
          }
        }
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Draw spectrum analyser.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const width = canvas.width;
    const height = canvas.height;
    ctx.fillStyle = "#000";
    ctx.fillRect(0, 0, width, height);

    if (fftData.length === 0) return;

    const barCount = 40;
    const barWidth = Math.floor(width / barCount);
    for (let i = 0; i < barCount; i++) {
      const magnitude = fftData[i + 2] ?? 0;
      const barHeight = Math.min(magnitude * height * 4, height);
      const hue = (i / barCount) * 120;
      ctx.fillStyle = `hsl(${hue}, 100%, 50%)`;
      ctx.fillRect(i * barWidth, height - barHeight, barWidth - 1, barHeight);
    }
  }, [fftData]);

  const openFile = useCallback(async () => {
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: "Audio",
          extensions: [
            "mp3",
            "flac",
            "ogg",
            "wav",
            "aac",
            "m4a",
            "alac",
            "wma",
          ],
        },
      ],
    });

    if (selected) {
      try {
        setError(null);
        await invoke("play_file", { path: selected });
      } catch (e) {
        setError(String(e));
      }
    }
  }, []);

  const meta = status.metadata;
  const title = meta?.title ?? "RetroAmp";
  const artist = meta?.artist ?? "";
  const isPlaying = status.state === "Playing";
  const isPaused = status.state === "Paused";

  return (
    <div
      style={{
        background: "#1a1a2e",
        color: "#00ff41",
        fontFamily: "monospace",
        fontSize: "11px",
        width: "100%",
        height: "100vh",
        display: "flex",
        flexDirection: "column",
        padding: "4px",
        boxSizing: "border-box",
        userSelect: "none",
        overflow: "hidden",
      }}
    >
      {/* Title bar — draggable */}
      <div
        style={{
          fontSize: "10px",
          color: "#888",
          marginBottom: "2px",
          cursor: "move",
        }}
        data-tauri-drag-region
      >
        RETROAMP
      </div>

      {/* Track info */}
      <div
        style={{
          color: "#00ff41",
          fontSize: "12px",
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
        }}
      >
        {artist ? `${artist} - ${title}` : title}
      </div>

      {/* Time display */}
      <div style={{ fontSize: "18px", color: "#00ff41", margin: "2px 0" }}>
        {formatTime(status.position)} / {formatTime(status.duration)}
      </div>

      {/* Spectrum analyser */}
      <canvas
        ref={canvasRef}
        width={260}
        height={30}
        style={{ width: "100%", height: "30px", marginBottom: "4px" }}
      />

      {/* Controls */}
      <div style={{ display: "flex", gap: "4px", alignItems: "center" }}>
        {(isPlaying || isPaused) && (
          <button
            onClick={() => invoke(isPlaying ? "pause" : "resume")}
            style={buttonStyle}
          >
            {isPlaying ? "||" : ">>"}
          </button>
        )}
        {(isPlaying || isPaused) && (
          <button onClick={() => invoke("stop")} style={buttonStyle}>
            []
          </button>
        )}
        <button onClick={openFile} style={buttonStyle}>
          {status.state === "Stopped" ? "OPEN" : "+"}
        </button>
      </div>

      {/* Error display */}
      {error && (
        <div style={{ color: "#ff4444", fontSize: "9px", marginTop: "2px" }}>
          {error}
        </div>
      )}
    </div>
  );
}

const buttonStyle: React.CSSProperties = {
  background: "#333",
  color: "#00ff41",
  border: "1px solid #555",
  padding: "2px 8px",
  fontFamily: "monospace",
  fontSize: "10px",
  cursor: "pointer",
};

export default App;
