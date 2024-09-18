use once_cell::sync::Lazy;
use symphonia_core::audio::{AudioBuffer, AudioBufferRef};
use symphonia_core::codecs::{CodecDescriptor, CodecParameters, Decoder, DecoderOptions, FinalizeResult, CODEC_TYPE_OPUS};
use symphonia_core::formats::Packet;
use crate::{celt, silk};

/// Opus codec descriptor 
/// Codecs register themselves using CodecDescriptor.
static OPUS_CODEC_DESCRIPTOR: Lazy<CodecDescriptor> = Lazy::new(|| {
    CodecDescriptor {
        codec: CODEC_TYPE_OPUS,
        short_name: "opus",
        long_name: "Opus Audio Codec",
        inst_func: |params: &CodecParameters, options: &DecoderOptions| -> symphonia_core::errors::Result<Box<dyn Decoder>> {
            Ok(Box::new(OpusDecoder::try_new(params, options)?))
        },
    }
});

/// Register the Opus decoder with Symphonia
pub fn get_codecs() -> &'static [CodecDescriptor] {
    return std::slice::from_ref(&*OPUS_CODEC_DESCRIPTOR);
}

// Opus-specific constants
const OPUS_FRAME_SIZES: [usize; 5] = [120, 240, 480, 960, 1920];
const MAX_FRAME_SIZE_MS: usize = 60;
const MAX_PACKET_DURATION_MS: usize = 120;
const SILK_INTERNAL_SAMPLE_RATE: u32 = 16000;
const CELT_INTERNAL_SAMPLE_RATE: u32 = 48000;


#[derive(Debug, Clone, Copy)]
enum Mode {
    Silk,
    Celt,
    Hybrid,
}

#[derive(Debug, Clone, Copy)]
enum Bandwidth {
    NarrowBand,
    MediumBand,
    WideBand,
    SuperWideBand,
    FullBand,
}

struct Frame {
    mode: Mode,
    bandwidth: Bandwidth,
    frame_size: usize,
    data: Vec<u8>,
}

pub struct OpusDecoder {
    params: CodecParameters,
    // TODO: extend if needed according to https://datatracker.ietf.org/doc/html/rfc6716
    buf: AudioBuffer<f32>,
    silk_decoder: Option<silk::Decoder>,
    celt_decoder: Option<celt::Decoder>,
    // range_decoder ?
}


impl Decoder for OpusDecoder {
    fn try_new(_params: &CodecParameters, _options: &DecoderOptions) -> symphonia_core::errors::Result<Self>
    where
        Self: Sized,
    {
        unimplemented!()
    }

    fn supported_codecs() -> &'static [CodecDescriptor]
    where
        Self: Sized,
    {
        unimplemented!()
    }

    fn reset(&mut self) {
        unimplemented!()
    }

    fn codec_params(&self) -> &CodecParameters {
        unimplemented!()
    }

    fn decode(&mut self, _packet: &Packet) -> symphonia_core::errors::Result<AudioBufferRef> {
        unimplemented!()
    }

    fn finalize(&mut self) -> FinalizeResult {
        unimplemented!()
    }

    fn last_decoded(&self) -> AudioBufferRef {
        unimplemented!()
    }
}