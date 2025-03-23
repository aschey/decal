use cpal::{SampleRate, SizedSample, SupportedStreamConfig};
use dasp::sample::Sample as DaspSample;
use symphonia::core::audio::conv::ConvertibleSample;
use symphonia::core::audio::sample::Sample;

use crate::decoder::{
    Decoder, DecoderError, DecoderResult, DecoderSettings, ResampledDecoder, ResamplerSettings,
    Source,
};
use crate::output::{
    AudioBackend, AudioOutput, AudioOutputError, OutputBuilder, RequestedOutputConfig,
    WriteBlockingError,
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

impl<
    T: Sample + DaspSample + SizedSample + ConvertibleSample + rubato::Sample + Send,
    B: AudioBackend,
> AudioManager<T, B>
{
    pub fn new(
        output_builder: OutputBuilder<B>,
        resampler_settings: ResamplerSettings,
    ) -> Result<Self, AudioOutputError> {
        let output_config = output_builder.default_output_config()?;

        let output: AudioOutput<T, B> =
            output_builder.new_output::<T>(None, output_config.clone())?;

        let resampled = ResampledDecoder::<T>::new(
            output_config.sample_rate().0 as usize,
            output_config.channels() as usize,
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
        &self,
        source: Box<dyn Source>,
        decoder_settings: DecoderSettings,
    ) -> Result<Decoder<T>, DecoderError> {
        Decoder::<T>::new(
            source,
            self.volume,
            self.output_config.channels() as usize,
            decoder_settings,
        )
    }

    pub fn initialize(&mut self, decoder: &mut Decoder<T>) -> Result<(), WriteBlockingError> {
        let res = if decoder.sample_rate() != self.resampled.in_sample_rate() {
            self.flush_output()
        } else {
            Ok(())
        };
        self.resampled.initialize(decoder);
        res
    }

    pub fn reset(&mut self, decoder: &mut Decoder<T>) -> Result<(), ResetError> {
        self.flush()?;
        self.output_config = self.output_builder.find_closest_config(
            self.device_name.as_deref(),
            RequestedOutputConfig {
                sample_rate: Some(SampleRate(decoder.sample_rate() as u32)),
                channels: Some(self.output_config.channels()),
                sample_format: Some(<T as cpal::SizedSample>::FORMAT),
            },
        )?;

        self.output = self
            .output_builder
            .new_output(None, self.output_config.clone())?;

        self.resampled = ResampledDecoder::new(
            self.output_config.sample_rate().0 as usize,
            self.output_config.channels() as usize,
            self.resampler_settings.clone(),
        );

        self.resampled.initialize(decoder);

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
        std::thread::sleep(self.output.settings().buffer_duration);
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
