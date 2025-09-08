use std::collections::BinaryHeap;

use bytes::Bytes;
use flowly::{
    DataFrame, EncodedFrame, Fourcc, Frame, FrameFlags, FrameSource, MemBlock, Service, VideoFrame,
    spsc,
};
use futures::{Stream, executor::block_on};
use openh264::{
    decoder::{DecoderConfig, Flush},
    formats::YUVSource,
};

pub use error::Error;

mod error;

#[derive(Debug, Clone)]
pub struct DecodedFrame<S> {
    pub timestamp: u64,
    pub data: Bytes,
    pub width: u16,
    pub height: u16,
    pub flags: FrameFlags,
    source: S,
}

impl<S: FrameSource> DataFrame for DecodedFrame<S> {
    type Source = S;
    type Chunk = Bytes;

    fn source(&self) -> &Self::Source {
        &self.source
    }

    fn chunks(&self) -> impl Send + Iterator<Item = <Self::Chunk as MemBlock>::Ref<'_>> {
        std::iter::once(&self.data)
    }

    fn into_chunks(self) -> impl Send + Iterator<Item = Self::Chunk> {
        std::iter::once(self.data)
    }
}

impl<S: FrameSource> Frame for DecodedFrame<S> {
    fn timestamp(&self) -> u64 {
        self.timestamp
    }

    fn codec(&self) -> Fourcc {
        Fourcc::PIXEL_FORMAT_RGB888
    }

    fn flags(&self) -> FrameFlags {
        self.flags
    }
}

impl<S: FrameSource> VideoFrame for DecodedFrame<S> {
    fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    fn bit_depth(&self) -> u8 {
        8
    }
}

pub struct Openh264Decoder<S> {
    sender: spsc::Sender<(Bytes, u64, S)>,
    receiver: spsc::Receiver<Result<DecodedFrame<S>, Error>>,
    _handler: tokio::task::JoinHandle<Result<(), Error>>,
}

impl<S: Send + Default + 'static> Openh264Decoder<S> {
    pub fn new(_num_threads: u32) -> Self {
        let (sender, mut rx) = spsc::channel(2);
        let (mut tx, receiver) = spsc::channel(2);

        Self {
            sender,
            receiver,
            _handler: tokio::task::spawn_blocking(move || {
                let mut ts_heap: BinaryHeap<Entry<S>> = BinaryHeap::new();
                let decode_config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);

                let mut decoder = openh264::decoder::Decoder::with_api_config(
                    openh264::OpenH264API::from_source(),
                    decode_config,
                )?;

                while let Some(frame) = block_on(rx.recv()) {
                    if let Some(ts) = ts_heap.peek() {
                        if ts.0 != frame.1 {
                            ts_heap.push(Entry(frame.1, frame.2));
                        }
                    } else {
                        ts_heap.push(Entry(frame.1, frame.2));
                    }

                    let res = decoder
                        .decode(&frame.0)
                        .map_err(Error::from)
                        .map(|frame| frame.map(|frame| Self::make_frame(ts_heap.pop(), frame)));

                    if let Some(res) = res.transpose() {
                        if block_on(tx.send(res)).is_err() {
                            break;
                        }
                    }
                }

                match decoder.flush_remaining() {
                    Ok(remaining) => {
                        for frame in remaining {
                            if block_on(tx.send(Ok(Self::make_frame(ts_heap.pop(), frame))))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    Err(err) => log::error!("openh264::Decoder::flush_remaining error: {err}"),
                }

                Ok(())
            }),
        }
    }

    #[inline]
    pub async fn push_data(&mut self, data: Bytes, timestamp: u64, source: S) -> Result<(), Error> {
        self.sender
            .send((data, timestamp, source))
            .await
            .map_err(|_| Error::TrySendError)
    }

    #[inline]
    pub fn pull_frame(&mut self) -> Result<Option<DecodedFrame<S>>, Error> {
        self.receiver
            .try_recv()
            .map_err(|_| Error::TrySendError)?
            .transpose()
    }

    #[allow(clippy::uninit_vec)]
    fn make_frame(
        in_frame: Option<Entry<S>>,
        frame: openh264::decoder::DecodedYUV<'_>,
    ) -> DecodedFrame<S> {
        let dims = frame.dimensions();
        let mut data = Vec::with_capacity(dims.0 * dims.1 * 3);
        unsafe { data.set_len(dims.0 * dims.1 * 3) };

        frame.write_rgb8(&mut data);

        DecodedFrame {
            timestamp: in_frame.as_ref().map(|x| x.0).unwrap_or_default(),
            data: data.into(),
            width: dims.0 as _,
            height: dims.1 as _,
            source: in_frame.map(|x| x.1).unwrap_or_default(),
            flags: FrameFlags::VIDEO_STREAM,
        }
    }
}

impl<S: Send + Default + 'static> Default for Openh264Decoder<S> {
    fn default() -> Self {
        Self::new(0)
    }
}

impl<F: EncodedFrame + 'static> Service<F> for Openh264Decoder<F::Source> {
    type Out = Result<DecodedFrame<F::Source>, Error>;

    fn handle(&mut self, frame: F, _cx: &flowly::Context) -> impl Stream<Item = Self::Out> {
        async_stream::stream! {
            let ts = frame.timestamp();
            let source = frame.source().clone();

            for chunk in frame.into_chunks() {
                if let Err(err) = self.push_data(chunk.into_cpu_bytes(), ts, source.clone()).await {
                    yield Err(err);
                }
            }

            while let Some(res) = self.pull_frame().transpose() {
                yield res;
            }
        }
    }
}

struct Entry<S>(u64, S);

impl<S> std::ops::Deref for Entry<S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.1
    }
}

impl<S> PartialEq for Entry<S> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<S> Eq for Entry<S> {}

impl<S> PartialOrd for Entry<S> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(Self::cmp(self, other))
    }
}

impl<S> Ord for Entry<S> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.cmp(&self.0)
    }
}
