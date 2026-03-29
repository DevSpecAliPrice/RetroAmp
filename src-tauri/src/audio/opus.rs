//! Opus decoder for Symphonia — wraps audiopus (libopus bindings) to provide
//! Opus codec support for all audio sources (YouTube, radio, local files).
//!
//! Registers as a Symphonia `Decoder` for `CODEC_TYPE_OPUS`, so any
//! FormatReader that encounters Opus audio (WebM, OGG) can decode it
//! automatically through the standard Symphonia pipeline.

use symphonia::core::audio::{
    AsAudioBufferRef, AudioBuffer, AudioBufferRef, Signal, SignalSpec,
};
use symphonia::core::codecs::{
    CodecDescriptor, CodecParameters, Decoder, DecoderOptions, FinalizeResult, CODEC_TYPE_OPUS,
};
use symphonia::core::errors::{unsupported_error, Result};
use symphonia::core::formats::Packet;

/// Maximum Opus frame size: 120ms at 48kHz = 5760 samples per channel.
const MAX_OPUS_FRAME_SIZE: usize = 5760;

/// Wrapper around audiopus::coder::Decoder that implements Send + Sync.
///
/// Safety: libopus decoders are safe to send between threads and use from
/// one thread at a time. Symphonia's Decoder trait requires Send + Sync but
/// the audiopus wrapper doesn't mark it as such because of the raw pointer.
struct OpusDecoderInner(audiopus::coder::Decoder);

// Safety: libopus decoder state is self-contained. Symphonia guarantees
// only one thread calls decode() at a time.
unsafe impl Send for OpusDecoderInner {}
unsafe impl Sync for OpusDecoderInner {}

pub struct OpusDecoder {
    inner: OpusDecoderInner,
    params: CodecParameters,
    sample_rate: u32,
    channels: usize,
    buf: AudioBuffer<f32>,
    decode_buf: Vec<f32>,
}

impl OpusDecoder {
    fn opus_rate(sample_rate: u32) -> audiopus::SampleRate {
        match sample_rate {
            8000 => audiopus::SampleRate::Hz8000,
            12000 => audiopus::SampleRate::Hz12000,
            16000 => audiopus::SampleRate::Hz16000,
            24000 => audiopus::SampleRate::Hz24000,
            _ => audiopus::SampleRate::Hz48000,
        }
    }

    fn opus_channels(channels: usize) -> audiopus::Channels {
        if channels >= 2 {
            audiopus::Channels::Stereo
        } else {
            audiopus::Channels::Mono
        }
    }
}

impl Decoder for OpusDecoder {
    fn try_new(params: &CodecParameters, _options: &DecoderOptions) -> Result<Self>
    where
        Self: Sized,
    {
        if params.codec != CODEC_TYPE_OPUS {
            return unsupported_error("opus: invalid codec type");
        }

        let sample_rate = params.sample_rate.unwrap_or(48000);
        let channels = params.channels.map(|c| c.count()).unwrap_or(2);

        let opus_rate = Self::opus_rate(sample_rate);
        let opus_channels = Self::opus_channels(channels);

        let decoder = audiopus::coder::Decoder::new(opus_rate, opus_channels).map_err(|e| {
            symphonia::core::errors::Error::DecodeError(&*Box::leak(
                format!("opus: failed to create decoder: {e}").into_boxed_str(),
            ))
        })?;

        let actual_rate = match opus_rate {
            audiopus::SampleRate::Hz8000 => 8000,
            audiopus::SampleRate::Hz12000 => 12000,
            audiopus::SampleRate::Hz16000 => 16000,
            audiopus::SampleRate::Hz24000 => 24000,
            audiopus::SampleRate::Hz48000 => 48000,
        };

        let ch_layout = if channels >= 2 {
            symphonia::core::audio::Channels::FRONT_LEFT
                | symphonia::core::audio::Channels::FRONT_RIGHT
        } else {
            symphonia::core::audio::Channels::FRONT_CENTRE
        };

        let spec = SignalSpec::new(actual_rate, ch_layout);
        let buf = AudioBuffer::new(MAX_OPUS_FRAME_SIZE as u64, spec);
        let decode_buf = vec![0.0f32; MAX_OPUS_FRAME_SIZE * channels];

        log::info!("[opus] decoder created: {}Hz, {} channel(s)", actual_rate, channels);

        Ok(Self {
            inner: OpusDecoderInner(decoder),
            params: params.clone(),
            sample_rate: actual_rate,
            channels,
            buf,
            decode_buf,
        })
    }

    fn supported_codecs() -> &'static [CodecDescriptor]
    where
        Self: Sized,
    {
        &[CodecDescriptor {
            codec: CODEC_TYPE_OPUS,
            short_name: "opus",
            long_name: "Opus (via libopus)",
            inst_func: |params, opt| Ok(Box::new(OpusDecoder::try_new(params, opt)?)),
        }]
    }

    fn reset(&mut self) {
        if let Ok(d) =
            audiopus::coder::Decoder::new(
                Self::opus_rate(self.sample_rate),
                Self::opus_channels(self.channels),
            )
        {
            self.inner = OpusDecoderInner(d);
        }
    }

    fn codec_params(&self) -> &CodecParameters {
        &self.params
    }

    fn decode(&mut self, packet: &Packet) -> Result<AudioBufferRef<'_>> {
        self.buf.clear();

        let data = packet.buf();

        // Decode the Opus packet to interleaved f32 samples.
        // audiopus returns the number of decoded frames (samples per channel).
        let decoded_frames = self
            .inner
            .0
            .decode_float(Some(data), &mut self.decode_buf[..], false)
            .map_err(|e| {
                symphonia::core::errors::Error::DecodeError(&*Box::leak(
                    format!("opus: decode error: {e}").into_boxed_str(),
                ))
            })?;

        log::trace!(
            "[opus] decoded {} frames from {} byte packet, buf capacity={}",
            decoded_frames, data.len(), self.buf.capacity(),
        );

        if decoded_frames == 0 {
            return Ok(self.buf.as_audio_buffer_ref());
        }

        // Write interleaved f32 samples into Symphonia's planar AudioBuffer.
        // Use render_reserved() to mark the frames as written, then copy
        // directly into the planar layout. This is much more efficient than
        // using render()'s per-frame callback for bulk copies.
        let ch_count = self.channels;

        self.buf.render_reserved(Some(decoded_frames));
        {
            let mut planes = self.buf.planes_mut();
            let plane_slices = planes.planes();
            for ch in 0..ch_count {
                for i in 0..decoded_frames {
                    plane_slices[ch][i] = self.decode_buf[i * ch_count + ch];
                }
            }
        }

        Ok(self.buf.as_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        Default::default()
    }

    fn last_decoded(&self) -> AudioBufferRef<'_> {
        self.buf.as_audio_buffer_ref()
    }
}
