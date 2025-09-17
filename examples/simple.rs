use std::error::Error;
use std::path::Path;

use decal::AudioManager;
use decal::decoder::{DecoderSettings, ReadSeekSource, ResamplerSettings};
use decal::output::{CpalOutput, OutputBuilder, OutputSettings};
use tracing::error;

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_line_number(true)
        .with_file(true)
        .init();

    let output_builder = OutputBuilder::new(
        CpalOutput::default(),
        OutputSettings::default(),
        || {},
        |err| error!("Output error: {err}"),
    );
    let mut manager = AudioManager::<f32, _>::new(output_builder, ResamplerSettings::default())?;

    let source = Box::new(ReadSeekSource::from_path(Path::new(
        "/home/aschey/code/platune/test.opus",
    ))?);

    let mut decoder = manager.init_decoder(source, DecoderSettings::default())?;

    manager.reset(&mut decoder)?;
    manager.write_all(&mut decoder)?;
    Ok(())
}
