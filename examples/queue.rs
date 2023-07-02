use std::{error::Error, fs::File, io::BufReader, path::Path, time::Duration};

use cpal::{SampleFormat, SampleRate};
use dcal::{
    decoder::{Decoder, DecoderError, ReadSeekSource, ResampledDecoder},
    output::{AudioOutput, OutputBuilder, RequestedOutputConfig},
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
        move || {
            reset_tx
                .send(())
                .tap_err(|e| error!("Error sending reset signal: {e}"))
                .ok();
        },
        |err| error!("Output error: {err}"),
    );
    let mut output_config = output_builder.default_output_config()?;

    let mut output: AudioOutput<f32> =
        output_builder.new_output::<f32>(None, output_config.clone())?;
    let queue = vec!["examples/music-1.mp3", "examples/music-2.mp3"];

    let mut resampled = ResampledDecoder::new(
        output_config.sample_rate().0 as usize,
        output_config.channels() as usize,
    );
    let mut initialized = false;
    let mut current_position = Duration::default();
    let mut seek_position: Option<Duration> = None;

    for file_name in queue.into_iter() {
        loop {
            let file = File::open(file_name)?;
            let file_len = file.metadata()?.len();

            let extension = Path::new(file_name)
                .extension()
                .unwrap()
                .to_string_lossy()
                .to_string();
            let reader = BufReader::new(file);
            let source = Box::new(ReadSeekSource::new(reader, Some(file_len), Some(extension)));

            let mut decoder =
                Decoder::<f32>::new(source, 1.0, output_config.channels() as usize, None)?;
            if let Some(seek_position) = seek_position.take() {
                decoder.seek(seek_position).unwrap();
            }

            if !initialized {
                initialized = true;

                output_config = output_builder.find_closest_config(
                    None,
                    RequestedOutputConfig {
                        sample_rate: Some(SampleRate(decoder.sample_rate() as u32)),
                        channels: Some(output_config.channels()),
                        sample_format: Some(SampleFormat::F32),
                    },
                )?;

                output = output_builder.new_output(None, output_config.clone())?;

                resampled = ResampledDecoder::new(
                    output_config.sample_rate().0 as usize,
                    output_config.channels() as usize,
                );

                resampled.initialize(&mut decoder);

                // Pre-fill output buffer before starting the stream
                while resampled.current(&decoder).len() <= output.buffer_space_available() {
                    output.write(resampled.current(&decoder)).unwrap();
                    if resampled.decode_next_frame(&mut decoder)?.is_none() {
                        break;
                    }
                }

                output.start()?;
            } else {
                if decoder.sample_rate() != resampled.in_sample_rate() {
                    output.write_blocking(resampled.flush());
                }
                resampled.initialize(&mut decoder);
            }

            let go_next = loop {
                if reset_rx.try_recv().is_ok() {
                    seek_position = Some(current_position);
                    initialized = false;
                    break false;
                }

                output.write_blocking(resampled.current(&decoder));
                match resampled.decode_next_frame(&mut decoder) {
                    Ok(None) => break true,
                    Ok(Some(_)) => {}
                    Err(DecoderError::ResetRequired) => {
                        break false;
                    }
                    Err(e) => {
                        return Err(e)?;
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
    output.write_blocking(resampled.flush());
    // Wait for all data to reach the audio device
    std::thread::sleep(output.buffer_duration());
    Ok(())
}
