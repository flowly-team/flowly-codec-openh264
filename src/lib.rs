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

pub struct Openh264Decoder<I: EncodedFrame> {
    sender: spsc::Sender<I>,
    receiver: spsc::Receiver<Result<DecodedFrame<I::Source>, Error>>,
    _handler: tokio::task::JoinHandle<Result<(), Error>>,
}

impl<I: EncodedFrame + 'static> Openh264Decoder<I> {
    pub fn new(_num_threads: u32) -> Self {
        let (sender, mut rx) = spsc::channel(2);
        let (mut tx, receiver) = spsc::channel(2);

        Self {
            sender,
            receiver,
            _handler: tokio::task::spawn_blocking(move || {
                let mut ts_heap: BinaryHeap<Entry<I>> = BinaryHeap::new();
                let decode_config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);

                let mut decoder = openh264::decoder::Decoder::with_api_config(
                    unsafe {
                        openh264::OpenH264API::from_blob_path_unchecked("/usr/lib/libopenh264.so")
                            .unwrap()
                    },
                    decode_config,
                )?;

                while let Some(frame) = block_on(rx.recv()) {
                    // if frame.has_params() {
                    //     for ps in frame.params() {
                    //         let res = decoder
                    //             .decode(ps.as_ref())
                    //             .map_err(Error::<flowly::Void>::from)
                    //             .map(|frame| {
                    //                 frame.map(|frame| Self::make_frame(ts_heap.pop(), frame))
                    //             });

                    //         if let Some(res) = res.transpose() {
                    //             if block_on(tx.send(res)).is_err() {
                    //                 break;
                    //             }
                    //         }
                    //     }
                    // }

                    if let Some(ts) = ts_heap.peek() {
                        if ts.timestamp() != frame.timestamp() {
                            ts_heap.push(Entry(frame.clone()));
                        }
                    } else {
                        ts_heap.push(Entry(frame.clone()));
                    }

                    for chunk in frame.chunks() {
                        let res = decoder
                            .decode(chunk.map_to_cpu())
                            .map_err(Error::from)
                            .map(|frame| frame.map(|frame| Self::make_frame(ts_heap.pop(), frame)));

                        if let Some(res) = res.transpose() {
                            if block_on(tx.send(res)).is_err() {
                                break;
                            }
                        }
                    }
                }

                // match decoder.flush_remaining() {
                //     Ok(remaining) => {
                //         for frame in remaining {
                //             if block_on(tx.send(Ok(Self::make_frame(ts_heap.pop(), frame))))
                //                 .is_err()
                //             {
                //                 break;
                //             }
                //         }
                //     }
                //     Err(err) => log::error!("openh264::Decoder::flush_remaining error: {err}"),
                // }

                Ok(())
            }),
        }
    }

    #[allow(clippy::uninit_vec)]
    fn make_frame(
        in_frame: Option<Entry<I>>,
        frame: openh264::decoder::DecodedYUV<'_>,
    ) -> DecodedFrame<I::Source> {
        let dims = frame.dimensions();
        let mut data = Vec::with_capacity(dims.0 * dims.1 * 3);
        unsafe { data.set_len(dims.0 * dims.1 * 3) };

        frame.write_rgb8(&mut data);

        let mut flags = in_frame
            .as_ref()
            .map(|x| x.flags())
            .unwrap_or(FrameFlags::VIDEO_STREAM);

        flags.set(FrameFlags::ENCODED, false);
        flags.set(FrameFlags::ANNEXB, false);

        DecodedFrame {
            timestamp: in_frame.as_ref().map(|x| x.timestamp()).unwrap_or_default(),
            data: data.into(),
            width: dims.0 as _,
            height: dims.1 as _,
            source: in_frame
                .as_ref()
                .map(|x| x.source().clone())
                .unwrap_or_default(),
            flags,
        }
    }
}

impl<I: EncodedFrame + 'static> Default for Openh264Decoder<I> {
    fn default() -> Self {
        Self::new(0)
    }
}
impl<F: EncodedFrame + 'static> Service<F> for Openh264Decoder<F> {
    type Out = Result<DecodedFrame<F::Source>, Error>;

    fn handle(&mut self, frame: F, _cx: &flowly::Context) -> impl Stream<Item = Self::Out> {
        async_stream::stream! {
            let _ = self
                .sender
                .send(frame)
                .await;

            while let Ok(Some(res)) = self.receiver.try_recv() {
                yield res;
            }
        }
    }
}

struct Entry<F: Frame>(F);

impl<F: Frame> std::ops::Deref for Entry<F> {
    type Target = F;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<F: Frame> PartialEq for Entry<F> {
    fn eq(&self, other: &Self) -> bool {
        self.0.timestamp() == other.0.timestamp()
    }
}

impl<F: Frame> Eq for Entry<F> {}

impl<F: Frame> PartialOrd for Entry<F> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(other.0.timestamp().cmp(&self.0.timestamp()))
    }
}

impl<F: Frame> Ord for Entry<F> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.timestamp().cmp(&self.0.timestamp())
    }
}
