use super::{MockDevice, MockHost, MockOutput, OutputBuilder, RequestedOutputConfig};
use cpal::{SampleFormat, SampleRate, SupportedBufferSize, SupportedStreamConfig};
use std::vec;

#[test]
fn find_closest_config_default() {
    let output_builder = OutputBuilder::new(
        MockOutput {
            default_host: MockHost {
                default_device: MockDevice::new(
                    "test-device".to_owned(),
                    SupportedStreamConfig::new(
                        2,
                        SampleRate(44100),
                        SupportedBufferSize::Range { min: 0, max: 9999 },
                        SampleFormat::F32,
                    ),
                    SampleRate(1024),
                    SampleRate(192000),
                    vec![],
                ),
                additional_devices: vec![],
            },
        },
        Default::default(),
        move || {},
        |_| {},
    );
    let config = output_builder
        .find_closest_config(
            None,
            RequestedOutputConfig {
                sample_rate: None,
                channels: None,
                sample_format: None,
            },
        )
        .unwrap();

    assert_eq!(
        SupportedBufferSize::Range { min: 0, max: 9999 },
        *config.buffer_size()
    );
    assert_eq!(2, config.channels());
    assert_eq!(SampleFormat::F32, config.sample_format());
    assert_eq!(SampleRate(44100), config.sample_rate());
}

#[test]
fn find_closest_config_sample_rate() {
    let output_builder = OutputBuilder::new(
        MockOutput {
            default_host: MockHost {
                default_device: MockDevice::new(
                    "test-device".to_owned(),
                    SupportedStreamConfig::new(
                        2,
                        SampleRate(44100),
                        SupportedBufferSize::Range { min: 0, max: 9999 },
                        SampleFormat::F32,
                    ),
                    SampleRate(1024),
                    SampleRate(192000),
                    vec![],
                ),
                additional_devices: vec![],
            },
        },
        Default::default(),
        move || {},
        |_| {},
    );
    let config = output_builder
        .find_closest_config(
            None,
            RequestedOutputConfig {
                sample_rate: Some(SampleRate(48000)),
                channels: None,
                sample_format: None,
            },
        )
        .unwrap();

    assert_eq!(
        SupportedBufferSize::Range { min: 0, max: 9999 },
        *config.buffer_size()
    );
    assert_eq!(2, config.channels());
    assert_eq!(SampleFormat::F32, config.sample_format());
    assert_eq!(SampleRate(48000), config.sample_rate());
}

#[test]
fn find_closest_config_channel_mismatch() {
    let output_builder = OutputBuilder::new(
        MockOutput {
            default_host: MockHost {
                default_device: MockDevice::new(
                    "test-device".to_owned(),
                    SupportedStreamConfig::new(
                        2,
                        SampleRate(44100),
                        SupportedBufferSize::Range { min: 0, max: 9999 },
                        SampleFormat::F32,
                    ),
                    SampleRate(1024),
                    SampleRate(192000),
                    vec![],
                ),
                additional_devices: vec![MockDevice::new(
                    "second-device".to_owned(),
                    SupportedStreamConfig::new(
                        1,
                        SampleRate(48000),
                        SupportedBufferSize::Range { min: 0, max: 9999 },
                        SampleFormat::F32,
                    ),
                    SampleRate(1024),
                    SampleRate(192000),
                    vec![],
                )],
            },
        },
        Default::default(),
        move || {},
        |_| {},
    );
    let config = output_builder
        .find_closest_config(
            None,
            RequestedOutputConfig {
                sample_rate: Some(SampleRate(48000)),
                channels: Some(2),
                sample_format: None,
            },
        )
        .unwrap();

    assert_eq!(
        SupportedBufferSize::Range { min: 0, max: 9999 },
        *config.buffer_size()
    );
    assert_eq!(2, config.channels());
    assert_eq!(SampleFormat::F32, config.sample_format());
    assert_eq!(SampleRate(48000), config.sample_rate());
}

#[test]
fn find_closest_config_sample_rate_mismatch() {
    let output_builder = OutputBuilder::new(
        MockOutput {
            default_host: MockHost {
                default_device: MockDevice::new(
                    "test-device".to_owned(),
                    SupportedStreamConfig::new(
                        2,
                        SampleRate(44100),
                        SupportedBufferSize::Range { min: 0, max: 9999 },
                        SampleFormat::F32,
                    ),
                    SampleRate(44100),
                    SampleRate(44100),
                    vec![],
                ),
                additional_devices: vec![MockDevice::new(
                    "second-device".to_owned(),
                    SupportedStreamConfig::new(
                        1,
                        SampleRate(48000),
                        SupportedBufferSize::Range { min: 0, max: 9999 },
                        SampleFormat::F32,
                    ),
                    SampleRate(1024),
                    SampleRate(192000),
                    vec![],
                )],
            },
        },
        Default::default(),
        move || {},
        |_| {},
    );
    let config = output_builder
        .find_closest_config(
            None,
            RequestedOutputConfig {
                sample_rate: Some(SampleRate(48000)),
                channels: Some(2),
                sample_format: None,
            },
        )
        .unwrap();

    assert_eq!(
        SupportedBufferSize::Range { min: 0, max: 9999 },
        *config.buffer_size()
    );
    assert_eq!(2, config.channels());
    assert_eq!(SampleFormat::F32, config.sample_format());
    assert_eq!(SampleRate(44100), config.sample_rate());
}

#[test]
fn find_closest_config_non_default_device() {
    let output_builder = OutputBuilder::new(
        MockOutput {
            default_host: MockHost {
                default_device: MockDevice::new(
                    "test-device".to_owned(),
                    SupportedStreamConfig::new(
                        2,
                        SampleRate(44100),
                        SupportedBufferSize::Range { min: 0, max: 9999 },
                        SampleFormat::F32,
                    ),
                    SampleRate(1024),
                    SampleRate(192000),
                    vec![],
                ),
                additional_devices: vec![MockDevice::new(
                    "second-device".to_owned(),
                    SupportedStreamConfig::new(
                        2,
                        SampleRate(48000),
                        SupportedBufferSize::Range { min: 0, max: 9999 },
                        SampleFormat::F32,
                    ),
                    SampleRate(1024),
                    SampleRate(192000),
                    vec![],
                )],
            },
        },
        Default::default(),
        move || {},
        |_| {},
    );
    let config = output_builder
        .find_closest_config(
            Some("second-device"),
            RequestedOutputConfig {
                sample_rate: None,
                channels: None,
                sample_format: None,
            },
        )
        .unwrap();

    assert_eq!(
        SupportedBufferSize::Range { min: 0, max: 9999 },
        *config.buffer_size()
    );
    assert_eq!(2, config.channels());
    assert_eq!(SampleFormat::F32, config.sample_format());
    assert_eq!(SampleRate(48000), config.sample_rate());
}
