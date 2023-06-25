use cpal::{
    default_host,
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BackendSpecificError, BuildStreamError, ChannelCount, DefaultStreamConfigError, Device,
    DevicesError, Host, HostId, HostUnavailable, OutputCallbackInfo, PlayStreamError, SampleFormat,
    SampleRate, Stream, StreamConfig, StreamError, SupportedStreamConfig,
    SupportedStreamConfigsError,
};
use rb::{RbConsumer, RbProducer, SpscRb, RB};
use std::{
    sync::{Arc, RwLock},
    time::Duration,
};
use thiserror::Error;
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum AudioOutputError {
    #[error("No default device found")]
    NoDefaultDevice,
    #[error("Error getting default device config: {0}")]
    OutputDeviceConfigError(DefaultStreamConfigError),
    #[error("Error opening output stream: {0}")]
    OpenStreamError(BuildStreamError),
    #[error("Error starting stream: {0}")]
    StartStreamError(PlayStreamError),
    #[error("Unsupported device configuration: {0}")]
    UnsupportedConfiguration(String),
    #[error("Error loading devices: {0}")]
    LoadDevicesError(DevicesError),
    #[error("Error loading config: {0}")]
    LoadConfigsError(SupportedStreamConfigsError),
}

pub struct RequestedOutputConfig {
    pub(crate) sample_rate: Option<SampleRate>,
    pub(crate) channels: Option<ChannelCount>,
    pub(crate) sample_format: Option<SampleFormat>,
}

pub struct OutputBuilder {
    host: Arc<Host>,
    on_device_changed: Arc<Box<dyn Fn() + Send + Sync>>,
    on_error: Arc<Box<dyn Fn(BackendSpecificError) + Send + Sync>>,
    current_device: Arc<RwLock<Option<String>>>,
}

impl OutputBuilder {
    pub fn new<F1, F2>(on_device_changed: F1, on_error: F2) -> Self
    where
        F1: Fn() + Send + Sync + 'static,
        F2: Fn(BackendSpecificError) + Send + Sync + 'static,
    {
        Self::new_from_host(default_host(), on_device_changed, on_error)
    }

    pub fn new_from_host_id<F1, F2>(
        host_id: HostId,
        on_device_changed: F1,
        on_error: F2,
    ) -> Result<Self, HostUnavailable>
    where
        F1: Fn() + Send + Sync + 'static,
        F2: Fn(BackendSpecificError) + Send + Sync + 'static,
    {
        Ok(Self::new_from_host(
            cpal::host_from_id(host_id)?,
            on_device_changed,
            on_error,
        ))
    }

    pub fn new_from_host<F1, F2>(host: Host, on_device_changed: F1, on_error: F2) -> Self
    where
        F1: Fn() + Send + Sync + 'static,
        F2: Fn(BackendSpecificError) + Send + Sync + 'static,
    {
        let builder = Self {
            host: Arc::new(host),
            on_device_changed: Arc::new(Box::new(on_device_changed)),
            on_error: Arc::new(Box::new(on_error)),
            current_device: Default::default(),
        };

        #[cfg(windows)]
        {
            let current_device = builder.current_device.clone();
            let host = builder.host.clone();
            let on_device_changed = builder.on_device_changed.clone();

            std::thread::spawn(move || {
                let mut current_default_device = host
                    .default_output_device()
                    .map(|d| d.name().unwrap_or_default())
                    .unwrap_or_default();
                loop {
                    if current_device.read().expect("lock poisioned").is_none() {
                        let default_device = host
                            .default_output_device()
                            .map(|d| d.name().unwrap_or_default())
                            .unwrap_or_default();

                        if default_device != current_default_device {
                            on_device_changed();
                            current_default_device = default_device;
                        }
                    }

                    std::thread::sleep(Duration::from_secs(1));
                }
            });
        }

        builder
    }

    pub fn default_output_config(&self) -> Result<SupportedStreamConfig, AudioOutputError> {
        let device = self
            .host
            .default_output_device()
            .ok_or(AudioOutputError::NoDefaultDevice)?;
        device
            .default_output_config()
            .map_err(AudioOutputError::OutputDeviceConfigError)
    }

    pub fn find_closest_config(
        &self,
        device_name: Option<String>,
        config: RequestedOutputConfig,
    ) -> Result<SupportedStreamConfig, AudioOutputError> {
        let default_device = self
            .host
            .default_output_device()
            .ok_or(AudioOutputError::NoDefaultDevice)?;
        let device = match &device_name {
            Some(device_name) => self
                .host
                .devices()
                .map_err(AudioOutputError::LoadDevicesError)?
                .find(|d| {
                    d.name()
                        .map(|n| n.trim() == device_name.trim())
                        .unwrap_or(false)
                })
                .unwrap_or(default_device),
            None => default_device,
        };
        let default_config = device
            .default_output_config()
            .map_err(AudioOutputError::OutputDeviceConfigError)?;

        let channels = config.channels.unwrap_or(default_config.channels());
        let sample_rate = config.sample_rate.unwrap_or(default_config.sample_rate());
        let sample_format = config
            .sample_format
            .unwrap_or(default_config.sample_format());

        if default_config.channels() == channels
            && default_config.sample_rate() == sample_rate
            && default_config.sample_format() == sample_format
        {
            return Ok(default_config);
        }

        if let Some(matched_config) = device
            .supported_output_configs()
            .map_err(AudioOutputError::LoadConfigsError)?
            .find(|c| {
                c.channels() == channels
                    && c.sample_format() == sample_format
                    && c.min_sample_rate() <= sample_rate
                    && c.max_sample_rate() >= sample_rate
            })
        {
            return Ok(matched_config.with_sample_rate(sample_rate));
        }

        Ok(default_config)
    }

