//! FFmpeg library license and configuration information.
//!
//! This module provides access to the license and build configuration of the
//! underlying FFmpeg libraries. This information is useful for meeting legal
//! obligations when distributing software that uses FFmpeg.

use crate::ffmpeg;

/// Information about an FFmpeg library's version, license, and build configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryInfo {
    /// The name of the library (e.g., "libavutil", "libavcodec").
    pub name: &'static str,
    /// The version of the library as a packed integer.
    /// Use [`version_string`](Self::version_string) for a human-readable format.
    pub version: u32,
    /// The license under which the library was built (e.g., "LGPL version 2.1 or later").
    pub license: &'static str,
    /// The configuration flags used when building the library.
    pub configuration: &'static str,
}

impl LibraryInfo {
    /// Returns the version as a human-readable string in "major.minor.micro" format.
    pub fn version_string(&self) -> String {
        let major = (self.version >> 16) & 0xFF;
        let minor = (self.version >> 8) & 0xFF;
        let micro = self.version & 0xFF;
        format!("{}.{}.{}", major, minor, micro)
    }
}

/// Returns license and configuration information for all FFmpeg libraries used by video-rs.
///
/// This function returns information about the following libraries:
/// - libavutil (core utilities)
/// - libavcodec (encoding/decoding)
/// - libavformat (muxing/demuxing)
/// - libswscale (video scaling)
/// - libswresample (audio resampling)
///
/// # Example
///
/// ```
/// let libraries = video_rs::license::ffmpeg_libraries();
/// for lib in &libraries {
///     println!("{} version {}", lib.name, lib.version_string());
///     println!("  License: {}", lib.license);
///     println!("  Configuration: {}", lib.configuration);
/// }
/// ```
pub fn ffmpeg_libraries() -> Vec<LibraryInfo> {
    vec![
        LibraryInfo {
            name: "libavutil",
            version: ffmpeg::util::version(),
            license: ffmpeg::util::license(),
            configuration: ffmpeg::util::configuration(),
        },
        LibraryInfo {
            name: "libavcodec",
            version: ffmpeg::codec::version(),
            license: ffmpeg::codec::license(),
            configuration: ffmpeg::codec::configuration(),
        },
        LibraryInfo {
            name: "libavformat",
            version: ffmpeg::format::version(),
            license: ffmpeg::format::license(),
            configuration: ffmpeg::format::configuration(),
        },
        LibraryInfo {
            name: "libswscale",
            version: ffmpeg::software::scaling::version(),
            license: ffmpeg::software::scaling::license(),
            configuration: ffmpeg::software::scaling::configuration(),
        },
        LibraryInfo {
            name: "libswresample",
            version: ffmpeg::software::resampling::version(),
            license: ffmpeg::software::resampling::license(),
            configuration: ffmpeg::software::resampling::configuration(),
        },
    ]
}
