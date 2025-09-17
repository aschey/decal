use std::error::Error;

use decal::AudioManager;
use decal::decoder::{DecoderResult, DecoderSettings, ReadSeekSource, ResamplerSettings};
use decal::output::{CpalOutput, OutputBuilder, OutputSettings};
use stream_download::http::HttpStream;
use stream_download::http::reqwest::Client;
use stream_download::source::{DecodeError, SourceStream};
use stream_download::storage::memory::MemoryStorageProvider;
use stream_download::{Settings, StreamDownload};
use tracing::metadata::LevelFilter;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::default().add_directive(LevelFilter::INFO.into()))
        .with_line_number(true)
        .with_file(true)
        .init();

    let output_builder = OutputBuilder::new(
        CpalOutput::default(),
        OutputSettings::default(),
        || {},
        |err| error!("Output error: {err}"),
    );

    let stream = HttpStream::<Client>::create(
        "http://www.hyperion-records.co.uk/audiotest/14 Clementi Piano Sonata in D major, Op 25 \
         No 6 - Movement 2 Un poco andante.MP3"
            .parse()?,
    )
    .await?;
    let content_type = stream.content_type().clone();

    info!("content length={:?}", stream.content_length());
    info!("content type={content_type:?}");
    let stream = match StreamDownload::from_stream(
        stream,
        MemoryStorageProvider,
        Settings::default().prefetch_bytes(1024),
    )
    .await
    {
        Ok(stream) => stream,
        Err(e) => return Err(e.decode_error().await.into()),
    };
    let source = Box::new(ReadSeekSource::new(
        stream,
        None,
        content_type.map(|c| match c.subtype.as_str() {
            "mpeg" => "mp3".to_owned(),
            subtype => subtype.to_owned(),
        }),
    ));

    tokio::task::spawn_blocking(move || {
        let mut manager =
            AudioManager::<f32, _>::new(output_builder, ResamplerSettings::default())?;
        let mut decoder = manager.init_decoder(source, DecoderSettings::default())?;
        manager.reset(&mut decoder)?;
        loop {
            if manager.write(&mut decoder)? == DecoderResult::Finished {
                manager.flush()?;
                return Ok::<_, Box<dyn Error + Send + Sync>>(());
            }
        }
    })
    .await??;

    Ok(())
}
