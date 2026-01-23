extern crate ffmpeg_next as ffmpeg;

use std::collections::HashMap;

use ffmpeg::Dictionary as AvDictionary;

// =============================================================================
// Reader Options (for input/decoding)
// =============================================================================

/// Options for input/reader configuration (e.g., RTSP transport settings).
///
/// These options are passed to the input format context when opening a source.
/// Use with `ReaderBuilder::with_options()`.
#[derive(Debug, Clone)]
pub struct ReaderOptions(AvDictionary<'static>);

impl ReaderOptions {
    /// Creates options such that ffmpeg will prefer TCP transport when reading RTSP stream (over
    /// the default UDP format).
    ///
    /// This sets the `rtsp_transport` to `tcp` in ffmpeg options.
    pub fn preset_rtsp_transport_tcp() -> Self {
        let mut opts = AvDictionary::new();
        opts.set("rtsp_transport", "tcp");
        Self(opts)
    }

    /// Creates options such that ffmpeg will prefer TCP transport when reading RTSP stream (over
    /// the default UDP format). It also adds some options to reduce the socket and I/O timeouts to
    /// 4 seconds.
    ///
    /// This sets the `rtsp_transport` to `tcp` in ffmpeg options, it also sets `rw_timeout` to
    /// lower (more sane) values.
    pub fn preset_rtsp_transport_tcp_and_sane_timeouts() -> Self {
        let mut opts = AvDictionary::new();
        opts.set("rtsp_transport", "tcp");
        // These can't be too low because ffmpeg takes its sweet time when connecting to RTSP
        // sources sometimes.
        opts.set("rw_timeout", "16000000");
        opts.set("stimeout", "16000000");

        Self(opts)
    }

    pub fn set(&mut self, key: &str, value: &str) {
        let Self(dict) = self;
        dict.set(key, value);
    }

    /// Convert back to ffmpeg native dictionary, which can be used with `ffmpeg_next` functions.
    pub(super) fn to_dict(&self) -> AvDictionary<'_> {
        self.0.clone()
    }
}

impl Default for ReaderOptions {
    fn default() -> Self {
        Self(AvDictionary::new())
    }
}

impl From<HashMap<String, String>> for ReaderOptions {
    fn from(item: HashMap<String, String>) -> Self {
        let mut opts = AvDictionary::new();
        for (k, v) in item {
            opts.set(&k.clone(), &v.clone());
        }
        Self(opts)
    }
}

