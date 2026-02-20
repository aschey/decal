use std::mem;

use rtaudio::NativeFormats;

use crate::{
    ChannelCount, SampleRate,
    output::{
        BuildStreamError, DecalSample, DefaultStreamConfigError, Device, DeviceNameError,
        DevicesError, Host, PlayStreamError, SampleFormat, Stream, StreamConfig, StreamError,
        SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
        SupportedStreamConfigsError,
    },
};

#[derive(Default)]
pub struct RtAudioHost(rtaudio::Host);

unsafe impl Send for RtAudioHost {}

unsafe impl Sync for RtAudioHost {}

impl RtAudioHost {
    pub fn host_from_id(&self, id: rtaudio::Api) -> Result<Self, super::HostUnavailableError> {
        Ok(RtAudioHost(rtaudio::Host::new(id).unwrap()))
    }
}

pub struct RtAudioStream(rtaudio::StreamHandle);

pub struct RtAudioDevice(rtaudio::DeviceInfo);

impl Device for RtAudioDevice {
    type SupportedOutputConfigs = Box<dyn Iterator<Item = SupportedStreamConfigRange>>;

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Ok(SupportedStreamConfig {
            channels: ChannelCount(self.0.output_channels as u16),
            buffer_size: SupportedBufferSize::Unknown,
            sample_format: SampleFormat::F32,
            sample_rate: SampleRate(self.0.preferred_sample_rate),
        })
    }

    fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.0.name().to_string())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
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
        config: &StreamConfig,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Result<Box<dyn Stream>, BuildStreamError>
    where
        T: DecalSample,
        D: FnMut(&mut [T]) + Send + Sync + 'static,
        E: FnMut(StreamError) + Clone + Send + Sync + 'static,
    {
        let mut stream = rtaudio::Host::default()
            .open_stream(&rtaudio::StreamConfig {
                output_device: Some(rtaudio::DeviceParams {
                    device_id: Some(self.0.id.clone()),
                    num_channels: Some(config.channels.0 as u32),
                    ..Default::default()
                }),
                sample_rate: Some(config.sample_rate.0),
                sample_format: match <T as DecalSample>::FORMAT {
                    SampleFormat::I8 => rtaudio::SampleFormat::SInt8,
                    SampleFormat::I16 => rtaudio::SampleFormat::SInt16,
                    SampleFormat::I32 => rtaudio::SampleFormat::SInt32,
                    SampleFormat::F32 => rtaudio::SampleFormat::Float32,
                    SampleFormat::F64 => rtaudio::SampleFormat::Float64,
                    _ => unreachable!(),
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

                    // SAFETY: T will always match the numeric type since we're checking its FORMAT property
                    match buffers {
                        rtaudio::Buffers::SInt8 { output, .. } => {
                            if <i8 as DecalSample>::FORMAT == <T as DecalSample>::FORMAT {
                                unsafe {
                                    data_callback(mem::transmute::<&mut [i8], &mut [T]>(output));
                                }
                                return;
                            }
                        }
                        rtaudio::Buffers::SInt16 { output, .. } => {
                            if <i16 as DecalSample>::FORMAT == <T as DecalSample>::FORMAT {
                                unsafe {
                                    data_callback(mem::transmute::<&mut [i16], &mut [T]>(output));
                                }
                                return;
                            }
                        }
                        rtaudio::Buffers::SInt32 { output, .. } => {
                            if <i32 as DecalSample>::FORMAT == <T as DecalSample>::FORMAT {
                                unsafe {
                                    data_callback(mem::transmute::<&mut [i32], &mut [T]>(output));
                                }
                                return;
                            }
                        }
                        rtaudio::Buffers::Float32 { output, .. } => {
                            if <f32 as DecalSample>::FORMAT == <T as DecalSample>::FORMAT {
                                unsafe {
                                    data_callback(mem::transmute::<&mut [f32], &mut [T]>(output));
                                }
                                return;
                            }
                        }
                        rtaudio::Buffers::Float64 { output, .. } => {
                            if <f64 as DecalSample>::FORMAT == <T as DecalSample>::FORMAT {
                                unsafe {
                                    data_callback(mem::transmute::<&mut [f64], &mut [T]>(output));
                                }
                                return;
                            }
                        }
                        // unsupported
                        rtaudio::Buffers::SInt24 { .. } => {}
                    }

                    error_callback(StreamError::InvalidConfiguration(
                        "Sample type does not match output buffer".to_string(),
                    ));
                },
            )
            .unwrap();

        Ok(Box::new(RtAudioStream(stream)))
    }
}

impl Stream for RtAudioStream {
    fn play(&mut self) -> Result<(), PlayStreamError> {
        Ok(())
    }

    fn stop(&mut self) -> Result<(), PlayStreamError> {
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

    fn output_devices(&self) -> Result<Self::Devices, DevicesError> {
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
