use std::{error::Error, fs::File, io::BufReader, path::Path, time::Duration};

use cpal::{SampleFormat, SampleRate};
use dcal::{
    decoder::{Decoder, ReadSeekSource, ResampledDecoder},
    output::{OutputBuilder, RequestedOutputConfig},
};
use tracing::error;

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_line_number(true)
        .with_file(true)
        .init();

    let output_builder = OutputBuilder::new(|| {}, |err| error!("Output error: {err}"));
    let default_output_config = output_builder.default_output_config()?;

    let file = File::open("examples/music.mp3")?;
    let file_len = file.metadata()?.len();

    let extension = Path::new("examples/music.mp3")
        .extension()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let reader = BufReader::new(file);
    let source = Box::new(ReadSeekSource::new(reader, Some(file_len), Some(extension)));

    let mut decoder =
        Decoder::<f32>::new(source, 1.0, default_output_config.channels() as usize, None)?;

    let output_config = output_builder.find_closest_config(
        None,
        RequestedOutputConfig {
            sample_rate: Some(SampleRate(decoder.sample_rate() as u32)),
            channels: Some(default_output_config.channels()),
            sample_format: Some(SampleFormat::F32),
        },
    )?;

    let mut resampled = ResampledDecoder::new(
        output_config.sample_rate().0 as usize,
        output_config.channels() as usize,
    );

    resampled.initialize(&mut decoder);

    let mut output = output_builder.new_output(None, output_config)?;

    // Prefill output buffer before starting the stream
    while resampled.current(&decoder).len() <= output.buffer_space_available() {
        output.write(resampled.current(&decoder)).unwrap();
        resampled.decode_next_frame(&mut decoder)?;
    }

    output.start()?;

    loop {
        output.write_blocking(resampled.current(&decoder));
        if resampled.decode_next_frame(&mut decoder)?.is_none() {
            std::thread::sleep(Duration::from_millis(200));
            return Ok(());
        }
    }
}
