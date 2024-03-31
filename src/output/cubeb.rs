use std::cell::OnceCell;
use std::sync::atomic::{AtomicBool, Ordering};

use cubeb::{
    self, ChannelLayout, Context, DeviceFormat, DeviceId, DeviceInfo, DeviceState, DeviceType,
    MonoFrame, StereoFrame, StreamPrefs,
};
use cubeb_core::DevicePref;

use super::{
    AudioBackend, DecalSample, DefaultStreamConfigError, Device, Host, SampleFormat, SampleRate,
    Stream, StreamConfig, StreamError, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange,
};

thread_local! {
    static CONTEXT: OnceCell<Context> = OnceCell::default();
    #[cfg(windows)]
    static COM_INIT: AtomicBool = const { std::sync::atomic::AtomicBool::new(false) };
}

fn with_context<F, T>(f: F) -> T
where
    F: FnOnce(&Context) -> T,
{
    #[cfg(windows)]
    COM_INIT.with(|init| {
        if !init.load(Ordering::Relaxed) {
            unsafe {
                let code = windows_sys::Win32::System::Com::CoInitializeEx(
                    std::ptr::null(),
                    windows_sys::Win32::System::Com::COINIT_MULTITHREADED as u32,
                );
                // S_FALSE
                if code == 0x1 {
                    tracing::error!("COM library was already initialized");
                } else if code != 0x0 {
                    tracing::error!("COM initialization error. HRESULT {code}");
                }
            }
            init.store(true, Ordering::Relaxed);
        }
    });
    CONTEXT.with(|c| {
        let context =
            c.get_or_init(|| cubeb::init("context").expect("failed to initialize context"));
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
            .or_else(|| device.vendor_name().map(|n| n.to_string()))
            .unwrap_or_else(|| "N/A".to_string());

        let mut configs = vec![];
        if device
            .format()
            .intersects(DeviceFormat::F32BE | DeviceFormat::F32LE)
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
            .intersects(DeviceFormat::S16BE | DeviceFormat::S16LE)
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

    fn get_stream<S: DecalSample, T, D, E>(
        &self,
        config: &StreamConfig,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Box<dyn Stream>
    where
        T: 'static,
        D: FnMut(&mut [T]) + Send + Sync + 'static,
        E: FnMut(StreamError) + Clone + Send + Sync + 'static,
    {
        let params = cubeb::StreamParamsBuilder::new()
            .channels(config.channels)
            .format(match <S as DecalSample>::FORMAT {
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
        let latency = with_context(|c| c.min_latency(&params).unwrap());
        #[cfg(target_os = "macos")]
        let mut error_callback_ = error_callback.clone();
        let mut builder = cubeb::StreamBuilder::<T>::new();
        builder
            .name("stream")
            .latency(latency)
            .output(self.device_id, &params)
            .data_callback(move |_, output| {
                data_callback(output);
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
        // Device changed hook is only implemented on mac
        #[cfg(target_os = "macos")]
        builder.device_changed_cb(move || {
            error_callback_(StreamError::DeviceNotAvailable);
        });
        let stream = with_context(move |ctx| {
            let stream = builder.init(ctx).unwrap();
            // On Windows, if we don't call start here and wait to call it when play() is invoked,
            // cubeb throws an error. If we don't call start here and call stop() before
            // calling start() later, the error doesn't happen. No idea why...
            stream.start().unwrap();
            stream
        });
        Box::new(CubebStream {
            stream,
            started: AtomicBool::new(true),
        })
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
        config: &StreamConfig,
        mut data_callback: D,
        error_callback: E,
    ) -> Result<Box<dyn Stream>, super::BuildStreamError>
    where
        T: DecalSample,
        D: FnMut(&mut [T]) + Send + Sync + 'static,
        E: FnMut(super::StreamError) + Clone + Send + Sync + 'static,
    {
        let mut buf = vec![T::EQUILIBRIUM; 1024];

        if config.channels > 1 {
            Ok(self.get_stream::<T, _, _, _>(
                config,
                move |output| {
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
                },
                error_callback,
            ))
        } else {
            Ok(self.get_stream::<T, _, _, _>(
                config,
                move |output| {
                    let samples = output.len() * 2;
                    if buf.len() < samples {
                        buf.resize(samples, T::EQUILIBRIUM);
                    }
                    data_callback(&mut buf[..samples]);

                    for (i, frame) in output.iter_mut().enumerate() {
                        *frame = MonoFrame::<T> { m: buf[i] };
                    }
                },
                error_callback,
            ))
        }
    }
}

struct CubebStream<T> {
    stream: cubeb::Stream<T>,
    started: AtomicBool,
}

impl<T> Drop for CubebStream<T> {
    fn drop(&mut self) {
        #[cfg(windows)]
        COM_INIT.with(|init| {
            if init.load(Ordering::Relaxed) {
                unsafe {
                    windows_sys::Win32::System::Com::CoUninitialize();
                }
                init.store(false, Ordering::Relaxed);
            }
        });
    }
}

impl<T> Stream for CubebStream<T> {
    fn play(&self) -> Result<(), super::PlayStreamError> {
        if !self.started.swap(true, Ordering::SeqCst) {
            self.stream.start().unwrap();
        }

        Ok(())
    }

    fn stop(&self) -> Result<(), super::PlayStreamError> {
        if self.started.swap(false, Ordering::SeqCst) {
            self.stream.stop().unwrap();
        }
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
                        .intersects(DevicePref::MULTIMEDIA | DevicePref::ALL)
                        && d.state() == DeviceState::Enabled
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
