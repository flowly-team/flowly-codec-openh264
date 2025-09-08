# flowly-codec-openh264

A Rust crate that provides bindings for OpenH264 in collaboration with the Flowly team. This library is designed to decode H.264 video streams efficiently.

## Features

- Bindings for OpenH264 decoder
- Support for asynchronous push and pull operations
- Integration with the Flowly framework

## Getting Started

### Installation

Add this crate as a dependency in your `Cargo.toml`:

```toml
[dependencies]
flowly-codec-openh264 = "0.1"
```

### Usage

Here's a basic example of how to use `flowly-codec-openh264`:

```rust
use flowly_codec_openh264::{Openh264Decoder, DecodedFrame};
use bytes::Bytes;

fn main() {
    let mut decoder = Openh264Decoder::<i32>::new(1);

    // Assume `encoded_data` is some H.264 encoded data and `timestamp` is the corresponding timestamp
    let encoded_data: Bytes = vec![0; 100].into(); // Replace with actual encoded data
    let timestamp = 0;
    let source_id = 0;

    tokio::block_on(async {
        decoder.push_data(encoded_data, timestamp, source_id).await.unwrap();

        if let Some(decoded_frame) = decoder.pull_frame().unwrap() {
            println!("Decoded frame dimensions: {}x{}", decoded_frame.width, decoded_frame.height);
        }
    });
}
```

## Documentation

You can find the full documentation at [https://docs.rs/flowly-mp4](https://docs.rs/flowly-mp4).

## License

This project is licensed under the MIT license. See the `LICENSE` file for more details.

## Contributing

Contributions are welcome! Please open an issue or submit a pull request on GitHub.

## Contact

For any questions or inquiries, please contact:

- **Email**: [flowly-team@example.com](mailto:flowly-team@example.com)
- **GitHub Issues**: [https://github.com/flowly-team/flowly-codec-openh264/issues](https://github.com/flowly-team/flowly-codec-openh264/issues)

We appreciate your interest and support!

