use std::thread;

use dasp::sample::Sample as DaspSample;
use symphonia::core::audio::conv::ConvertibleSample;
use symphonia::core::audio::sample::Sample;

use crate::DEFAULT_SAMPLE_RATE;
use crate::decoder::{
    Decoder, DecoderError, DecoderResult, DecoderSettings, ResampledDecoder, ResamplerSettings,
    Source,
};
use crate::output::{
    AudioBackend, AudioOutput, AudioOutputError, DecalSample, OutputBuilder, RequestedOutputConfig,
    SupportedStreamConfig, WriteBlockingError,
};

#[derive(thiserror::Error, Debug)]
pub enum WriteOutputError {
    #[error(transparent)]
    DecoderError(#[from] DecoderError),
    #[error(transparent)]
    WriteBlockingError(#[from] WriteBlockingError),
}

#[derive(thiserror::Error, Debug)]
pub enum ResetError {
    #[error(transparent)]
    AudioOutputError(#[from] AudioOutputError),
    #[error(transparent)]
    WriteBlockingError(#[from] WriteBlockingError),
    #[error(transparent)]
    DecoderError(#[from] DecoderError),
}

pub struct AudioManager<T: Sample + DaspSample, B: AudioBackend> {
    output_builder: OutputBuilder<B>,
    output_config: SupportedStreamConfig,
    output: AudioOutput<T, B>,
    resampled: ResampledDecoder<T>,
    device_name: Option<String>,
    resampler_settings: ResamplerSettings,
    volume: T::Float,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetMode {
    Force,
    Default,
}

impl<T, B> AudioManager<T, B>
where
    T: Sample + DecalSample + ConvertibleSample + rubato::Sample + Send,
    B: AudioBackend,
{
    pub fn new(
        output_builder: OutputBuilder<B>,
        resampler_settings: ResamplerSettings,
    ) -> Result<Self, ResetError> {
        let default_output_config = output_builder.default_output_config()?;
        let output_config = output_builder.find_closest_config(
            None,
            RequestedOutputConfig {
                sample_rate: Some(DEFAULT_SAMPLE_RATE),
                channels: Some(default_output_config.channels),
                sample_format: Some(<T as DecalSample>::FORMAT),
            },
        )?;

        let output: AudioOutput<T, B> =
            output_builder.new_output::<T>(None, output_config.clone())?;

        let resampled = ResampledDecoder::<T>::new(
            output_config.sample_rate,
            output_config.channels,
            resampler_settings.clone(),
        );

        Ok(Self {
            output_config,
            output_builder,
            output,
            resampled,
            device_name: None,
            resampler_settings,
            volume: 1.0.to_sample(),
        })
    }

    pub fn resampler_settings(&self) -> &ResamplerSettings {
        &self.resampler_settings
    }

    pub fn set_resampler_settings(&mut self, settings: ResamplerSettings) {
        self.resampler_settings = settings;
    }

    pub fn set_device(&mut self, device: Option<String>) {
        self.device_name = device;
    }

    pub fn set_volume(&mut self, volume: T::Float) {
        self.volume = volume;
    }

    pub fn init_decoder(
        &mut self,
        source: Box<dyn Source>,
        decoder_settings: DecoderSettings,
    ) -> Result<Decoder<T>, ResetError> {
        let mut decoder = Decoder::<T>::new(
            source,
            self.volume,
            self.output_config.channels,
            decoder_settings,
        )?;
        self.reset(&mut decoder, ResetMode::Default)?;
        Ok(decoder)
    }

    fn rebuild_output(&mut self) -> Result<(), AudioOutputError> {
        self.output = self
            .output_builder
            .new_output(self.device_name.clone(), self.output_config.clone())?;
        Ok(())
    }

    pub fn reset_output(&mut self) -> Result<(), ResetError> {
        let new_output_config = self.output_builder.find_closest_config(
            self.device_name.as_deref(),
            RequestedOutputConfig {
                sample_rate: Some(self.output_config.sample_rate),
                channels: Some(self.output_config.channels),
                sample_format: Some(<T as DecalSample>::FORMAT),
            },
        )?;
        self.output_config = new_output_config;
        self.rebuild_output()?;
        Ok(())
    }

    pub fn reset(&mut self, decoder: &mut Decoder<T>, mode: ResetMode) -> Result<(), ResetError> {
        let new_output_config = self.output_builder.find_closest_config(
            self.device_name.as_deref(),
            RequestedOutputConfig {
                sample_rate: Some(decoder.sample_rate()),
                channels: Some(self.output_config.channels),
                sample_format: Some(<T as DecalSample>::FORMAT),
            },
        )?;

        let force_reset = mode == ResetMode::Force;
        let output_config_changed = new_output_config != self.output_config || force_reset;
        let in_sample_rate_changed =
            self.resampled.in_sample_rate() != decoder.sample_rate() || force_reset;
        let resampler_config_changed = new_output_config.sample_rate
            != self.output_config.sample_rate
            || new_output_config.channels != self.output_config.channels
            || force_reset;
        self.output_config = new_output_config;

        // No changes needed, just make sure the output is running
        if !output_config_changed && !in_sample_rate_changed {
            self.resampled.initialize(decoder)?;
            self.output.start()?;
            return Ok(());
        }

        if output_config_changed {
            self.rebuild_output()?;
        }
        self.flush()?;

        // Recreate decoder if the relevant output configuration changed
        if resampler_config_changed {
            self.resampled = ResampledDecoder::new(
                self.output_config.sample_rate,
                self.output_config.channels,
                self.resampler_settings.clone(),
            );
        }
        self.resampled.initialize(decoder)?;

        // Pre-fill output buffer before starting the stream
        while self.resampled.current(decoder).len() <= self.output.buffer_space_available() {
            self.output.write(self.resampled.current(decoder)).unwrap();
            if self.resampled.decode_next_frame(decoder)? == DecoderResult::Finished {
                break;
            }
        }

        self.output.start()?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), WriteBlockingError> {
        let res = self.flush_output();
        if res.is_ok() {
            thread::sleep(self.output.settings().buffer_duration);
        }
        self.output.stop();
        res
    }

    pub fn write(&mut self, decoder: &mut Decoder<T>) -> Result<DecoderResult, WriteOutputError> {
        self.output
            .write_blocking(self.resampled.current(decoder))?;
        let decoder_result = self.resampled.decode_next_frame(decoder)?;
        Ok(decoder_result)
    }

    pub fn write_all(&mut self, decoder: &mut Decoder<T>) -> Result<(), WriteOutputError> {
        loop {
            if self.write(decoder)? == DecoderResult::Finished {
                self.flush()?;
                return Ok(());
            }
        }
    }

    fn flush_output(&mut self) -> Result<(), WriteBlockingError> {
        self.output.write_blocking(self.resampled.flush())
    }
}
