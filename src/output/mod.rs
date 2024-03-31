use std::sync::{Arc, RwLock};
use std::time::Duration;

use rb::{RbConsumer, RbInspector, RbProducer, SpscRb, RB};
use thiserror::Error;
use tracing::{info, warn};

#[cfg(feature = "output-cpal")]
mod cpal;
#[cfg(feature = "output-cpal")]
pub use cpal::*;
#[cfg(feature = "mock")]
mod mock;
#[cfg(feature = "mock")]
pub use mock::*;

#[cfg(feature = "output-cubeb")]
mod cubeb;
#[cfg(feature = "output-cubeb")]
pub use cubeb::*;

#[derive(thiserror::Error, Debug)]
pub enum PlayStreamError {}

#[derive(thiserror::Error, Debug)]
pub enum DeviceNameError {}

#[derive(thiserror::Error, Debug)]
pub enum SupportedStreamConfigsError {}

#[derive(thiserror::Error, Debug)]
pub enum DefaultStreamConfigError {}

#[derive(thiserror::Error, Debug)]
pub enum BuildStreamError {}

#[derive(thiserror::Error, Debug)]
pub enum StreamError {
    #[error("Device not available")]
    DeviceNotAvailable,
    #[error("{0}")]
    BackendSpecific(BackendSpecificError),
}

#[derive(thiserror::Error, Debug)]
pub enum DevicesError {}

#[derive(thiserror::Error, Debug)]
pub enum HostUnavailableError {}

#[derive(thiserror::Error, Debug)]
#[error("{0}")]
pub struct BackendSpecificError(pub String);

