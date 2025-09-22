use super::{
    AudioBackend, BackendSpecificError, BufferSize, BuildStreamError, DecalSample,
    DefaultStreamConfigError, Device, DeviceNameError, DevicesError, Host, PlayStreamError,
    SampleFormat, Stream, StreamConfig, StreamError, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange, SupportedStreamConfigsError,
};
use crate::{ChannelCount, SampleRate};
use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

#[derive(Default, Clone)]
pub struct CpalOutput {}

pub struct CpalHost(cpal::Host);

pub struct CpalDevice(cpal::Device);

pub struct CpalDevices(cpal::OutputDevices<<cpal::Host as cpal::traits::HostTrait>::Devices>);

impl Iterator for CpalDevices {
    type Item = CpalDevice;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(CpalDevice)
    }
}

pub struct CpalStream(cpal::Stream);

impl Stream for CpalStream {
    fn play(&self) -> Result<(), PlayStreamError> {
        self.0.play().unwrap();
        Ok(())
    }

    fn stop(&self) -> Result<(), PlayStreamError> {
        Ok(())
    }
}

impl Device for CpalDevice {
    type SupportedOutputConfigs = Box<dyn Iterator<Item = SupportedStreamConfigRange>>;

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let config = self.0.default_output_config().unwrap();

        Ok(SupportedStreamConfig {
            channels: ChannelCount(config.channels()),
            sample_rate: SampleRate(config.sample_rate().0),
            buffer_size: match config.buffer_size() {
                cpal::SupportedBufferSize::Range { min, max } => SupportedBufferSize::Range {
                    min: *min,
                    max: *max,
                },
                cpal::SupportedBufferSize::Unknown => SupportedBufferSize::Unknown,
            },
            sample_format: match config.sample_format() {
                cpal::SampleFormat::I8 => SampleFormat::I8,
                cpal::SampleFormat::I16 => SampleFormat::I16,
                cpal::SampleFormat::I32 => SampleFormat::I32,
                cpal::SampleFormat::I64 => SampleFormat::I64,
                cpal::SampleFormat::U8 => SampleFormat::U8,
                cpal::SampleFormat::U16 => SampleFormat::U16,
                cpal::SampleFormat::U32 => SampleFormat::U32,
                cpal::SampleFormat::U64 => SampleFormat::U64,
                cpal::SampleFormat::F32 => SampleFormat::F32,
                cpal::SampleFormat::F64 => SampleFormat::F64,
                _ => unimplemented!("unsupported"),
            },
        })
    }

    fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.0.name().unwrap())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Ok(Box::new(self.0.supported_output_configs().unwrap().map(
            |c| SupportedStreamConfigRange {
                channels: ChannelCount(c.channels()),
                min_sample_rate: SampleRate(c.max_sample_rate().0),
                max_sample_rate: SampleRate(c.max_sample_rate().0),
                buffer_size: match c.buffer_size() {
                    cpal::SupportedBufferSize::Range { min, max } => SupportedBufferSize::Range {
                        min: *min,
                        max: *max,
                    },
                    cpal::SupportedBufferSize::Unknown => SupportedBufferSize::Unknown,
                },
                sample_format: match c.sample_format() {
                    cpal::SampleFormat::I8 => SampleFormat::I8,
                    cpal::SampleFormat::I16 => SampleFormat::I16,
                    cpal::SampleFormat::I32 => SampleFormat::I32,
                    cpal::SampleFormat::I64 => SampleFormat::I64,
                    cpal::SampleFormat::U8 => SampleFormat::U8,
                    cpal::SampleFormat::U16 => SampleFormat::U16,
                    cpal::SampleFormat::U32 => SampleFormat::U32,
                    cpal::SampleFormat::U64 => SampleFormat::U64,
                    cpal::SampleFormat::F32 => SampleFormat::F32,
                    cpal::SampleFormat::F64 => SampleFormat::F64,
                    _ => unimplemented!("unsupported"),
                },
            },
        )))
    }

    fn build_output_stream<T, D, E>(
        &self,
        config: &StreamConfig,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Result<Box<dyn Stream>, BuildStreamError>
    where
        T: DecalSample,
        D: FnMut(&mut [T]) + Send + 'static,
        E: FnMut(StreamError) + Send + Sync + 'static,
    {
        let stream = self
            .0
            .build_output_stream(
                &cpal::StreamConfig {
                    channels: config.channels.0,
                    sample_rate: cpal::SampleRate(config.sample_rate.0),
                    buffer_size: match config.buffer_size {
                        BufferSize::Fixed(val) => cpal::BufferSize::Fixed(val),
                        BufferSize::Default => cpal::BufferSize::Default,
                    },
                },
                move |data: &mut [T], _| {
                    data_callback(data);
                },
                move |stream_error| {
                    error_callback(match stream_error {
                        cpal::StreamError::DeviceNotAvailable => StreamError::DeviceNotAvailable,
                        cpal::StreamError::BackendSpecific { err } => {
                            StreamError::BackendSpecific(BackendSpecificError(err.to_string()))
                        }
                    });
                },
                None,
            )
            .map_err(|e| match e {
                cpal::BuildStreamError::DeviceNotAvailable => BuildStreamError::DeviceNotAvailable,
                cpal::BuildStreamError::StreamConfigNotSupported => {
                    BuildStreamError::StreamConfigNotSupported
                }
                cpal::BuildStreamError::InvalidArgument => BuildStreamError::InvalidArgument,
                cpal::BuildStreamError::StreamIdOverflow => BuildStreamError::StreamIdOverflow,
                cpal::BuildStreamError::BackendSpecific { err } => {
                    BuildStreamError::BackendSpecific(BackendSpecificError(err.to_string()))
                }
            })?;

        Ok(Box::new(CpalStream(stream)))
    }
}

impl Host for CpalHost {
    type Device = CpalDevice;

    type Devices = CpalDevices;

    fn default_output_device(&self) -> Option<Self::Device> {
        self.0.default_output_device().map(CpalDevice)
    }

    fn output_devices(&self) -> Result<Self::Devices, DevicesError> {
        Ok(CpalDevices(self.0.output_devices().unwrap()))
    }
}

impl AudioBackend for CpalOutput {
    type Host = CpalHost;
    type Device = CpalDevice;

    fn default_host(&self) -> Self::Host {
        CpalHost(cpal::default_host())
    }
}
