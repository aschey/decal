use std::mem;

use rtaudio::NativeFormats;

use crate::{
    ChannelCount, SampleRate,
    output::{
        AudioBackend, Device, Host, SampleFormat, Stream, StreamError, SupportedBufferSize,
        SupportedStreamConfig, SupportedStreamConfigRange,
    },
};

pub struct RtAudioHost(rtaudio::Host);

unsafe impl Send for RtAudioHost {}

unsafe impl Sync for RtAudioHost {}

#[derive(Clone, Default)]
pub struct RtAudioOutput {}

pub struct RtAudioStream(rtaudio::StreamHandle);

pub struct RtAudioDevice(rtaudio::DeviceInfo);

impl Device for RtAudioDevice {
    type SupportedOutputConfigs = Box<dyn Iterator<Item = SupportedStreamConfigRange>>;

    fn default_output_config(
        &self,
    ) -> Result<super::SupportedStreamConfig, super::DefaultStreamConfigError> {
        Ok(SupportedStreamConfig {
            channels: ChannelCount(self.0.output_channels as u16),
            buffer_size: SupportedBufferSize::Unknown,
            sample_format: SampleFormat::F32,
            sample_rate: SampleRate(self.0.preferred_sample_rate),
        })
    }

    fn name(&self) -> Result<String, super::DeviceNameError> {
        Ok(self.0.name().to_string())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, super::SupportedStreamConfigsError> {
        let formats: Vec<_> = self
            .0
            .native_formats
            .iter()
            .map(|f| SupportedStreamConfigRange {
                channels: ChannelCount(self.0.output_channels as u16),
                buffer_size: SupportedBufferSize::Unknown,
                min_sample_rate: SampleRate(*self.0.sample_rates.iter().min().unwrap()),
                max_sample_rate: SampleRate(*self.0.sample_rates.iter().max().unwrap()),
                sample_format: match f {
                    NativeFormats::SINT8 => SampleFormat::I8,
                    NativeFormats::SINT16 => SampleFormat::I16,
                    NativeFormats::SINT24 => SampleFormat::I24,
                    NativeFormats::SINT32 => SampleFormat::I32,
                    NativeFormats::FLOAT32 => SampleFormat::F32,
                    NativeFormats::FLOAT64 => SampleFormat::F64,
                    _ => unreachable!(),
                },
            })
            .collect();
        Ok(Box::new(formats.into_iter()))
    }

    fn build_output_stream<T, D, E>(
        &mut self,
        config: &super::StreamConfig,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Result<Box<dyn super::Stream>, super::BuildStreamError>
    where
        T: super::DecalSample,
        D: FnMut(&mut [T]) + Send + Sync + 'static,
        E: FnMut(super::StreamError) + Clone + Send + Sync + 'static,
    {
        let mut stream = rtaudio::Host::default()
            .open_stream(&rtaudio::StreamConfig {
                output_device: Some(rtaudio::DeviceParams {
                    device_id: Some(self.0.id.clone()),
                    num_channels: Some(config.channels.0 as u32),
                    ..Default::default()
                }),
                sample_rate: Some(config.sample_rate.0),
                sample_format: match <T as super::DecalSample>::FORMAT {
                    SampleFormat::I8 => rtaudio::SampleFormat::SInt8,
                    SampleFormat::I16 => rtaudio::SampleFormat::SInt16,
                    SampleFormat::I24 => rtaudio::SampleFormat::SInt24,
                    SampleFormat::I32 => rtaudio::SampleFormat::SInt32,
                    SampleFormat::F32 => rtaudio::SampleFormat::Float32,
                    SampleFormat::F64 => rtaudio::SampleFormat::Float64,
                    _ => rtaudio::SampleFormat::Float32,
                },
                ..Default::default()
            })
            .unwrap();

        stream
            .start(
                move |buffers: rtaudio::Buffers<'_>,
                      _info: &rtaudio::StreamInfo,
                      status: rtaudio::StreamStatus| {
                    if status.intersects(rtaudio::StreamStatus::INPUT_OVERFLOW) {
                        error_callback(StreamError::InputOverflow);
                    }
                    if status.intersects(rtaudio::StreamStatus::OUTPUT_UNDERFLOW) {
                        error_callback(StreamError::BufferUnderrun);
                    }

                    match <T as super::DecalSample>::FORMAT {
                        SampleFormat::I8 => {
                            if let rtaudio::Buffers::SInt8 { output, .. } = buffers {
                                unsafe {
                                    data_callback(mem::transmute::<&mut [i8], &mut [T]>(output))
                                };
                            } else {
                                error_callback(StreamError::InvalidConfiguration(
                                    "expected SInt8 format".to_string(),
                                ))
                            }
                        }
                        SampleFormat::I16 => {
                            if let rtaudio::Buffers::SInt16 { output, .. } = buffers {
                                unsafe {
                                    data_callback(mem::transmute::<&mut [i16], &mut [T]>(output))
                                };
                            } else {
                                error_callback(StreamError::InvalidConfiguration(
                                    "expected SInt16 format".to_string(),
                                ))
                            }
                        }
                        SampleFormat::I24 => {
                            if let rtaudio::Buffers::SInt24 { output, .. } = buffers {
                                unsafe {
                                    data_callback(mem::transmute::<&mut [u8], &mut [T]>(output))
                                };
                            } else {
                                error_callback(StreamError::InvalidConfiguration(
                                    "expected SInt24 format".to_string(),
                                ))
                            }
                        }
                        SampleFormat::I32 => {
                            if let rtaudio::Buffers::SInt32 { output, .. } = buffers {
                                unsafe {
                                    data_callback(mem::transmute::<&mut [i32], &mut [T]>(output))
                                };
                            } else {
                                error_callback(StreamError::InvalidConfiguration(
                                    "expected SInt32 format".to_string(),
                                ))
                            }
                        }
                        SampleFormat::F32 => {
                            if let rtaudio::Buffers::Float32 { output, .. } = buffers {
                                unsafe {
                                    data_callback(mem::transmute::<&mut [f32], &mut [T]>(output))
                                };
                            } else {
                                error_callback(StreamError::InvalidConfiguration(
                                    "expected F32 format".to_string(),
                                ))
                            }
                        }
                        SampleFormat::F64 => {
                            if let rtaudio::Buffers::Float64 { output, .. } = buffers {
                                unsafe {
                                    data_callback(mem::transmute::<&mut [f64], &mut [T]>(output))
                                };
                            } else {
                                error_callback(StreamError::InvalidConfiguration(
                                    "expected F64 format".to_string(),
                                ))
                            }
                        }
                        _ => unimplemented!(),
                    }
                },
            )
            .unwrap();

        Ok(Box::new(RtAudioStream(stream)))
    }
}

impl Stream for RtAudioStream {
    fn play(&mut self) -> Result<(), super::PlayStreamError> {
        Ok(())
    }

    fn stop(&mut self) -> Result<(), super::PlayStreamError> {
        self.0.stop();
        Ok(())
    }
}

impl Host for RtAudioHost {
    type Device = RtAudioDevice;

    type Devices = Box<dyn Iterator<Item = RtAudioDevice>>;

    fn default_output_device(&self) -> Option<Self::Device> {
        self.0
            .default_output_device_index()
            .map(|i| RtAudioDevice(self.0.devices()[i].clone()))
    }

    fn output_devices(&self) -> Result<Self::Devices, super::DevicesError> {
        let devices: Vec<_> = self
            .0
            .devices()
            .iter()
            .filter(|d| d.output_channels > 0)
            .map(|d| RtAudioDevice(d.clone()))
            .collect();
        Ok(Box::new(devices.into_iter()))
    }
}

impl AudioBackend for RtAudioOutput {
    type Host = RtAudioHost;
    type Device = RtAudioDevice;

    fn default_host(&self) -> Self::Host {
        RtAudioHost(rtaudio::Host::default())
    }
}