pub trait Stream {
    fn play(&self) -> Result<(), PlayStreamError>;
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum SampleFormat {
    /// `i8` with a valid range of 'u8::MIN..=u8::MAX' with `0` being the origin
    I8,

    /// `i16` with a valid range of 'u16::MIN..=u16::MAX' with `0` being the origin
    I16,

    // /// `I24` with a valid range of '-(1 << 23)..(1 << 23)' with `0` being the origin
    // I24,
    /// `i32` with a valid range of 'u32::MIN..=u32::MAX' with `0` being the origin
    I32,

    // /// `I24` with a valid range of '-(1 << 47)..(1 << 47)' with `0` being the origin
    // I48,
    /// `i64` with a valid range of 'u64::MIN..=u64::MAX' with `0` being the origin
    I64,

    /// `u8` with a valid range of 'u8::MIN..=u8::MAX' with `1 << 7 == 128` being the origin
    U8,

    /// `u16` with a valid range of 'u16::MIN..=u16::MAX' with `1 << 15 == 32768` being the origin
    U16,

    // /// `U24` with a valid range of '0..16777216' with `1 << 23 == 8388608` being the origin
    // U24,
    /// `u32` with a valid range of 'u32::MIN..=u32::MAX' with `1 << 31` being the origin
    U32,

    // /// `U48` with a valid range of '0..(1 << 48)' with `1 << 47` being the origin
    // U48,
    /// `u64` with a valid range of 'u64::MIN..=u64::MAX' with `1 << 63` being the origin
    U64,

    /// `f32` with a valid range of `-1.0..1.0` with `0.0` being the origin
    F32,

    /// `f64` with a valid range of -1.0..1.0 with 0.0 being the origin
    F64,
}

pub type FrameCount = u32;

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum SupportedBufferSize {
    Range {
        min: FrameCount,
        max: FrameCount,
    },
    /// In the case that the platform provides no way of getting the default
    /// buffersize before starting a stream.
    Unknown,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
pub struct SampleRate(pub u32);

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
pub struct HostId(u32);

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
pub enum BufferSize {
    Default,
    Fixed(u32),
}

pub type ChannelCount = u32;

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct SupportedStreamConfig {
    pub channels: ChannelCount,
    pub sample_rate: SampleRate,
    pub buffer_size: SupportedBufferSize,
    pub sample_format: SampleFormat,
}

pub struct StreamConfig {
    pub channels: ChannelCount,
    pub sample_rate: SampleRate,
    pub buffer_size: BufferSize,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct SupportedStreamConfigRange {
    pub(crate) channels: ChannelCount,
    /// Minimum value for the samples rate of the supported formats.
    pub(crate) min_sample_rate: SampleRate,
    /// Maximum value for the samples rate of the supported formats.
    pub(crate) max_sample_rate: SampleRate,
    /// Buffersize ranges supported by the device
    pub(crate) buffer_size: SupportedBufferSize,
    /// Type of data expected by the device.
    pub(crate) sample_format: SampleFormat,
}

impl SupportedStreamConfigRange {
    fn with_sample_rate(self, sample_rate: SampleRate) -> SupportedStreamConfig {
        SupportedStreamConfig {
            channels: self.channels,
            sample_rate,
            buffer_size: self.buffer_size,
            sample_format: self.sample_format,
        }
    }
}

#[cfg(feature = "cpal")]
pub trait DecalSample:
    ::cpal::SizedSample + dasp::Sample + Send + Sync + Default + 'static
{
    const FORMAT: SampleFormat;
}

#[cfg(feature = "cpal")]
impl<T> DecalSample for T
where
    T: ::cpal::SizedSample + dasp::Sample + Send + Sync + Default + 'static,
{
    const FORMAT: SampleFormat = match <T as ::cpal::SizedSample>::FORMAT {
        ::cpal::SampleFormat::I8 => SampleFormat::I8,
        ::cpal::SampleFormat::I16 => SampleFormat::I16,
        ::cpal::SampleFormat::I32 => SampleFormat::I32,
        ::cpal::SampleFormat::I64 => SampleFormat::I64,
        ::cpal::SampleFormat::U8 => SampleFormat::U8,
        ::cpal::SampleFormat::U16 => SampleFormat::U16,
        ::cpal::SampleFormat::U32 => SampleFormat::U32,
        ::cpal::SampleFormat::U64 => SampleFormat::U64,
        ::cpal::SampleFormat::F32 => SampleFormat::F32,
        ::cpal::SampleFormat::F64 => SampleFormat::F64,
        _ => unimplemented!(),
    };
}

#[cfg(not(feature = "cpal"))]
pub trait DecalSample: dasp::Sample + Send + Sync + Default {}

#[cfg(not(feature = "cpal"))]
impl<T> DecalSample for T where T: dasp::Sample + Send + Sync + Default {}

pub trait Device {
    type SupportedOutputConfigs: Iterator<Item = SupportedStreamConfigRange>;

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError>;

    fn name(&self) -> Result<String, DeviceNameError>;

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError>;

    fn build_output_stream<T, D, E>(
        &self,
        config: &StreamConfig,
        data_callback: D,
        error_callback: E,
    ) -> Result<Box<dyn Stream>, BuildStreamError>
    where
        T: DecalSample,
        D: FnMut(&mut [T]) + Send + Sync + 'static,
        E: FnMut(StreamError) + Send + Sync + 'static;
}

pub trait Host {
    type Device: Device;
    type Devices: Iterator<Item = Self::Device>;
    fn default_output_device(&self) -> Option<Self::Device>;
    fn output_devices(&self) -> Result<Self::Devices, DevicesError>;
}

pub trait AudioBackend: Clone {
    type Host: Host<Device = Self::Device> + Send + Sync + 'static;
    type Device: Device;

    fn default_host(&self) -> Self::Host;
    // fn host_from_id(&self, id: HostId) -> Result<Self::Host, HostUnavailableError>;
}

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
    pub sample_rate: Option<SampleRate>,
    pub channels: Option<ChannelCount>,
    pub sample_format: Option<SampleFormat>,
}

#[derive(Clone)]
pub struct OutputSettings {
    pub buffer_duration: Duration,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            buffer_duration: Duration::from_millis(200),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum WriteBlockingError {
    #[error("Output stalled")]
    OutputStalled,
}

pub struct OutputBuilder<B: AudioBackend> {
    host: Arc<B::Host>,
    on_device_changed: Arc<Box<dyn Fn() + Send + Sync>>,
    on_error: Arc<Box<dyn Fn(BackendSpecificError) + Send + Sync>>,
    current_device: Arc<RwLock<Option<String>>>,
    settings: OutputSettings,
}

impl<B: AudioBackend> OutputBuilder<B> {
    pub fn new<F1, F2>(
        backend: B,
        settings: OutputSettings,
        on_device_changed: F1,
        on_error: F2,
    ) -> Self
    where
        B: AudioBackend,
        F1: Fn() + Send + Sync + 'static,
        F2: Fn(BackendSpecificError) + Send + Sync + 'static,
    {
        Self::new_from_host(
            backend.default_host(),
            settings,
            on_device_changed,
            on_error,
        )
    }

    pub fn settings(&self) -> &OutputSettings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: OutputSettings) {
        self.settings = settings;
    }

    // pub fn new_from_host_id<F1, F2>(
    //     backend: B,
    //     host_id: HostId,
    //     settings: OutputSettings,
    //     on_device_changed: F1,
    //     on_error: F2,
    // ) -> Result<Self, HostUnavailableError>
    // where
    //     B: AudioBackend,
    //     F1: Fn() + Send + Sync + 'static,
    //     F2: Fn(BackendSpecificError) + Send + Sync + 'static,
    // {
    //     Ok(Self::new_from_host(
    //         backend.host_from_id(host_id)?,
    //         settings,
    //         on_device_changed,
    //         on_error,
    //     ))
    // }

    pub fn new_from_host<F1, F2>(
        host: B::Host,
        settings: OutputSettings,
        on_device_changed: F1,
        on_error: F2,
    ) -> Self
    where
        F1: Fn() + Send + Sync + 'static,
        F2: Fn(BackendSpecificError) + Send + Sync + 'static,
    {
        let builder = Self {
            host: Arc::new(host),
            on_device_changed: Arc::new(Box::new(on_device_changed)),
            on_error: Arc::new(Box::new(on_error)),
            current_device: Default::default(),
            settings,
        };

        #[cfg(any(windows, test))]
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

        #[allow(clippy::let_and_return)]
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
        device_name: Option<&str>,
        config: RequestedOutputConfig,
    ) -> Result<SupportedStreamConfig, AudioOutputError> {
        let default_device = self
            .host
            .default_output_device()
            .ok_or(AudioOutputError::NoDefaultDevice)?;
        let device = match &device_name {
            Some(device_name) => self
                .host
                .output_devices()
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

        let channels = config.channels.unwrap_or(default_config.channels);
        let sample_rate = config.sample_rate.unwrap_or(default_config.sample_rate);
        let sample_format = config.sample_format.unwrap_or(default_config.sample_format);

        if default_config.channels == channels
            && default_config.sample_rate == sample_rate
            && default_config.sample_format == sample_format
        {
            return Ok(default_config);
        }

        if let Some(matched_config) = device
            .supported_output_configs()
            .map_err(AudioOutputError::LoadConfigsError)?
            .find(|c| {
                c.channels == channels
                    && c.sample_format == sample_format
                    && c.min_sample_rate <= sample_rate
                    && c.max_sample_rate >= sample_rate
            })
        {
            return Ok(matched_config.with_sample_rate(sample_rate));
        }

        Ok(default_config)
    }

    pub fn default_output_device(&self) -> Option<B::Device> {
        self.host.default_output_device()
    }

    pub fn output_devices(&self) -> Result<<B::Host as Host>::Devices, DevicesError> {
        self.host.output_devices()
    }

    pub fn new_output<T: DecalSample + Default + 'static>(
        &self,
        device_name: Option<String>,
        config: SupportedStreamConfig,
    ) -> Result<AudioOutput<T, B>, AudioOutputError> {
        *self.current_device.write().expect("lock poisoned") = device_name.clone();
        let default_device = self
            .host
            .default_output_device()
            .ok_or(AudioOutputError::NoDefaultDevice)?;

        let device = match &device_name {
            Some(device_name) => self
                .host
                .output_devices()
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

        Ok(AudioOutput::<T, B>::new(
            device,
            config,
            self.on_device_changed.clone(),
            self.on_error.clone(),
            self.settings.clone(),
        ))
    }
}

pub struct AudioOutput<T, B: AudioBackend> {
    ring_buf_producer: rb::Producer<T>,
    ring_buf: SpscRb<T>,
    stream: Option<Box<dyn Stream>>,
    on_device_changed: Arc<Box<dyn Fn() + Send + Sync>>,
    on_error: Arc<Box<dyn Fn(BackendSpecificError) + Send + Sync>>,
    device: B::Device,
    config: SupportedStreamConfig,
    settings: OutputSettings,
}

impl<T: DecalSample + Default + 'static, B: AudioBackend> AudioOutput<T, B> {
    pub(crate) fn new(
        device: B::Device,
        config: SupportedStreamConfig,
        on_device_changed: Arc<Box<dyn Fn() + Send + Sync>>,
        on_error: Arc<Box<dyn Fn(BackendSpecificError) + Send + Sync>>,
        settings: OutputSettings,
    ) -> Self {
        let buffer_duration = Duration::from_millis(200);
        let buffer_ms: usize = buffer_duration.as_millis().try_into().unwrap();
        let ring_buf = SpscRb::<T>::new(
            ((buffer_ms * config.sample_rate.0 as usize) / 1000) * config.channels as usize,
        );

        Self {
            ring_buf_producer: ring_buf.producer(),
            ring_buf,
            stream: None,
            device,
            config,
            on_device_changed,
            on_error,
            settings,
        }
    }

    pub fn start(&mut self) -> Result<(), AudioOutputError> {
        if self.stream.is_some() {
            return Ok(());
        }

        let stream = self.create_stream(self.ring_buf.consumer())?;
        stream.play().unwrap();
        self.stream = Some(stream);

        Ok(())
    }

    pub fn stop(&mut self) {
        self.stream = None;
    }

    pub fn is_buffer_full(&self) -> bool {
        self.ring_buf.is_full()
    }

    pub fn buffer_size(&self) -> usize {
        self.ring_buf.count()
    }

    pub fn buffer_capacity(&self) -> usize {
        self.ring_buf.capacity()
    }

    pub fn buffer_space_available(&self) -> usize {
        self.ring_buf.slots_free()
    }

    pub fn write(&self, samples: &[T]) -> Result<usize, rb::RbError> {
        self.ring_buf_producer.write(samples)
    }

    pub fn settings(&self) -> &OutputSettings {
        &self.settings
    }

    pub fn device(&self) -> &B::Device {
        &self.device
    }

    pub fn write_blocking(&self, mut samples: &[T]) -> Result<(), WriteBlockingError> {
        let timeout = self.settings.buffer_duration;
        loop {
            match self
                .ring_buf_producer
                .write_blocking_timeout(samples, timeout)
            {
                Ok(Some(written)) => {
                    samples = &samples[written..];
                }
                Ok(None) => {
                    break;
                }
                Err(_) => {
                    warn!("Audio stream stalled. Cancelling write.");
                    return Err(WriteBlockingError::OutputStalled);
                }
            }
        }
        Ok(())
    }

    fn create_stream(
        &self,
        ring_buf_consumer: rb::Consumer<T>,
    ) -> Result<Box<dyn Stream>, AudioOutputError> {
        let channels = self.config.channels;
        let config = StreamConfig {
            channels: self.config.channels,
            sample_rate: self.config.sample_rate,
            buffer_size: BufferSize::Default,
        };

        info!("Output channels = {channels}");
        info!("Output sample rate = {}", self.config.sample_rate.0);

        let filler = T::EQUILIBRIUM;
        let on_error = self.on_error.clone();
        let on_device_changed = self.on_device_changed.clone();
        let stream = self
            .device
            .build_output_stream(
                &config,
                move |data: &mut [T]| {
                    // Write out as many samples as possible from the ring buffer to the audio
                    // output.
                    let written = ring_buf_consumer.read(data).unwrap_or(0);
                    // Mute any remaining samples.
                    if data.len() > written {
                        warn!("Output buffer not full, muting remaining",);
                        data[written..].iter_mut().for_each(|s| *s = filler);
                    }
                },
                move |err| match err {
                    StreamError::DeviceNotAvailable => {
                        info!("Device unplugged. Resetting...");
                        on_device_changed();
                    }
                    StreamError::BackendSpecific(err) => {
                        on_error(err);
                    }
                },
            )
            .map_err(AudioOutputError::OpenStreamError)?;

        // Start the output stream.
        stream.play().map_err(AudioOutputError::StartStreamError)?;

        Ok(stream)
    }
}

#[cfg(test)]
#[path = "./output_config_test.rs"]
mod output_config_test;

#[cfg(test)]
#[path = "./write_output_test.rs"]
mod write_output_test;
