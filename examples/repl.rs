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
    Reset,
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
    let (command_tx, command_rx) = mpsc::sync_channel(32);
    let command_tx_ = command_tx.clone();
    let output_builder = OutputBuilder::new(
        move || {
            command_tx_.send(Command::Reset).unwrap();
        },
        |err| error!("Output error: {err}"),
    );

    std::thread::spawn(move || event_loop(output_builder, queue_rx, command_rx).unwrap());

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
    output_builder: OutputBuilder,
    queue_rx: mpsc::Receiver<String>,
    command_rx: mpsc::Receiver<Command>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut output_config = output_builder.default_output_config()?;

    let mut output: AudioOutput<f32> =
        output_builder.new_output::<f32>(None, output_config.clone())?;

    let mut resampled = ResampledDecoder::<f32>::new(
        output_config.sample_rate().0 as usize,
        output_config.channels() as usize,
    );
    let mut current_file: String;
    let mut reset: bool;
    let mut paused = false;
    let mut current_position = Duration::default();
    let mut seek_position: Option<Duration> = None;

    loop {
        match queue_rx.try_recv() {
            Ok(file_name) => {
                current_file = file_name;
                reset = false;
            }
            Err(TryRecvError::Empty) => {
                reset = true;
                output.write_blocking(resampled.flush());
                std::thread::sleep(output.buffer_duration());
                output.stop();

                current_file = queue_rx.recv()?;
            }
            Err(TryRecvError::Disconnected) => {
                return Ok(());
            }
        };

        loop {
            let file = File::open(&current_file)?;
            let file_len = file.metadata()?.len();

            let extension = Path::new(&current_file)
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
            if paused {
                decoder.pause();
            }

            if reset {
                reset = false;

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
            } else {
                if decoder.sample_rate() != resampled.in_sample_rate() {
                    output.write_blocking(resampled.flush());
                }
                resampled.initialize(&mut decoder);
            }

            let go_next = loop {
                if let Ok(command) = command_rx.try_recv() {
                    match command {
                        Command::Pause => {
                            decoder.pause();
                            paused = true;
                        }
                        Command::Play => {
                            decoder.resume();
                            paused = false;
                        }
                        Command::Next => {
                            break true;
                        }
                        Command::Stop => {
                            return Ok(());
                        }
                        Command::Seek(time) => {
                            decoder.seek(time).unwrap();
                        }
                        Command::Reset => {
                            reset = true;
                            seek_position = Some(current_position);
                            break false;
                        }
                    }
                }

                output.write_blocking(resampled.current(&decoder));
                match resampled.decode_next_frame(&mut decoder) {
                    Ok(DecoderResult::Finished) => break true,
                    Ok(DecoderResult::Unfinished) => {}
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
}
