use cpal::{SampleFormat, SampleRate};
use crossterm::style::Stylize;
use dcal::{
    decoder::{Decoder, DecoderError, DecoderResult, ReadSeekSource, ResampledDecoder},
    output::{AudioOutput, OutputBuilder, RequestedOutputConfig},
};
use reedline::{
    default_emacs_keybindings, Color, ColumnarMenu, DefaultCompleter, DefaultPrompt, Emacs,
    KeyCode, KeyModifiers, Reedline, ReedlineEvent, ReedlineMenu, Signal,
};
use std::{
    error::Error,
    fs::File,
    io::{self, BufReader},
    path::Path,
    sync::mpsc::{self, TryRecvError},
    time::Duration,
};
use tracing::error;

enum Command {
    Play,
    Pause,
    Next,
    Stop,
    Seek(Duration),
}

fn main() -> io::Result<()> {
    let commands = vec![
        "add".into(),
        "play".into(),
        "pause".into(),
        "seek".into(),
        "next".into(),
        "stop".into(),
    ];

    println!("\n{}", "Available Commands:".with(Color::Blue).bold());
    println!(
        "add {}  {}",
        "<file path>".cyan(),
        "Adds a song to the queue".with(Color::DarkGrey).dim()
    );
    println!(
        "play             {}",
        "Starts or resumes the queue".with(Color::DarkGrey)
    );
    println!(
        "pause            {}",
        "Pauses the queue".with(Color::DarkGrey).dim()
    );
    println!(
        "seek {}   {}",
        "<seconds>".cyan(),
        "Seek to a specific point in the song"
            .with(Color::DarkGrey)
            .dim()
    );
    println!(
        "next             {}",
        "Skips to the next song".with(Color::DarkGrey).dim()
    );
    println!(
        "stop             {}\n",
        "Stops the queue".with(Color::DarkGrey).dim()
    );

    let completer = Box::new(DefaultCompleter::new_with_wordlen(commands, 0));
    let completion_menu = Box::new(ColumnarMenu::default().with_name("completion_menu"));

    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );

    let edit_mode = Box::new(Emacs::new(keybindings));

    let mut line_editor = Reedline::create()
        .with_completer(completer)
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode);
    let prompt = DefaultPrompt::default();

    let (queue_tx, queue_rx) = mpsc::channel();
    let (command_tx, command_rx) = mpsc::channel();

    std::thread::spawn(move || event_loop(queue_rx, command_rx).unwrap());

    loop {
        let sig = line_editor.read_line(&prompt)?;
        match sig {
            Signal::Success(buffer) => {
                let buffer = buffer.to_lowercase();
                match (buffer.as_str(), buffer.split_once(' ')) {
                    (_, Some(("add", val))) => {
                        queue_tx.send(val.to_owned()).unwrap();
                    }
                    (val @ "add" | val @ "seek", None) => {
                        println!("Command '{val}' requires one argument");
                    }
                    ("play", None) => {
                        command_tx.send(Command::Play).unwrap();
                    }
                    ("pause", None) => {
                        command_tx.send(Command::Pause).unwrap();
                    }
                    (_, Some(("seek", val))) => {
                        command_tx
                            .send(Command::Seek(Duration::from_secs(val.parse().unwrap())))
                            .unwrap();
                    }
                    ("next", None) => {
                        command_tx.send(Command::Next).unwrap();
                    }
                    ("stop", None) => {
                        command_tx.send(Command::Stop).unwrap();
                    }
                    (cmd, _) => {
                        println!("Invalid command: {cmd}")
                    }
                }
            }
            Signal::CtrlD | Signal::CtrlC => {
                println!("\nAborted!");
                break Ok(());
            }
        }
    }
}

fn event_loop(
    queue_rx: mpsc::Receiver<String>,
    command_rx: mpsc::Receiver<Command>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let output_builder = OutputBuilder::new(
        move || {
            // reset_tx
            //     .send(())
            //     .tap_err(|e| error!("Error sending reset signal: {e}"))
            //     .ok();
        },
        |err| error!("Output error: {err}"),
    );

    let mut output_config = output_builder.default_output_config()?;

    let mut output: AudioOutput<f32> =
        output_builder.new_output::<f32>(None, output_config.clone())?;

    let mut resampled = ResampledDecoder::<f32>::new(
        output_config.sample_rate().0 as usize,
        output_config.channels() as usize,
    );

    loop {
        let mut decoder = match queue_rx.try_recv() {
            Ok(file_name) => {
                let file = File::open(&file_name)?;
                let file_len = file.metadata()?.len();

                let extension = Path::new(&file_name)
                    .extension()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                let reader = BufReader::new(file);
                let source = ReadSeekSource::new(reader, Some(file_len), Some(extension));
                let mut decoder = Decoder::<f32>::new(
                    Box::new(source),
                    1.0,
                    output_config.channels() as usize,
                    None,
                )?;
                resampled.initialize(&mut decoder);
                decoder
            }
            Err(TryRecvError::Empty) => {
                output.write_blocking(resampled.flush());
                std::thread::sleep(output.buffer_duration());
                output.stop();

                let file_name = queue_rx.recv()?;
                let file = File::open(&file_name)?;
                let file_len = file.metadata()?.len();

                let extension = Path::new(&file_name)
                    .extension()
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                let reader = BufReader::new(file);
                let source = ReadSeekSource::new(reader, Some(file_len), Some(extension));

                let mut decoder = Decoder::<f32>::new(
                    Box::new(source),
                    1.0,
                    output_config.channels() as usize,
                    None,
                )?;
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
                    if resampled.decode_next_frame(&mut decoder)? == DecoderResult::Finished {
                        break;
                    }
                }
                output.start()?;

                decoder
            }
            Err(TryRecvError::Disconnected) => {
                return Ok(());
            }
        };

        loop {
            // if reset_rx.try_recv().is_ok() {
            //     seek_position = Some(current_position);
            //     initialized = false;
            //     break false;
            // }
            if let Ok(command) = command_rx.try_recv() {
                match command {
                    Command::Pause => {
                        decoder.pause();
                    }
                    Command::Play => {
                        decoder.resume();
                    }
                    Command::Next => {
                        break;
                    }
                    Command::Stop => {
                        return Ok(());
                    }
                    Command::Seek(time) => {
                        decoder.seek(time).unwrap();
                    }
                }
            }
            output.write_blocking(resampled.current(&decoder));
            match resampled.decode_next_frame(&mut decoder) {
                Ok(DecoderResult::Finished) => break,
                Ok(DecoderResult::Unfinished) => {}
                Err(DecoderError::ResetRequired) => {
                    break; //false;
                }
                Err(e) => {
                    return Err(e)?;
                }
            }
            // current_position = decoder.current_position().position;
        }
    }
}
