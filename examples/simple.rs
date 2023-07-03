use decal::{
    decoder::{DecoderResult, ReadSeekSource},
    output::OutputBuilder,
    AudioManager,
};
use std::{error::Error, path::Path};
use tracing::error;

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_line_number(true)
        .with_file(true)
        .init();

    let output_builder = OutputBuilder::new(|| {}, |err| error!("Output error: {err}"));
    let mut manager = AudioManager::<f32>::new(output_builder);

    let source = Box::new(ReadSeekSource::from_path(Path::new("examples/music.mp3")));

    let mut decoder = manager.init_decoder(source);

    manager.reset(&mut decoder);

    loop {
        if manager.write(&mut decoder)? == DecoderResult::Finished {
            manager.flush();
            return Ok(());
        }
    }
}
