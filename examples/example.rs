use std::{fs::File, io::BufReader, path::Path};

use dcal::{
    decoder::{AudioManager, ReadSeekSource},
    output::OutputBuilder,
};

fn main() {
    let output_builder = OutputBuilder::new(|| {}, |_| {});
    let mut manager = AudioManager::new(output_builder, 1.0).unwrap();

    let file = File::open("examples/music.mp3").unwrap();

    let file_len = file.metadata().unwrap().len();

    let extension = Path::new("./music.mp3")
        .extension()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let reader = BufReader::new(file);
    let source = Box::new(ReadSeekSource::new(reader, Some(file_len), Some(extension)));
    let mut decoder = manager.initialize_decoder(source, None, None).unwrap();
    manager.start().unwrap();
    manager.decode_source(&decoder, 1024);
    loop {
        if manager.decode_next(&mut decoder).unwrap() == 0 {
            return;
        }
    }
}
