use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};

use super::{
    BufferSize, DecalSample, Device, Host, SampleFormat, Stream, StreamConfig, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange,
};
use crate::{ChannelCount, SampleRate, output};

pub struct CpalHost(cpal::Host);

impl Default for CpalHost {
    fn default() -> Self {
        Self(cpal::default_host())
    }
}

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
    fn play(&mut self) -> Result<(), output::Error> {
        self.0.play()?;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), output::Error> {
        self.0.pause()?;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), output::Error> {
        Ok(())
    }
}

impl Device for CpalDevice {
    type SupportedOutputConfigs = Box<dyn Iterator<Item = SupportedStreamConfigRange>>;

    fn default_output_config(&self) -> Result<SupportedStreamConfig, output::Error> {
        let config = self.0.default_output_config()?;

        Ok(SupportedStreamConfig {
            channels: ChannelCount(config.channels()),
            sample_rate: SampleRate(config.sample_rate()),
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

    fn name(&self) -> Result<String, output::Error> {
        Ok(self.0.description()?.name().to_string())
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, output::Error> {
        Ok(Box::new(self.0.supported_output_configs()?.map(|c| {
            SupportedStreamConfigRange {
                channels: ChannelCount(c.channels()),
                min_sample_rate: SampleRate(c.max_sample_rate()),
                max_sample_rate: SampleRate(c.max_sample_rate()),
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
                    cpal::SampleFormat::I24 => SampleFormat::I24,
                    cpal::SampleFormat::I32 => SampleFormat::I32,
                    cpal::SampleFormat::I64 => SampleFormat::I64,
                    cpal::SampleFormat::U8 => SampleFormat::U8,
                    cpal::SampleFormat::U16 => SampleFormat::U16,
                    cpal::SampleFormat::U24 => SampleFormat::U24,
                    cpal::SampleFormat::U32 => SampleFormat::U32,
                    cpal::SampleFormat::U64 => SampleFormat::U64,
                    cpal::SampleFormat::F32 => SampleFormat::F32,
                    cpal::SampleFormat::F64 => SampleFormat::F64,
                    c => unimplemented!("unsupported: {c:?}"),
                },
            }
        })))
    }

    fn build_output_stream<T, D, E>(
        &mut self,
        config: &StreamConfig,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Result<Box<dyn Stream>, output::Error>
    where
        T: DecalSample,
        D: FnMut(&mut [T]) + Send + 'static,
        E: FnMut(output::Error) + Send + Sync + 'static,
    {
        let stream = self
            .0
            .build_output_stream(
                cpal::StreamConfig {
                    channels: config.channels.0,
                    sample_rate: config.sample_rate.0,
                    buffer_size: match config.buffer_size {
                        BufferSize::Fixed(val) => cpal::BufferSize::Fixed(val),
                        BufferSize::Default => cpal::BufferSize::Default,
                    },
                },
                move |data: &mut [T], _| {
                    data_callback(data);
                },
                move |stream_error| {
                    error_callback(stream_error.into());
                },
                None,
            )
            .map_err(|e| {
                let err: output::Error = e.into();
                err
            })?;

        Ok(Box::new(CpalStream(stream)))
    }
}

impl Host for CpalHost {
    type Device = CpalDevice;
    type Id = cpal::HostId;
    type Devices = CpalDevices;

    fn from_id(id: cpal::HostId) -> Result<Self, output::Error> {
        Ok(cpal::host_from_id(id).map(CpalHost)?)
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        self.0.default_output_device().map(CpalDevice)
    }

    fn output_devices(&self) -> Result<Self::Devices, output::Error> {
        Ok(CpalDevices(self.0.output_devices()?))
    }

    fn id(&self) -> Self::Id {
        self.0.id()
    }
}

impl From<cpal::ErrorKind> for output::ErrorKind {
    fn from(value: cpal::ErrorKind) -> Self {
        match value {
            cpal::ErrorKind::DeviceBusy => output::ErrorKind::DeviceBusy,
            cpal::ErrorKind::DeviceChanged => output::ErrorKind::DeviceChanged,
            cpal::ErrorKind::DeviceNotAvailable => output::ErrorKind::DeviceNotAvailable,
            cpal::ErrorKind::HostUnavailable => output::ErrorKind::HostUnavailable,
            cpal::ErrorKind::InvalidInput => output::ErrorKind::InvalidInput,
            cpal::ErrorKind::PermissionDenied => output::ErrorKind::PermissionDenied,
            cpal::ErrorKind::RealtimeDenied => output::ErrorKind::RealtimeDenied,
            cpal::ErrorKind::ResourceExhausted => output::ErrorKind::ResourceExhausted,
            cpal::ErrorKind::StreamInvalidated => output::ErrorKind::StreamInvalidated,
            cpal::ErrorKind::UnsupportedConfig => output::ErrorKind::UnsupportedConfig,
            cpal::ErrorKind::UnsupportedOperation => output::ErrorKind::UnsupportedOperation,
            cpal::ErrorKind::Xrun => output::ErrorKind::Xrun,
            cpal::ErrorKind::BackendError => output::ErrorKind::BackendError,
            cpal::ErrorKind::Other => output::ErrorKind::Other,
            _ => output::ErrorKind::Other,
        }
    }
}

impl From<cpal::Error> for output::Error {
    fn from(value: cpal::Error) -> Self {
        let message = value.message().map(|s| s.to_string());
        Self {
            error_kind: value.kind().into(),
            message: message.map(|s| s.into()),
        }
    }
}
