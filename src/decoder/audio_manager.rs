use crate::decoder::DecoderParams;
use crate::output::{AudioOutput, AudioOutputError, OutputBuilder, RequestedOutputConfig};
use cpal::SupportedStreamConfig;
use rubato::{FftFixedInOut, Resampler};
use std::time::Duration;
use tracing::info;

use super::DecoderError;
use super::{channel_buffer::ChannelBuffer, source::Source, vec_ext::VecExt, Decoder};

enum DecoderMode {
    Resample,
    NoResample,
}

pub struct AudioManager {
    in_buf: ChannelBuffer,
    out_buf: Vec<f32>,
    input_sample_rate: usize,
    output_builder: OutputBuilder,
    device_name: Option<String>,
    output: AudioOutput<f32>,
    resampler: FftFixedInOut<f32>,
    resampler_buf: Vec<Vec<f32>>,
    volume: f32,
    output_config: SupportedStreamConfig,
    decoder_mode: DecoderMode,
    written: usize,
}

impl AudioManager {
    pub fn new(output_builder: OutputBuilder, volume: f32) -> Result<Self, AudioOutputError> {
        let output_config = output_builder.default_output_config()?;
        let sample_rate = output_config.sample_rate().0 as usize;
        let channels = output_config.channels() as usize;

        let resampler = FftFixedInOut::<f32>::new(
            sample_rate,
            sample_rate,
            1024,
            output_config.channels() as usize,
        )
        .expect("failed to create resampler");

        let resampler_buf = resampler.input_buffer_allocate();
        let n_frames = resampler.input_frames_next();
        let output = output_builder.new_output(None, output_config.clone())?;

        Ok(Self {
            output_builder,
            in_buf: ChannelBuffer::new(n_frames, channels),
            out_buf: Vec::with_capacity(n_frames * channels),
            input_sample_rate: sample_rate,
            resampler,
            resampler_buf,
            output,
            volume,
            output_config,
            device_name: None,
            decoder_mode: DecoderMode::NoResample,
            written: 0,
        })
    }

    fn set_resampler(&mut self, resample_chunk_size: usize) {
        self.resampler = FftFixedInOut::<f32>::new(
            self.input_sample_rate,
            self.output_config.sample_rate().0 as usize,
            resample_chunk_size,
            self.output_config.channels() as usize,
        )
        .expect("failed to create resampler");
        let n_frames = self.resampler.input_frames_next();
        self.resampler_buf = self.resampler.input_buffer_allocate();

        let channels = self.output_config.channels() as usize;
        self.in_buf = ChannelBuffer::new(n_frames, channels);
        self.out_buf = Vec::with_capacity(n_frames * channels);
    }

    pub fn set_device_name(&mut self, device_name: Option<String>) {
        self.device_name = device_name;
    }

    pub fn reset(
        &mut self,
        output_config: RequestedOutputConfig,
        resample_chunk_size: usize,
    ) -> Result<(), AudioOutputError> {
        self.output_config = self
            .output_builder
            .find_closest_config(None, output_config)?;
        self.output = self
            .output_builder
            .new_output(None, self.output_config.clone())?;
        self.set_resampler(resample_chunk_size);
        self.start()?;

        Ok(())
    }

    pub fn start(&mut self) -> Result<(), AudioOutputError> {
        self.output.start()
    }

    pub fn stop(&mut self) {
        self.output.stop();
    }

    pub fn play_remaining(&mut self) {
        if self.in_buf.position() > 0 {
            self.in_buf.silence_remainder();
            self.write_output();
        }
    }

    fn write_output(&mut self) {
        // This shouldn't panic as long as we calculated the number of channels and frames correctly
        self.resampler
            .process_into_buffer(self.in_buf.inner(), &mut self.resampler_buf, None)
            .expect("number of frames was not correctly calculated");
        self.in_buf.reset();

        self.out_buf.fill_from_deinterleaved(&self.resampler_buf);
        self.output.write(&self.out_buf);
    }

    pub fn initialize_decoder(
        &mut self,
        source: Box<dyn Source>,
        volume: Option<f32>,
        start_position: Option<Duration>,
    ) -> Result<Decoder<f32>, DecoderError> {
        Decoder::new(DecoderParams {
            source,
            volume: volume.unwrap_or(self.volume),
            output_channels: self.output_config.channels() as usize,
            start_position,
        })
    }

    pub fn decode_source(&mut self, decoder: &Decoder<f32>, resample_chunk_size: usize) {
        self.volume = decoder.volume();

        let input_sample_rate = decoder.sample_rate();
        if input_sample_rate != self.input_sample_rate {
            self.play_remaining();
            self.input_sample_rate = input_sample_rate;
            self.set_resampler(resample_chunk_size);
        }

        if decoder.sample_rate() != self.output_config.sample_rate().0 as usize {
            info!("Resampling source");
            self.decoder_mode = DecoderMode::Resample;
            self.written = 0;
            // self.decode_resample(decoder);
        } else {
            info!("Not resampling source");
            self.decoder_mode = DecoderMode::NoResample;
            self.output.write(decoder.current());
            // self.decode_no_resample(decoder);
        }
        // self.volume = decoder.volume();
        // info!("Finished decoding");
        // let stop_position = decoder.current_position();
        // info!("Stopped decoding at {stop_position:?}");

        // stop_position.position
    }

    pub fn decode_next(&mut self, decoder: &mut Decoder<f32>) -> Result<usize, DecoderError> {
        match self.decoder_mode {
            DecoderMode::Resample => self.decode_resample(decoder),
            DecoderMode::NoResample => self.decode_no_resample(decoder),
        }
    }

    fn decode_no_resample(&mut self, decoder: &mut Decoder<f32>) -> Result<usize, DecoderError> {
        // self.output.write(decoder.current());
        // loop {
        match decoder.next()? {
            Some(data) => {
                self.output.write(data);
                Ok(data.len())
            }
            None => Ok(0),
        }
        // }
    }

    fn decode_resample(&mut self, decoder: &mut Decoder<f32>) -> Result<usize, DecoderError> {
        let mut cur_frame = decoder.current();
        // let mut written = 0;

        // loop {
        while !self.in_buf.is_full() {
            self.written += self.in_buf.fill_from_slice(&cur_frame[self.written..]);

            if self.written == cur_frame.len() {
                match decoder.next()? {
                    Some(next) => {
                        cur_frame = next;
                        self.written = 0;
                    }
                    None => return Ok(0),
                    // Err(e) => {
                    //     error!("Error while decoding: {e:?}");
                    //     return;
                    // }
                }
            }
        }

        self.write_output();
        Ok(cur_frame.len())
        // }
    }
}
