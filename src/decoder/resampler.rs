use audioadapter_buffers::direct::InterleavedSlice;
use dasp::sample::Sample as DaspSample;
use rubato::{Fft, FixedSync, Indexing, Resampler};
use symphonia::core::audio::conv::ConvertibleSample;
use symphonia::core::audio::sample::Sample;

use super::{Decoder, DecoderError};
use crate::decoder::fixed_buffer::FixedBuffer;
use crate::{ChannelCount, SampleRate};

struct ResampleDecoderInner<T: Sample + DaspSample> {
    channels: usize,
    resampler: Fft<T>,
    in_buf: FixedBuffer<T>,
    out_buf: Vec<T>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecoderResult {
    Finished,
    Unfinished,
}

impl<T: Sample + DaspSample + ConvertibleSample + rubato::Sample> ResampleDecoderInner<T> {
    fn next(&mut self, decoder: &mut Decoder<T>) -> Result<DecoderResult, DecoderError> {
        let input_samples_left = self.resampler.input_frames_next() * self.channels;
        while self.in_buf.position() < input_samples_left {
            let to_write = input_samples_left.min(self.in_buf.remaining());
            let current_slice = decoder.current(Some(to_write));
            if current_slice.is_empty() {
                if decoder.next()? == DecoderResult::Finished {
                    return Ok(DecoderResult::Finished);
                } else {
                    continue;
                }
            }
            self.in_buf.append_from_slice(current_slice);
        }

        let (input_adapter, mut output_adapter) = create_adapters(
            &self.in_buf,
            &mut self.out_buf,
            &self.resampler,
            self.channels,
        );
        self.resampler
            .process_into_buffer(&input_adapter, &mut output_adapter, None)
            .expect("number of frames was not correctly calculated");
        self.in_buf.reset();
        Ok(DecoderResult::Unfinished)
    }

    fn current(&self) -> &[T] {
        &self.out_buf
    }

    fn flush(&mut self) -> &[T] {
        let in_position = self.in_buf.position();
        if in_position == 0 {
            return &[];
        }

        let (input_adapter, mut output_adapter) = create_adapters(
            &self.in_buf,
            &mut self.out_buf,
            &self.resampler,
            self.channels,
        );

        let partial_len = in_position / self.channels;
        let (_, n_out) = self
            .resampler
            .process_into_buffer(
                &input_adapter,
                &mut output_adapter,
                Some(&Indexing {
                    input_offset: 0,
                    output_offset: 0,
                    partial_len: Some(partial_len),
                    active_channels_mask: None,
                }),
            )
            .expect("number of frames was not correctly calculated");
        self.in_buf.reset();
        &self.out_buf[..n_out * self.channels]
    }
}

fn create_adapters<'a, T>(
    in_buf: &'a FixedBuffer<T>,
    out_buf: &'a mut [T],
    resampler: &Fft<T>,
    channels: usize,
) -> (InterleavedSlice<&'a [T]>, InterleavedSlice<&'a mut [T]>)
where
    T: rubato::Sample,
{
    let input_adapter =
        InterleavedSlice::new(in_buf.inner(), channels, resampler.input_frames_next())
            .expect("number of frames was not correctly calculated");

    let output_adapter =
        InterleavedSlice::new_mut(out_buf, channels, resampler.output_frames_next())
            .expect("number of frames was not correctly calculated");
    (input_adapter, output_adapter)
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
    in_sample_rate: SampleRate,
    out_sample_rate: SampleRate,
    channels: ChannelCount,
    settings: ResamplerSettings,
}

impl<T: Sample + DaspSample + ConvertibleSample + rubato::Sample> ResampledDecoder<T> {
    pub fn new(
        out_sample_rate: SampleRate,
        channels: ChannelCount,
        settings: ResamplerSettings,
    ) -> Self {
        Self {
            decoder_inner: ResampledDecoderImpl::Native,
            in_sample_rate: out_sample_rate,
            out_sample_rate,
            channels,
            settings,
        }
    }

    pub fn initialize(&mut self, decoder: &mut Decoder<T>) -> Result<(), DecoderError> {
        let sample_rate_changed = self.in_sample_rate != decoder.sample_rate;
        self.in_sample_rate = decoder.sample_rate;
        match &mut self.decoder_inner {
            ResampledDecoderImpl::Native => {
                if self.in_sample_rate != self.out_sample_rate {
                    self.initialize_resampler();
                    self.decode_next_frame(decoder)?;
                }
            }
            ResampledDecoderImpl::Resampled(_) => {
                if sample_rate_changed {
                    self.initialize_resampler();
                }
                // Ensure current() always returns the current set of samples
                self.decode_next_frame(decoder)?;
            }
        }
        Ok(())
    }

    fn initialize_resampler(&mut self) {
        let resampler = Fft::<T>::new(
            self.in_sample_rate.0 as usize,
            self.out_sample_rate.0 as usize,
            self.settings.chunk_size,
            1,
            self.channels.0 as usize,
            FixedSync::Both,
        )
        .expect("failed to create resampler");

        let n_frames_in = resampler.input_frames_max();
        let n_frames_out = resampler.output_frames_max();

        let resampler = ResampledDecoderImpl::Resampled(ResampleDecoderInner {
            channels: self.channels.0 as usize,
            in_buf: FixedBuffer::new(T::MID, n_frames_in * self.channels.0 as usize),
            out_buf: vec![T::MID; n_frames_out * self.channels.0 as usize],
            resampler,
        });
        self.decoder_inner = resampler;
    }

    pub fn in_sample_rate(&self) -> SampleRate {
        self.in_sample_rate
    }

    pub fn out_sample_rate(&self) -> SampleRate {
        self.out_sample_rate
    }

    pub fn current<'a>(&'a self, decoder: &'a mut Decoder<T>) -> &'a [T] {
        match &self.decoder_inner {
            ResampledDecoderImpl::Resampled(decoder_inner) => decoder_inner.current(),
            ResampledDecoderImpl::Native => decoder.current(None),
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
            ResampledDecoderImpl::Native => Ok(decoder.next()?),
        }
    }
}
