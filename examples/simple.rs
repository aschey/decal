use decal::{
    decoder::{DecoderResult, DecoderSettings, ReadSeekSource, ResamplerSettings},
    output::{CpalOutput, OutputBuilder, OutputSettings},
    AudioManager,
};
use std::{error::Error, path::Path};
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
    let mut manager = AudioManager::<f32, _>::new(output_builder, ResamplerSettings::default());

    let source = Box::new(ReadSeekSource::from_path(Path::new("examples/music.mp3")));

    let mut decoder = manager.init_decoder(source, DecoderSettings::default());

    manager.reset(&mut decoder)?;

    loop {
        if manager.write(&mut decoder)? == DecoderResult::Finished {
            manager.flush()?;
            return Ok(());
        }
    }
}