impl From<ReaderOptions> for HashMap<String, String> {
    fn from(item: ReaderOptions) -> Self {
        item.0
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
}

unsafe impl Send for ReaderOptions {}
unsafe impl Sync for ReaderOptions {}

// =============================================================================
// Codec Options (for encoder configuration)
// =============================================================================

/// Options for codec/encoder configuration (e.g., preset, quality settings).
///
/// These options are passed to the encoder when opening it.
/// Use with `Settings::new()` or encoder-specific settings.
#[derive(Debug, Clone)]
pub struct CodecOptions(AvDictionary<'static>);

impl CodecOptions {
    /// Default options for a H264 encoder.
    #[cfg(feature = "h264")]
    pub fn preset_h264() -> Self {
        let mut opts = AvDictionary::new();
        // Set H264 encoder to the medium preset.
        opts.set("preset", "medium");
        Self(opts)
    }

    /// Options for a H264 encoder that are tuned for low-latency encoding such as for real-time
    /// streaming.
    #[cfg(feature = "h264")]
    pub fn preset_h264_realtime() -> Self {
        let mut opts = AvDictionary::new();
        // Set H264 encoder to the medium preset.
        opts.set("preset", "medium");
        // Tune for low latency
        opts.set("tune", "zerolatency");
        Self(opts)
    }

    pub fn set(&mut self, key: &str, value: &str) {
        let Self(dict) = self;
        dict.set(key, value);
    }

    /// Convert back to ffmpeg native dictionary, which can be used with `ffmpeg_next` functions.
    pub(super) fn to_dict(&self) -> AvDictionary<'_> {
        self.0.clone()
    }
}

impl Default for CodecOptions {
    fn default() -> Self {
        Self(AvDictionary::new())
    }
}

impl From<HashMap<String, String>> for CodecOptions {
    fn from(item: HashMap<String, String>) -> Self {
        let mut opts = AvDictionary::new();
        for (k, v) in item {
            opts.set(&k.clone(), &v.clone());
        }
        Self(opts)
    }
}

impl From<CodecOptions> for HashMap<String, String> {
    fn from(item: CodecOptions) -> Self {
        item.0
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
}

unsafe impl Send for CodecOptions {}
unsafe impl Sync for CodecOptions {}

// =============================================================================
// Muxer Options (for container/output format configuration)
// =============================================================================

/// Options for muxer/container configuration (e.g., movflags for MP4).
///
/// These options are passed to `avformat_write_header()` when writing the
/// container header. Use with `EncoderBuilder::with_muxer_options()`.
#[derive(Debug, Clone)]
pub struct MuxerOptions(AvDictionary<'static>);

impl MuxerOptions {
    /// Creates options for fragmented MP4 output suitable for streaming.
    ///
    /// This sets `movflags` to enable:
    /// - `faststart`: Move moov atom to the beginning
    /// - `frag_keyframe`: Start a new fragment at each keyframe
    /// - `frag_custom`: Allow manual fragment boundaries via flush
    /// - `empty_moov`: Write an empty moov atom initially
    /// - `omit_tfhd_offset`: Better compatibility
    ///
    /// The output will be compatible with Media Source Extensions (MSE) for
    /// web streaming and will be playable during encoding.
    pub fn preset_fragmented_mp4() -> Self {
        let mut opts = AvDictionary::new();
        opts.set(
            "movflags",
            "faststart+frag_keyframe+frag_custom+empty_moov+omit_tfhd_offset",
        );
        Self(opts)
    }

    pub fn set(&mut self, key: &str, value: &str) {
        let Self(dict) = self;
        dict.set(key, value);
    }

    /// Convert back to ffmpeg native dictionary, which can be used with `ffmpeg_next` functions.
    pub(super) fn to_dict(&self) -> AvDictionary<'_> {
        self.0.clone()
    }
}

impl Default for MuxerOptions {
    fn default() -> Self {
        Self(AvDictionary::new())
    }
}

impl From<HashMap<String, String>> for MuxerOptions {
    fn from(item: HashMap<String, String>) -> Self {
        let mut opts = AvDictionary::new();
        for (k, v) in item {
            opts.set(&k.clone(), &v.clone());
        }
        Self(opts)
    }
}

impl From<MuxerOptions> for HashMap<String, String> {
    fn from(item: MuxerOptions) -> Self {
        item.0
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
}

unsafe impl Send for MuxerOptions {}
unsafe impl Sync for MuxerOptions {}

// =============================================================================
// Legacy Options type (for backwards compatibility)
// =============================================================================

/// A wrapper type for ffmpeg options.
///
/// **Deprecated**: Prefer using the more specific option types:
/// - [`ReaderOptions`] for input/reader configuration
/// - [`CodecOptions`] for encoder configuration
/// - [`MuxerOptions`] for container/muxer configuration
#[derive(Debug, Clone)]
pub struct Options(AvDictionary<'static>);

impl Options {
    /// Creates options such that ffmpeg will prefer TCP transport when reading RTSP stream.
    #[deprecated(since = "0.9.0", note = "Use ReaderOptions::preset_rtsp_transport_tcp() instead")]
    pub fn preset_rtsp_transport_tcp() -> Self {
        let mut opts = AvDictionary::new();
        opts.set("rtsp_transport", "tcp");
        Self(opts)
    }

    /// Creates options for fragmented MP4 output.
    #[deprecated(since = "0.9.0", note = "Use MuxerOptions::preset_fragmented_mp4() instead")]
    pub fn preset_fragmented_mov() -> Self {
        let mut opts = AvDictionary::new();
        opts.set(
            "movflags",
            "faststart+frag_keyframe+frag_custom+empty_moov+omit_tfhd_offset",
        );
        Self(opts)
    }

    /// Default options for a H264 encoder.
    #[cfg(feature = "h264")]
    #[deprecated(since = "0.9.0", note = "Use CodecOptions::preset_h264() instead")]
    pub fn preset_h264() -> Self {
        let mut opts = AvDictionary::new();
        opts.set("preset", "medium");
        Self(opts)
    }

    pub fn set(&mut self, key: &str, value: &str) {
        let Self(dict) = self;
        dict.set(key, value);
    }

    /// Convert back to ffmpeg native dictionary.
    pub(super) fn to_dict(&self) -> AvDictionary<'_> {
        self.0.clone()
    }
}

impl Default for Options {
    fn default() -> Self {
        Self(AvDictionary::new())
    }
}

impl From<HashMap<String, String>> for Options {
    fn from(item: HashMap<String, String>) -> Self {
        let mut opts = AvDictionary::new();
        for (k, v) in item {
            opts.set(&k.clone(), &v.clone());
        }
        Self(opts)
    }
}

impl From<Options> for HashMap<String, String> {
    fn from(item: Options) -> Self {
        item.0
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
}

unsafe impl Send for Options {}
unsafe impl Sync for Options {}
