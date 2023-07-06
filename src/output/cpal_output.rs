use super::{AudioBackend, DeviceTrait, HostTrait, StreamTrait};
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

impl StreamTrait for CpalStream {
    fn play(&self) -> Result<(), cpal::PlayStreamError> {
        self.0.play()
    }
}

impl DeviceTrait for CpalDevice {
    type Stream = CpalStream;

    type SupportedOutputConfigs = cpal::SupportedOutputConfigs;

    fn default_output_config(
        &self,
    ) -> Result<cpal::SupportedStreamConfig, cpal::DefaultStreamConfigError> {
        self.0.default_output_config()
    }

    fn name(&self) -> Result<String, cpal::DeviceNameError> {
        self.0.name()
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, cpal::SupportedStreamConfigsError> {
        self.0.supported_output_configs()
    }

    fn build_output_stream<T, D, E>(
        &self,
        config: &cpal::StreamConfig,
        mut data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, cpal::BuildStreamError>
    where
        T: cpal::SizedSample,
        D: FnMut(&mut [T]) + Send + 'static,
        E: FnMut(cpal::StreamError) + Send + 'static,
    {
        self.0
            .build_output_stream(
                config,
                move |data: &mut [T], _| data_callback(data),
                error_callback,
                None,
            )
            .map(CpalStream)
    }
}

impl HostTrait for CpalHost {
    type Device = CpalDevice;

    type Devices = CpalDevices;

    fn default_output_device(&self) -> Option<Self::Device> {
        self.0.default_output_device().map(CpalDevice)
    }

    fn output_devices(&self) -> Result<Self::Devices, cpal::DevicesError> {
        self.0.output_devices().map(CpalDevices)
    }
}

impl AudioBackend for CpalOutput {
    type Host = CpalHost;
    type Stream = CpalStream;
    type Device = CpalDevice;

    fn default_host(&self) -> Self::Host {
        CpalHost(cpal::default_host())
    }

    fn host_from_id(&self, id: cpal::HostId) -> Result<Self::Host, cpal::HostUnavailable> {
        cpal::host_from_id(id).map(CpalHost)
    }
}
