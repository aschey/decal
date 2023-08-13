use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use cpal::{Sample, SampleRate, SupportedStreamConfig, SupportedStreamConfigRange};

use super::{AudioBackend, DeviceTrait, HostTrait, StreamTrait};

pub struct MockStream {
    started: Arc<AtomicBool>,
}

pub struct MockDevices {
    index: usize,
    devices: Vec<MockDevice>,
}

impl Iterator for MockDevices {
    type Item = MockDevice;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.devices.len() {
            None
        } else {
            let device = &self.devices[self.index];
            self.index += 1;
            Some(device.clone())
        }
    }
}

pub struct MockSupportedOutputConfigs {
    index: usize,
    configs: Vec<SupportedStreamConfigRange>,
}

impl Iterator for MockSupportedOutputConfigs {
    type Item = SupportedStreamConfigRange;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.configs.len() {
            None
        } else {
            let config = &self.configs[self.index];
            self.index += 1;
            Some(config.clone())
        }
    }
}

impl StreamTrait for MockStream {
    fn play(&self) -> Result<(), cpal::PlayStreamError> {
        self.started.store(true, Ordering::SeqCst);
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

impl DeviceTrait for MockDevice {
    type Stream = MockStream;

    type SupportedOutputConfigs = MockSupportedOutputConfigs;

    fn default_output_config(
        &self,
    ) -> Result<SupportedStreamConfig, cpal::DefaultStreamConfigError> {
        Ok(self.default_config.clone())
    }

    fn name(&self) -> Result<String, cpal::DeviceNameError> {
        Ok(self.name.to_owned())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, cpal::SupportedStreamConfigsError> {
        Ok(MockSupportedOutputConfigs {
            index: 0,
            configs: [
                vec![SupportedStreamConfigRange::new(
                    self.default_config.channels(),
                    self.default_min_sample_rate,
                    self.default_max_sample_rate,
                    self.default_config.buffer_size().clone(),
                    self.default_config.sample_format(),
                )],
                self.additional_configs.clone(),
            ]
            .concat(),
        })
    }

    fn build_output_stream<T, D, E>(
        &self,
        _config: &cpal::StreamConfig,
        mut data_callback: D,
        _error_callback: E,
    ) -> Result<Self::Stream, cpal::BuildStreamError>
    where
        T: cpal::SizedSample,
        D: FnMut(&mut [T]) + Send + 'static,
        E: FnMut(cpal::StreamError) + Send + 'static,
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

        Ok(MockStream { started })
    }
}

#[derive(Clone)]
pub struct MockHost {
    pub default_device: MockDevice,
    pub additional_devices: Vec<MockDevice>,
}

impl HostTrait for MockHost {
    type Device = MockDevice;

    type Devices = MockDevices;

    fn default_output_device(&self) -> Option<Self::Device> {
        Some(self.default_device.clone())
    }

    fn output_devices(&self) -> Result<Self::Devices, cpal::DevicesError> {
        Ok(MockDevices {
            index: 0,
            devices: [
                vec![self.default_device.clone()],
                self.additional_devices.clone(),
            ]
            .concat(),
        })
    }
}

#[derive(Clone)]
pub struct MockOutput {
    pub default_host: MockHost,
}

impl AudioBackend for MockOutput {
    type Host = MockHost;

    type Stream = MockStream;

    type Device = MockDevice;

    fn default_host(&self) -> Self::Host {
        self.default_host.clone()
    }

    fn host_from_id(&self, _id: cpal::HostId) -> Result<Self::Host, cpal::HostUnavailable> {
        Ok(self.default_host.clone())
    }
}
