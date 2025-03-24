use dasp::sample::Sample as DaspSample;
use rubato::{FftFixedInOut, Resampler};
use symphonia::core::audio::conv::ConvertibleSample;
use symphonia::core::audio::sample::Sample;

use super::channel_buffer::ChannelBuffer;
use super::vec_ext::VecExt;
use super::{Decoder, DecoderError};

struct ResampleDecoderInner<T: Sample + DaspSample> {
    written: usize,
    in_buf: ChannelBuffer<T>,
    resampler: FftFixedInOut<T>,
    resampler_buf: Vec<Vec<T>>,
    out_buf: Vec<T>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecoderResult {
    Finished,
    Unfinished,
}

impl<T: Sample + DaspSample + ConvertibleSample + rubato::Sample> ResampleDecoderInner<T> {
    fn next(&mut self, decoder: &mut Decoder<T>) -> Result<DecoderResult, DecoderError> {
        let mut cur_frame = decoder.current();

        while !self.in_buf.is_full() {
            self.written += self.in_buf.fill_from_slice(&cur_frame[self.written..]);

            if self.written == cur_frame.len() {
                match decoder.next()? {
                    Some(next) => {
                        cur_frame = next;
                        self.written = 0;
                    }
                    None => {
                        return Ok(DecoderResult::Finished);
                    }
                }
            }
        }

        self.resampler
            .process_into_buffer(self.in_buf.inner(), &mut self.resampler_buf, None)
            .expect("number of frames was not correctly calculated");
        self.in_buf.reset();

        self.out_buf.fill_from_deinterleaved(&self.resampler_buf);
        Ok(DecoderResult::Unfinished)
    }

    fn current(&self) -> &[T] {
        &self.out_buf
    }

    fn flush(&mut self) -> &[T] {
        if self.in_buf.position() > 0 {
            self.in_buf.silence_remainder();
            self.resampler
                .process_into_buffer(self.in_buf.inner(), &mut self.resampler_buf, None)
                .expect("number of frames was not correctly calculated");
            self.in_buf.reset();
            &self.out_buf
        } else {
            &[]
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum ResampledDecoderImpl<T: Sample + DaspSample> {
    Resampled(ResampleDecoderInner<T>),
    Native,
}

#[derive(Clone, Debug)]
pub struct ResamplerSettings {
    pub chunk_size: usize,
}

impl Default for ResamplerSettings {
    fn default() -> Self {
        Self { chunk_size: 1024 }
    }
}

pub struct ResampledDecoder<T: Sample + DaspSample> {
    decoder_inner: ResampledDecoderImpl<T>,
    in_sample_rate: usize,
    out_sample_rate: usize,
    channels: usize,
    settings: ResamplerSettings,
}

impl<T: Sample + DaspSample + ConvertibleSample + rubato::Sample> ResampledDecoder<T> {
    pub fn new(out_sample_rate: usize, channels: usize, settings: ResamplerSettings) -> Self {
        Self {
            decoder_inner: ResampledDecoderImpl::Native,
            in_sample_rate: out_sample_rate,
            out_sample_rate,
            channels,
            settings,
        }
    }

    pub fn initialize(&mut self, decoder: &mut Decoder<T>) {
        let current_in_rate = self.in_sample_rate;
        self.in_sample_rate = decoder.sample_rate();
        match &mut self.decoder_inner {
            ResampledDecoderImpl::Native => {
                self.initialize_resampler(decoder);
            }
            ResampledDecoderImpl::Resampled(inner) => {
                if self.in_sample_rate != self.out_sample_rate
                    && self.in_sample_rate == current_in_rate
                {
                    inner.written = 0;
                } else if self.in_sample_rate == self.out_sample_rate {
                    self.decoder_inner = ResampledDecoderImpl::Native;
                } else {
                    self.initialize_resampler(decoder);
                }
            }
        }
    }

    fn initialize_resampler(&mut self, decoder: &mut Decoder<T>) {
        let resampler = FftFixedInOut::<T>::new(
            self.in_sample_rate,
            self.out_sample_rate,
            self.settings.chunk_size,
            self.channels,
        )
        .expect("failed to create resampler");

        let in_buf = resampler.input_buffer_allocate(true);
        let resampler_buf = resampler.output_buffer_allocate(true);
        let n_frames = resampler.input_frames_next();

        let resampler = ResampledDecoderImpl::Resampled(ResampleDecoderInner {
            written: 0,
            resampler_buf,
            out_buf: Vec::with_capacity(n_frames * self.channels),
            in_buf: ChannelBuffer::new(in_buf),
            resampler,
        });
        self.decoder_inner = resampler;
        self.decode_next_frame(decoder).unwrap();
    }

    pub fn in_sample_rate(&self) -> usize {
        self.in_sample_rate
    }

    pub fn out_sample_rate(&self) -> usize {
        self.out_sample_rate
    }

    pub fn current<'a>(&'a self, decoder: &'a Decoder<T>) -> &'a [T] {
        match &self.decoder_inner {
            ResampledDecoderImpl::Resampled(decoder_inner) => decoder_inner.current(),
            ResampledDecoderImpl::Native => decoder.current(),
        }
    }

    pub fn flush(&mut self) -> &[T] {
        match &mut self.decoder_inner {
            ResampledDecoderImpl::Resampled(decoder) => decoder.flush(),
            ResampledDecoderImpl::Native => &[],
        }
    }

    pub fn decode_next_frame<'a>(
        &'a mut self,
        decoder: &'a mut Decoder<T>,
    ) -> Result<DecoderResult, DecoderError> {
        match &mut self.decoder_inner {
            ResampledDecoderImpl::Resampled(decoder_inner) => decoder_inner.next(decoder),
            ResampledDecoderImpl::Native => Ok(match decoder.next()? {
                Some(_) => DecoderResult::Unfinished,
                None => DecoderResult::Finished,
            }),
        }
    }
}
