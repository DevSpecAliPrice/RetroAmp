/**
 * Bridges Rust-sourced FFT data to Butterchurn's expected AnalyserNode interface.
 *
 * Butterchurn expects a Web Audio API AudioNode via connectAudio(), which it
 * internally wires to an AnalyserNode. Since our audio is decoded in Rust,
 * we create a dummy audio graph and override the AnalyserNode's data methods
 * to return our Rust-sourced FFT and waveform data.
 */

/** FFT data payload from the Rust backend. */
export interface FftData {
  magnitudes: number[];
  waveform: number[];
  sample_rate: number;
}

export class AudioAdapter {
  readonly audioContext: AudioContext;
  private frequencyData: Uint8Array;
  private timeDomainData: Uint8Array;
  private dummyNode: OscillatorNode;

  constructor() {
    this.audioContext = new AudioContext();
    // 512 frequency bins (matching our 1024-point FFT)
    this.frequencyData = new Uint8Array(512);
    // 1024 time-domain samples
    this.timeDomainData = new Uint8Array(1024);
    this.timeDomainData.fill(128); // silence = 128

    // Silent oscillator so connectAudio() has a valid node
    this.dummyNode = this.audioContext.createOscillator();
    this.dummyNode.frequency.value = 0;
    this.dummyNode.start();
  }

  /** The dummy AudioNode to pass to Butterchurn's connectAudio(). */
  get audioNode(): AudioNode {
    return this.dummyNode;
  }

  /**
   * After Butterchurn calls connectAudio() and creates its internal
   * AnalyserNode, call this to override the data methods so they
   * return our Rust-sourced data instead of analysing the silent dummy.
   */
  patchAnalyserNode(analyserNode: AnalyserNode): void {
    const self = this;

    analyserNode.getByteFrequencyData = (array: Uint8Array) => {
      const len = Math.min(array.length, self.frequencyData.length);
      array.set(self.frequencyData.subarray(0, len));
    };

    analyserNode.getByteTimeDomainData = (array: Uint8Array) => {
      const len = Math.min(array.length, self.timeDomainData.length);
      array.set(self.timeDomainData.subarray(0, len));
    };
  }

  /** Convert and store the latest FFT data from the Rust backend. */
  update(data: FftData): void {
    const { magnitudes, waveform } = data;

    // Frequency: float [0.0, 1.0] → Uint8 [0, 255]
    for (let i = 0; i < this.frequencyData.length; i++) {
      if (i < magnitudes.length) {
        this.frequencyData[i] = Math.min(255, Math.max(0, Math.round(magnitudes[i] * 255)));
      } else {
        this.frequencyData[i] = 0;
      }
    }

    // Waveform: float [-1.0, 1.0] → Uint8 [0, 255] (128 = zero crossing)
    for (let i = 0; i < this.timeDomainData.length; i++) {
      if (i < waveform.length) {
        this.timeDomainData[i] = Math.min(255, Math.max(0, Math.round((waveform[i] + 1.0) * 127.5)));
      } else {
        this.timeDomainData[i] = 128;
      }
    }
  }

  /** Resume the AudioContext (required after user gesture due to autoplay policy). */
  async resume(): Promise<void> {
    if (this.audioContext.state === "suspended") {
      await this.audioContext.resume();
    }
  }

  dispose(): void {
    this.dummyNode.stop();
    this.dummyNode.disconnect();
    void this.audioContext.close();
  }
}