    pub fn new_output(
        &self,
        device_name: Option<String>,
        config: SupportedStreamConfig,
    ) -> Result<AudioOutput, AudioOutputError> {
        *self.current_device.write().expect("lock poisoned") = device_name.clone();
        let default_device = self
            .host
            .default_output_device()
            .ok_or(AudioOutputError::NoDefaultDevice)?;

        let device = match &device_name {
            Some(device_name) => self
                .host
                .devices()
                .map_err(AudioOutputError::LoadDevicesError)?
                .find(|d| {
                    d.name()
                        .map(|n| n.trim() == device_name.trim())
                        .unwrap_or(false)
                })
                .unwrap_or(default_device),
            None => default_device,
        };
        info!("Using device: {:?}", device.name());
        info!("Device config: {config:?}");

        Ok(AudioOutput::new(
            device,
            config,
            self.on_device_changed.clone(),
            self.on_error.clone(),
        ))
    }
}

pub struct AudioOutput {
    ring_buf_producer: Option<rb::Producer<f32>>,
    stream: Option<Stream>,
    on_device_changed: Arc<Box<dyn Fn() + Send + Sync>>,
    on_error: Arc<Box<dyn Fn(BackendSpecificError) + Send + Sync>>,
    device: Device,
    config: SupportedStreamConfig,
}

impl AudioOutput {
    pub(crate) fn new(
        device: Device,
        config: SupportedStreamConfig,
        on_device_changed: Arc<Box<dyn Fn() + Send + Sync>>,
        on_error: Arc<Box<dyn Fn(BackendSpecificError) + Send + Sync>>,
    ) -> Self {
        Self {
            ring_buf_producer: None,
            stream: None,
            device,
            config,
            on_device_changed,
            on_error,
        }
    }

    pub fn start(&mut self) -> Result<(), AudioOutputError> {
        if self.stream.is_some() {
            return Ok(());
        }

        let buffer_ms = 200;
        let ring_buf = SpscRb::<f32>::new(
            ((buffer_ms * self.config.sample_rate().0 as usize) / 1000)
                * self.config.channels() as usize,
        );
        let (ring_buf_producer, ring_buf_consumer) = (ring_buf.producer(), ring_buf.consumer());

        let stream = self.create_stream(ring_buf_consumer)?;

        self.ring_buf_producer = Some(ring_buf_producer);
        self.stream = Some(stream);

        Ok(())
    }

    pub fn stop(&mut self) {
        self.stream = None;
    }

    pub fn write(&mut self, mut samples: &[f32]) {
        if let Some(producer) = &self.ring_buf_producer {
            loop {
                match producer.write_blocking_timeout(samples, Duration::from_millis(1000)) {
                    Ok(Some(written)) => {
                        samples = &samples[written..];
                    }
                    Ok(None) => {
                        break;
                    }
                    Err(_) => {
                        info!("Consumer stalled. Terminating.");
                        return;
                    }
                }
            }
        }
    }

    fn create_stream(
        &self,
        ring_buf_consumer: rb::Consumer<f32>,
    ) -> Result<Stream, AudioOutputError> {
        let channels = self.config.channels();
        let config = StreamConfig {
            channels: self.config.channels(),
            sample_rate: self.config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };
        info!("Output channels = {channels}");
        info!("Output sample rate = {}", self.config.sample_rate().0);

        let filler = 0.0;
        let on_error = self.on_error.clone();
        let on_device_changed = self.on_device_changed.clone();
        let stream = self
            .device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &OutputCallbackInfo| {
                    // Write out as many samples as possible from the ring buffer to the audio output.
                    let written = ring_buf_consumer.read(data).unwrap_or(0);
                    // Mute any remaining samples.
                    if data.len() > written {
                        warn!("Output buffer not full, muting remaining");
                        data[written..].iter_mut().for_each(|s| *s = filler);
                    }
                },
                move |err| match err {
                    StreamError::DeviceNotAvailable => {
                        info!("Device unplugged. Resetting...");
                        on_device_changed();
                    }
                    StreamError::BackendSpecific { err } => {
                        on_error(err);
                    }
                },
                None,
            )
            .map_err(AudioOutputError::OpenStreamError)?;

        // Start the output stream.
        stream.play().map_err(AudioOutputError::StartStreamError)?;

        Ok(stream)
    }
}
