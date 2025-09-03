use std::error::Error;
use std::path::Path;
use std::time::Duration;

use decal::decoder::{
    Decoder, DecoderResult, DecoderSettings, ReadSeekSource, ResampledDecoder, ResamplerSettings,
};
use decal::output::{
    AudioOutput, CpalOutput, OutputBuilder, OutputSettings, RequestedOutputConfig, SampleFormat,
    SampleRate,
};
use tap::TapFallible;
use tracing::error;

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_line_number(true)
        .with_file(true)
        .init();

    let (reset_tx, reset_rx) = std::sync::mpsc::sync_channel(32);
    let output_builder = OutputBuilder::new(
        CpalOutput::default(),
        OutputSettings::default(),
        move || {
            reset_tx
                .send(())
                .tap_err(|e| error!("Error sending reset signal: {e}"))
                .ok();
        },
        |err| error!("Output error: {err}"),
    );
    let mut output_config = output_builder.default_output_config()?;

    let mut output: AudioOutput<f32, _> =
        output_builder.new_output::<f32>(None, output_config.clone())?;
    let queue = vec!["examples/music-1.mp3", "examples/music-2.mp3"];

    let mut resampled = ResampledDecoder::new(
        output_config.sample_rate.0 as usize,
        output_config.channels as usize,
        ResamplerSettings::default(),
    );
    let mut initialized = false;
    let mut current_position = Duration::default();
    let mut seek_position: Option<Duration> = None;

    for file_name in queue.into_iter() {
        loop {
            let source = Box::new(ReadSeekSource::from_path(Path::new(file_name))?);

            let mut decoder = Decoder::<f32>::new(
                source,
                1.0,
                output_config.channels as usize,
                DecoderSettings {
                    enable_gapless: true,
                },
            )?;
            if let Some(seek_position) = seek_position.take() {
                decoder.seek(seek_position).unwrap();
            }

            if !initialized {
                initialized = true;

                output_config = output_builder.find_closest_config(
                    None,
                    RequestedOutputConfig {
                        sample_rate: Some(SampleRate(decoder.sample_rate() as u32)),
                        channels: Some(output_config.channels),
                        sample_format: Some(SampleFormat::F32),
                    },
                )?;

                output = output_builder.new_output(None, output_config.clone())?;

                resampled = ResampledDecoder::new(
                    output_config.sample_rate.0 as usize,
                    output_config.channels as usize,
                    ResamplerSettings::default(),
                );

                resampled.initialize(&mut decoder);

                // Pre-fill output buffer before starting the stream
                while resampled.current(&decoder).len() <= output.buffer_space_available() {
                    output.write(resampled.current(&decoder)).unwrap();
                    if resampled.decode_next_frame(&mut decoder)? == DecoderResult::Finished {
                        break;
                    }
                }

                output.start()?;
            } else {
                if decoder.sample_rate() != resampled.in_sample_rate() {
                    output.write_blocking(resampled.flush()).ok();
                }
                resampled.initialize(&mut decoder);
            }

            let go_next = loop {
                if reset_rx.try_recv().is_ok() {
                    seek_position = Some(current_position);
                    initialized = false;
                    break false;
                }

                output.write_blocking(resampled.current(&decoder)).ok();
                match resampled.decode_next_frame(&mut decoder) {
                    Ok(DecoderResult::Finished) => break true,
                    Ok(DecoderResult::Unfinished) => {}
                    Err(e) => {
                        Err(e)?;
                    }
                }
                current_position = decoder.current_position().position;
            };

            if go_next {
                break;
            }
        }
    }
    // Write out any remaining data
    output.write_blocking(resampled.flush()).ok();
    // Wait for all data to reach the audio device
    std::thread::sleep(output.settings().buffer_duration);
    Ok(())
}
