use cpal::{SampleRate, SizedSample, SupportedStreamConfig};
use dasp::sample::Sample as DaspSample;
use decoder::{Decoder, DecoderError, DecoderResult, ResampledDecoder, Source};
use output::{AudioOutput, OutputBuilder, RequestedOutputConfig};
use symphonia::core::{conv::ConvertibleSample, sample::Sample};

pub mod decoder;
pub mod output;

pub struct AudioManager<T: Sample + DaspSample> {
    output_builder: OutputBuilder,
    output_config: SupportedStreamConfig,
    output: AudioOutput<T>,
    resampled: ResampledDecoder<T>,
}

impl<T: Sample + DaspSample + SizedSample + ConvertibleSample + rubato::Sample + Send>
    AudioManager<T>
{
    pub fn new(output_builder: OutputBuilder) -> Self {
        let output_config = output_builder.default_output_config().unwrap();

        let output: AudioOutput<T> = output_builder
            .new_output::<T>(None, output_config.clone())
            .unwrap();

        let resampled = ResampledDecoder::<T>::new(
            output_config.sample_rate().0 as usize,
            output_config.channels() as usize,
        );

        Self {
            output_config,
            output_builder,
            output,
            resampled,
        }
    }

    pub fn init_decoder(&self, source: Box<dyn Source>) -> Decoder<T> {
        Decoder::<T>::new(
            source,
            1.0.to_sample(),
            self.output_config.channels() as usize,
            None,
        )
        .unwrap()
    }

    pub fn initialize(&mut self, decoder: &mut Decoder<T>) {
        if decoder.sample_rate() != self.resampled.in_sample_rate() {
            self.flush_output();
        }
        self.resampled.initialize(decoder);
    }

    pub fn reset(&mut self, decoder: &mut Decoder<T>) {
        self.flush();
        self.output_config = self
            .output_builder
            .find_closest_config(
                None,
                RequestedOutputConfig {
                    sample_rate: Some(SampleRate(decoder.sample_rate() as u32)),
                    channels: Some(self.output_config.channels()),
                    sample_format: Some(<T as cpal::SizedSample>::FORMAT),
                },
            )
            .unwrap();

        self.output = self
            .output_builder
            .new_output(None, self.output_config.clone())
            .unwrap();

        self.resampled = ResampledDecoder::new(
            self.output_config.sample_rate().0 as usize,
            self.output_config.channels() as usize,
        );

        self.resampled.initialize(decoder);

        // Pre-fill output buffer before starting the stream
        while self.resampled.current(decoder).len() <= self.output.buffer_space_available() {
            self.output.write(self.resampled.current(decoder)).unwrap();
            if self.resampled.decode_next_frame(decoder).unwrap() == DecoderResult::Finished {
                break;
            }
        }

        self.output.start().unwrap();
    }

    pub fn flush(&mut self) {
        self.flush_output();
        std::thread::sleep(self.output.buffer_duration());
        self.output.stop();
    }

    pub fn write(&mut self, decoder: &mut Decoder<T>) -> Result<DecoderResult, DecoderError> {
        self.output.write_blocking(self.resampled.current(decoder));
        self.resampled.decode_next_frame(decoder)
    }

    fn flush_output(&mut self) {
        self.output.write_blocking(self.resampled.flush());
    }
}
