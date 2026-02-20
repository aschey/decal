use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use super::{
    BuildStreamError, DecalSample, DefaultStreamConfigError, Device, DeviceNameError, DevicesError,
    Host, PlayStreamError, Stream, StreamConfig, StreamError, SupportedStreamConfig,
    SupportedStreamConfigRange, SupportedStreamConfigsError,
};
use crate::output::{SampleFormat, SupportedBufferSize};
use crate::{ChannelCount, SampleRate};
use dasp::Sample;

pub struct MockStream {
    started: Arc<AtomicBool>,
}

impl Stream for MockStream {
    fn play(&mut self) -> Result<(), PlayStreamError> {
        self.started.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), PlayStreamError> {
        self.started.store(false, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Clone)]
pub struct MockDevice {
    pub name: String,
    pub default_config: SupportedStreamConfig,
    pub default_min_sample_rate: SampleRate,
    pub default_max_sample_rate: SampleRate,
    pub additional_configs: Vec<SupportedStreamConfigRange>,
    stream_tx: Arc<RwLock<mpsc::SyncSender<()>>>,
    data_rx: Arc<Mutex<mpsc::Receiver<[f32; 1024]>>>,
}

impl MockDevice {
    pub fn new(
        name: String,
        default_config: SupportedStreamConfig,
        default_min_sample_rate: SampleRate,
        default_max_sample_rate: SampleRate,
        additional_configs: Vec<SupportedStreamConfigRange>,
    ) -> Self {
        let (stream_tx, _) = mpsc::sync_channel(0);
        let (_, data_rx) = mpsc::channel();

        Self {
            name,
            default_config,
            default_min_sample_rate,
            default_max_sample_rate,
            additional_configs,
            stream_tx: Arc::new(RwLock::new(stream_tx)),
            data_rx: Arc::new(Mutex::new(data_rx)),
        }
    }

    pub fn trigger_callback(&self) -> [f32; 1024] {
        self.stream_tx.read().unwrap().send(()).unwrap();
        self.data_rx.lock().unwrap().recv().unwrap()
    }
}

impl Device for MockDevice {
    type SupportedOutputConfigs = Box<dyn Iterator<Item = SupportedStreamConfigRange>>;

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Ok(self.default_config.clone())
    }

    fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.name.to_owned())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Ok(Box::new(
            [
                vec![SupportedStreamConfigRange {
                    channels: self.default_config.channels,
                    min_sample_rate: self.default_min_sample_rate,
                    max_sample_rate: self.default_max_sample_rate,
                    buffer_size: self.default_config.buffer_size.clone(),
                    sample_format: self.default_config.sample_format,
                }],
                self.additional_configs.clone(),
            ]
            .concat()
            .into_iter(),
        ))
    }

    fn build_output_stream<T, D, E>(
        &mut self,
        _config: &StreamConfig,
        mut data_callback: D,
        _error_callback: E,
    ) -> Result<Box<dyn Stream>, BuildStreamError>
    where
        T: DecalSample,
        D: FnMut(&mut [T]) + Send + 'static,
        E: FnMut(StreamError) + Send + Sync + 'static,
    {
        let started = Arc::new(AtomicBool::new(false));

        let (stream_tx, stream_rx) = mpsc::sync_channel(10000);
        *self.stream_tx.write().unwrap() = stream_tx;
        let (data_tx, data_rx) = mpsc::channel();
        *self.data_rx.lock().unwrap() = data_rx;
        thread::spawn(move || {
            const BUF_SIZE: usize = 1024;
            let mut buf = [T::EQUILIBRIUM; BUF_SIZE];

            while let Ok(()) = stream_rx.recv() {
                data_callback(&mut buf);
                data_tx
                    .send(buf.map(|b| b.to_float_sample().to_sample()))
                    .unwrap();
            }
        });

        Ok(Box::new(MockStream { started }))
    }
}

#[derive(Clone)]
pub struct MockHost {
    pub default_device: MockDevice,
    pub additional_devices: Vec<MockDevice>,
}

impl Default for MockHost {
    fn default() -> Self {
        Self {
            default_device: MockDevice::new(
                "".to_owned(),
                SupportedStreamConfig {
                    channels: ChannelCount(2),
                    sample_rate: SampleRate(44100),
                    buffer_size: SupportedBufferSize::Range { min: 0, max: 9999 },
                    sample_format: SampleFormat::F32,
                },
                SampleRate(1024),
                SampleRate(192000),
                vec![],
            ),
            additional_devices: vec![],
        }
    }
}

impl Host for MockHost {
    type Device = MockDevice;
    type Id = ();
    type Devices = Box<dyn Iterator<Item = MockDevice>>;

    fn from_id(_id: Self::Id) -> Result<Self, super::HostUnavailableError> {
        Ok(Self::default())
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        Some(self.default_device.clone())
    }

    fn output_devices(&self) -> Result<Self::Devices, DevicesError> {
        Ok(Box::new(
            [
                vec![self.default_device.clone()],
                self.additional_devices.clone(),
            ]
            .concat()
            .into_iter(),
        ))
    }

    fn id(&self) -> Self::Id {}
}
