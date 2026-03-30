declare module "butterchurn" {
  interface VisualizerOptions {
    width: number;
    height: number;
  }

  interface Visualizer {
    connectAudio(node: AudioNode): void;
    disconnectAudio(node: AudioNode): void;
    loadPreset(preset: object, blendTime: number): void;
    render(): void;
    setRendererSize(width: number, height: number): void;
    /** Internal AudioProcessor — has analyser nodes we need to patch. */
    audio: {
      analyser: AnalyserNode;
      analyserL: AnalyserNode;
      analyserR: AnalyserNode;
      [key: string]: unknown;
    };
    renderer: { [key: string]: unknown };
  }

  const butterchurn: {
    createVisualizer(
      audioContext: AudioContext,
      canvas: HTMLCanvasElement,
      options: VisualizerOptions
    ): Visualizer;
  };

  export default butterchurn;
}

declare module "butterchurn-presets" {
  const butterchurnPresets: {
    getPresets(): Record<string, object>;
  };

  export default butterchurnPresets;
}
