use std::cell::OnceCell;

use cubeb::{
    self, ChannelLayout, Context, DeviceFormat, DeviceId, DeviceInfo, DeviceType, MonoFrame,
    StereoFrame, StreamPrefs,
};
use cubeb_core::DevicePref;

use super::{
    AudioBackend, DecalSample, DefaultStreamConfigError, Device, Host, SampleFormat, SampleRate,
    Stream, StreamError, SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
};

thread_local! {
    static CONTEXT: OnceCell<Context> = OnceCell::default();
}

fn with_context<F, T>(f: F) -> T
where
    F: FnOnce(&Context) -> T,
{
    CONTEXT.with(|c| {
        let context = c.get_or_init(|| cubeb::init("context").unwrap());
        f(context)
    })
}

#[derive(Clone)]
pub struct CubebDevice {
    default_output_config: SupportedStreamConfig,
    name: String,
    output_configs: Vec<SupportedStreamConfigRange>,
    device_id: DeviceId,
}

impl CubebDevice {
    fn new(device: &DeviceInfo) -> Self {
        let default_format = device.default_format();

        let name = device
            .friendly_name()
            .map(|n| n.to_string())
            .unwrap_or_else(|| {
                device
                    .vendor_name()
                    .map(|n| n.to_string())
                    .unwrap_or_default()
            });

        let mut configs = vec![];
        if device
            .format()
            .contains(DeviceFormat::F32BE | DeviceFormat::F32LE)
        {
            configs.push(SupportedStreamConfigRange {
                channels: device.max_channels(),
                min_sample_rate: SampleRate(device.min_rate()),
                max_sample_rate: SampleRate(device.max_rate()),
                buffer_size: SupportedBufferSize::Unknown,
                sample_format: SampleFormat::F32,
            })
        }

        if device
            .format()
            .contains(DeviceFormat::S16BE | DeviceFormat::S16LE)
        {
            configs.push(SupportedStreamConfigRange {
                channels: device.max_channels(),
                min_sample_rate: SampleRate(device.min_rate()),
                max_sample_rate: SampleRate(device.max_rate()),
                buffer_size: SupportedBufferSize::Unknown,
                sample_format: SampleFormat::I16,
            })
        }

        Self {
            default_output_config: SupportedStreamConfig {
                channels: device.max_channels(),
                sample_rate: SampleRate(device.default_rate()),
                buffer_size: SupportedBufferSize::Unknown,
                sample_format: match default_format {
                    DeviceFormat::F32BE | DeviceFormat::F32LE => SampleFormat::F32,
                    DeviceFormat::S16BE | DeviceFormat::S16LE => SampleFormat::I16,
                    _ => SampleFormat::F32,
                },
            },
            output_configs: configs,
            name,
            device_id: device.devid(),
        }
    }
}

impl Device for CubebDevice {
    type SupportedOutputConfigs = Box<dyn Iterator<Item = SupportedStreamConfigRange>>;

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Ok(self.default_output_config.clone())
    }

    fn name(&self) -> Result<String, super::DeviceNameError> {
        Ok(self.name.clone())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, super::SupportedStreamConfigsError> {
        Ok(Box::new(self.output_configs.clone().into_iter()))
    }

    fn build_output_stream<T, D, E>(
        &self,
        config: &super::StreamConfig,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Result<Box<dyn Stream>, super::BuildStreamError>
    where
        T: DecalSample,
        D: FnMut(&mut [T]) + Send + Sync + 'static,
        E: FnMut(super::StreamError) + Send + Sync + 'static,
    {
        let mut buf = vec![T::EQUILIBRIUM; 1024];
        let params = cubeb::StreamParamsBuilder::new()
            .channels(config.channels)
            .format(match <T as DecalSample>::FORMAT {
                SampleFormat::I32 => cubeb::SampleFormat::S16NE,
                SampleFormat::F32 => cubeb::SampleFormat::Float32NE,
                _ => unimplemented!(),
            })
            .layout(if config.channels > 1 {
                ChannelLayout::STEREO
            } else {
                ChannelLayout::MONO
            })
            .rate(config.sample_rate.0)
            .prefs(StreamPrefs::NONE)
            .take();
        if config.channels > 1 {
            let mut builder = cubeb::StreamBuilder::<StereoFrame<T>>::new();
            builder
                .name("stream")
                .latency(1)
                .output(self.device_id, &params)
                .data_callback(move |_, output| {
                    let samples = output.len() * 2;
                    if buf.len() < samples {
                        buf.resize(samples, T::EQUILIBRIUM);
                    }
                    data_callback(&mut buf[..samples]);
                    let mut i = 0;
                    for frame in output.iter_mut() {
                        *frame = StereoFrame::<T> {
                            l: buf[i],
                            r: buf[i + 1],
                        };

                        i += 2;
                    }
                    output.len() as isize
                })
                .state_callback(move |state| {
                    match state {
                        cubeb::State::Started => {}
                        cubeb::State::Stopped => {}
                        cubeb::State::Drained => {}
                        cubeb::State::Error => {
                            error_callback(StreamError::DeviceNotAvailable);
                        }
                    };
                });
            let stream = with_context(move |ctx| {
                let res = builder.init(ctx);
                match res {
                    Ok(stream) => Ok(stream),
                    Err(e) => {
                        println!("{:?}", e.code());
                        Err(e)
                    }
                }
            })
            .unwrap();
            Ok(Box::new(CubebStream { stream }))
        } else {
            let mut builder = cubeb::StreamBuilder::<MonoFrame<T>>::new();
            builder
                .name("stream")
                .latency(1)
                .output(self.device_id, &params)
                .data_callback(move |_, output| {
                    let samples = output.len() * 2;
                    if buf.len() < samples {
                        buf.resize(samples, T::EQUILIBRIUM);
                    }
                    data_callback(&mut buf[..samples]);

                    for (i, frame) in output.iter_mut().enumerate() {
                        *frame = MonoFrame::<T> { m: buf[i] };
                    }
                    output.len() as isize
                });
            let stream = with_context(move |ctx| builder.init(ctx).unwrap());
            Ok(Box::new(CubebStream { stream }))
        }
    }
}

struct CubebStream<T> {
    stream: cubeb::Stream<T>,
}

impl<T> Stream for CubebStream<T> {
    fn play(&self) -> Result<(), super::PlayStreamError> {
        self.stream.start().unwrap();
        Ok(())
    }
}

pub struct CubebHost {}

impl Host for CubebHost {
    type Device = CubebDevice;

    type Devices = Box<dyn Iterator<Item = CubebDevice>>;

    fn default_output_device(&self) -> Option<Self::Device> {
        with_context(|ctx| {
            ctx.enumerate_devices(DeviceType::OUTPUT)
                .unwrap()
                .iter()
                .find(|d| {
                    d.preferred()
                        .contains(DevicePref::MULTIMEDIA | DevicePref::ALL)
                })
                .map(CubebDevice::new)
        })
    }

    fn output_devices(&self) -> Result<Self::Devices, super::DevicesError> {
        let devices: Vec<_> = with_context(|ctx| {
            ctx.enumerate_devices(DeviceType::OUTPUT)
                .unwrap()
                .iter()
                .map(CubebDevice::new)
                .collect()
        });
        Ok(Box::new(devices.into_iter()))
    }
}

#[derive(Clone, Default)]
pub struct CubebOutput {}

impl AudioBackend for CubebOutput {
    type Host = CubebHost;
    type Device = CubebDevice;

    fn default_host(&self) -> Self::Host {
        CubebHost {}
    }
}
