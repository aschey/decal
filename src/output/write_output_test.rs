use std::vec;

use cpal::{SampleFormat, SampleRate, SupportedBufferSize, SupportedStreamConfig};

use super::{MockDevice, MockHost, MockOutput, OutputBuilder};

#[test]
fn test_write_output() {
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

    let mut output = output_builder
        .new_output::<f32>(None, output_builder.default_output_config().unwrap())
        .unwrap();

    output.start().unwrap();
    output.write_blocking(&[1.0; 1024]).unwrap();
    let written = output.device().trigger_callback();
    assert_eq!([1.0; 1024], written);
}
