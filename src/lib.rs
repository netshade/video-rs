pub mod decode;
pub mod encode;
pub mod error;
pub mod extradata;
pub mod frame;
pub mod hwaccel;
pub mod init;
pub mod io;
pub mod license;
pub mod location;
pub mod mux;
pub mod options;
pub mod packet;
pub mod resize;
pub mod rtp;
pub mod stream;
pub mod subtitle;
pub mod time;

mod ffi;
mod ffi_hwaccel;

pub use decode::{Decoder, DecoderBuilder};
pub use encode::{Encoder, EncoderBuilder};
pub use error::Error;
#[cfg(feature = "ndarray")]
pub use frame::Frame;
pub use init::init;
pub use io::{Reader, ReaderBuilder, Writer, WriterBuilder};
pub use license::{ffmpeg_libraries, LibraryInfo};
pub use location::{Location, Url};
pub use mux::{Muxer, MuxerBuilder};
pub use options::Options;
pub use packet::Packet;
pub use resize::Resize;
pub use subtitle::{SubtitleCue, SubtitleFormat};
pub use time::Time;

/// Re-export backend `ffmpeg` library.
pub use ffmpeg_next as ffmpeg;

/// Create a FourCC tag from four bytes, replicating FFmpeg's `MKTAG` macro.
///
/// This combines four characters into a single `u32` value, commonly used
/// for codec tags and format identifiers in FFmpeg.
///
/// # Example
///
/// ```
/// use video_rs::mktag;
///
/// // Create the 'avc1' tag for H.264
/// let avc1 = mktag(b'a', b'v', b'c', b'1');
/// assert_eq!(avc1, 0x31637661);
/// ```
#[inline]
pub const fn mktag(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}
