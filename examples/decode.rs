use std::{path::PathBuf, pin::pin};

use bytes::Bytes;
use flowly::{Context, Service, ServiceExt};
use flowly_codec_openh264::Openh264Decoder;
use flowly_flv::FlvDemuxer;

use futures::TryStreamExt;

#[tokio::main]
async fn main() {
    let mut flow = flowly::flow()
        .flow(flowly::io::file::FileReader::new(8192))
        .flow(FlvDemuxer::<Bytes, _>::default())
        .flow(Openh264Decoder::default());

    let cx = Context::new();

    let mut stream = pin!(flow.handle(PathBuf::from("/home/andrey/demo/h264/test.flv"), &cx));

    while let Some(frame) = stream.try_next().await.unwrap() {
        println!("got frame {}", frame.timestamp);
    }
}
