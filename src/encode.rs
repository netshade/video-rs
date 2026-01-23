extern crate ffmpeg_next as ffmpeg;

use std::collections::HashMap;
use std::ffi::c_int;

use ffmpeg::codec::encoder::video::Encoder as AvEncoder;
use ffmpeg::codec::encoder::video::Video as AvVideo;
use ffmpeg::codec::flag::Flags as AvCodecFlags;
use ffmpeg::codec::packet::Packet as AvPacket;
use ffmpeg::codec::Context as AvContext;
use ffmpeg::software::scaling::context::Context as AvScaler;
use ffmpeg::util::error::EAGAIN;
use ffmpeg::util::mathematics::rescale::TIME_BASE;
use ffmpeg::util::picture::Type as AvFrameType;
use ffmpeg::Error as AvError;
use ffmpeg::Rational as AvRational;

// We re-export these two types to allow callers to select pixel formats and codecs
pub use ffmpeg::codec::codec::Codec as AvCodec;
pub use ffmpeg::codec::Id as AvCodecId;
pub use ffmpeg::software::scaling::flag::Flags as AvScalerFlags;
pub use ffmpeg::util::format::Pixel as AvPixel;
use tracing::error;

use crate::error::Error;
use crate::ffi;
#[cfg(feature = "ndarray")]
use crate::frame::Frame;
use crate::frame::{PixelFormat, RawFrame};
use crate::io::private::Write;
use crate::io::{Writer, WriterBuilder};
use crate::location::Location;
use crate::mktag;
use crate::options::{CodecOptions, MuxerOptions};
use crate::subtitle::{SubtitleCue, SubtitleEncoderState, SubtitleFormat};
use crate::time::Time;

type Result<T> = std::result::Result<T, Error>;

/// Builds an [`Encoder`].
pub struct EncoderBuilder<'a> {
    destination: Location,
    settings: Settings,
    muxer_options: Option<&'a MuxerOptions>,
    format: Option<&'a str>,
    interleaved: bool,
    subtitle_format: Option<SubtitleFormat>,
    subtitle_language: String,
}

impl<'a> EncoderBuilder<'a> {
    /// Create an encoder with the specified destination and settings.
    ///
    /// * `destination` - Where to encode to.
    /// * `settings` - Encoding settings.
    pub fn new(destination: impl Into<Location>, settings: Settings) -> Self {
        Self {
            destination: destination.into(),
            settings,
            muxer_options: None,
            format: None,
            interleaved: false,
            subtitle_format: None,
            subtitle_language: "eng".to_string(),
        }
    }

    /// Set the muxer options for the container format.
    ///
    /// These options (like `movflags` for MP4) are applied when writing the
    /// container header via `avformat_write_header()`.
    ///
    /// # Arguments
    ///
    /// * `options` - The muxer options.
    pub fn with_muxer_options(mut self, options: &'a MuxerOptions) -> Self {
        self.muxer_options = Some(options);
        self
    }

    /// Set the container format for the encoder.
    ///
    /// # Arguments
    ///
    /// * `format` - Container format to use.
    pub fn with_format(mut self, format: &'a str) -> Self {
        self.format = Some(format);
        self
    }

    /// Set interleaved. This will cause the encoder to use interleaved write instead of normal
    /// write.
    pub fn interleaved(mut self) -> Self {
        self.interleaved = true;
        self
    }

    /// Add a subtitle track with the specified format.
    ///
    /// # Arguments
    ///
    /// * `format` - The subtitle format to use.
    pub fn with_subtitle_track(mut self, format: SubtitleFormat) -> Self {
        self.subtitle_format = Some(format);
        self
    }

    /// Set the language for the subtitle track (ISO 639-2 code, e.g., "eng", "spa", "fra").
    ///
    /// Defaults to "eng" (English) if not specified.
    ///
    /// # Arguments
    ///
    /// * `language` - ISO 639-2 language code.
    pub fn subtitle_language(mut self, language: impl Into<String>) -> Self {
        self.subtitle_language = language.into();
        self
    }

    /// Build an [`Encoder`].
    pub fn build(self) -> Result<Encoder> {
        let mut writer_builder = WriterBuilder::new(self.destination);
        if let Some(options) = self.muxer_options {
            writer_builder = writer_builder.with_muxer_options(options);
        }
        if let Some(format) = self.format {
            writer_builder = writer_builder.with_format(format);
        }
        Encoder::from_writer(
            writer_builder.build()?,
            self.interleaved,
            self.settings,
            self.subtitle_format,
            self.subtitle_language,
        )
    }
}

/// Encodes frames into a video stream.
///
/// # Example
///
/// ```ignore
/// let encoder = Encoder::new(
///     Path::new("video_in.mp4"),
///     Settings::for_h264_yuv420p(800, 600, 30.0)
/// )
/// .unwrap();
///
/// let decoder = Decoder::new(Path::new("video_out.mkv")).unwrap();
/// decoder
///     .decode_iter()
///     .take_while(Result::is_ok)
///     .map(|frame| encoder
///         .encode(frame.unwrap())
///         .expect("Failed to encode frame."),
///     );
/// ```
pub struct Encoder {
    writer: Writer,
    writer_stream_index: usize,
    encoder: AvEncoder,
    encoder_time_base: AvRational,
    keyframe_interval: u64,
    interleaved: bool,
    scaler: Option<AvScaler>,
    source_width: u32,
    source_height: u32,
    source_format: PixelFormat,
    frame_count: u64,
    have_written_header: bool,
    have_written_trailer: bool,
    subtitle_state: Option<SubtitleEncoderState>,
}

impl Encoder {
    /// Create an encoder with the specified destination and settings.
    ///
    /// * `destination` - Where to encode to.
    /// * `settings` - Encoding settings.
    #[inline]
    pub fn new(destination: impl Into<Location>, settings: Settings) -> Result<Self> {
        EncoderBuilder::new(destination, settings).build()
    }

    /// Encode a single `ndarray` frame.
    ///
    /// # Arguments
    ///
    /// * `frame` - Frame to encode in `HWC` format and standard layout.
    /// * `source_timestamp` - Frame timestamp of original source. This is necessary to make sure
    ///   the output will be timed correctly.
    #[cfg(feature = "ndarray")]
    pub fn encode(&mut self, frame: &Frame, source_timestamp: Time) -> Result<()> {
        let (height, width, channels) = frame.dim();
        if height != self.source_height as usize
            || width != self.source_width as usize
            || channels != 3
        {
            error!(
                "Error, expected {}x{} 3 channels, received {}x{} {:?} channels",
                self.source_width, self.source_height, width, height, channels
            );
            return Err(Error::InvalidFrameFormat);
        }

        let mut frame = ffi::convert_ndarray_to_frame(frame, self.source_format.into())
            .map_err(Error::BackendError)?;

        frame.set_pts(
            source_timestamp
                .aligned_with_rational(self.encoder_time_base)
                .into_value(),
        );

        self.encode_raw(frame)
    }

    /// Encode a single raw frame.
    ///
    /// # Arguments
    ///
    /// * `frame` - Frame to encode.
    pub fn encode_raw(&mut self, frame: RawFrame) -> Result<()> {
        if frame.width() != self.source_width
            || frame.height() != self.source_height
            || frame.format() != self.source_format
        {
            error!(
                "Error, expected {}x{} {:?}, received {}x{} {:?}",
                self.source_width,
                self.source_height,
                self.source_format,
                frame.width(),
                frame.height(),
                frame.format()
            );
            return Err(Error::InvalidFrameFormat);
        }

        // Write file header if we hadn't done that yet.
        if !self.have_written_header {
            self.writer.write_header()?;
            self.have_written_header = true;
        }

        // Reformat frame to target pixel format.
        let mut frame = self.scale(frame)?;
        // Producer key frame every once in a while
        if self.frame_count.is_multiple_of(self.keyframe_interval) {
            frame.set_kind(AvFrameType::I);
        }

        self.encoder
            .send_frame(&frame)
            .map_err(Error::BackendError)?;
        // Increment frame count regardless of whether or not frame is written, see
        // https://github.com/oddity-ai/video-rs/issues/46.
        self.frame_count += 1;

        if let Some(packet) = self.encoder_receive_packet()? {
            self.write(packet)?;
        }

        Ok(())
    }

    /// Encode a subtitle cue.
    ///
    /// # Arguments
    ///
    /// * `cue` - The subtitle cue to encode.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SubtitleTrackNotConfigured`] if no subtitle track was configured
    /// when building the encoder.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use video_rs::subtitle::{SubtitleCue, SubtitleFormat};
    /// use video_rs::time::Time;
    ///
    /// let mut encoder = EncoderBuilder::new("output.mp4", settings)
    ///     .with_subtitle_track(SubtitleFormat::MovText)
    ///     .build()?;
    ///
    /// encoder.encode_subtitle(&SubtitleCue::new(
    ///     "Hello, world!",
    ///     Time::from_secs(1.0),
    ///     Time::from_secs(2.5),
    /// ))?;
    /// ```
    pub fn encode_subtitle(&mut self, cue: &SubtitleCue) -> Result<()> {
        let state = self
            .subtitle_state
            .as_ref()
            .ok_or(Error::SubtitleTrackNotConfigured)?;

        // Write file header if we hadn't done that yet.
        if !self.have_written_header {
            self.writer.write_header()?;
            self.have_written_header = true;
        }

        // Convert start time to time base units for PTS
        let pts_value = cue
            .start
            .aligned_with_rational(state.time_base)
            .into_value()
            .unwrap_or(0);

        // Duration in milliseconds (since time_base is 1/1000)
        let duration_ms = (cue.duration.as_secs_f64() * 1000.0).round() as i64;

        // Create subtitle data packet with format-specific encoding
        let packet_data = match state.format {
            SubtitleFormat::MovText => {
                // mov_text (tx3g) format requires a 16-bit big-endian length prefix
                let text_bytes = cue.text.as_bytes();
                let len = text_bytes.len() as u16;
                let mut data = Vec::with_capacity(2 + text_bytes.len());
                data.extend_from_slice(&len.to_be_bytes());
                data.extend_from_slice(text_bytes);
                data
            }
            _ => {
                // Other formats: use raw text for now
                cue.text.as_bytes().to_vec()
            }
        };

        let mut packet = AvPacket::copy(&packet_data);
        packet.set_stream(state.stream_index);
        packet.set_pts(Some(pts_value));
        packet.set_dts(Some(pts_value));
        packet.set_duration(duration_ms);

        // Get the output stream time base and rescale
        let stream_time_base = self
            .writer
            .output
            .stream(state.stream_index)
            .unwrap()
            .time_base();
        packet.rescale_ts(state.time_base, stream_time_base);

        if self.interleaved {
            self.writer.write_interleaved(&mut packet)?;
        } else {
            self.writer.write(&mut packet)?;
        }

        Ok(())
    }

    /// Signal to the encoder that writing has finished. This will cause any packets in the encoder
    /// to be flushed and a trailer to be written if the container format has one.
    ///
    /// Note: If you don't call this function before dropping the encoder, it will be called
    /// automatically. This will block the caller thread. Any errors cannot be propagated in this
    /// case.
    pub fn finish(&mut self) -> Result<()> {
        if self.have_written_header && !self.have_written_trailer {
            self.have_written_trailer = true;
            self.flush()?;
            self.writer.write_trailer()?;
        }

        Ok(())
    }

    /// Get encoder time base.
    #[inline]
    pub fn time_base(&self) -> AvRational {
        self.encoder_time_base
    }

    /// Create an encoder from a [`Writer`].
    ///
    /// # Arguments
    ///
    /// * `writer` - [`Writer`] to create encoder from.
    /// * `interleaved` - Whether or not to use interleaved write.
    /// * `settings` - Encoder settings to use.
    /// * `subtitle_format` - Optional subtitle format to enable subtitle track.
    /// * `subtitle_language` - Language code for the subtitle track.
    fn from_writer(
        mut writer: Writer,
        interleaved: bool,
        settings: Settings,
        subtitle_format: Option<SubtitleFormat>,
        subtitle_language: String,
    ) -> Result<Self> {
        let global_header = writer.global_header();

        if let Some(metadata) = settings.metadata.clone() {
            writer.set_metadata(metadata);
        }
        let mut writer_stream = writer.add_stream(settings.codec)?;
        let writer_stream_index = writer_stream.index();
        let Settings {
            source_width,
            source_height,
            destination_width,
            destination_height,
            source_format,
            destination_format,
            scaler_flags,
            ..
        } = settings;
        let mut encoder_context = match settings.codec {
            Some(codec) => ffi::codec_context_as(&codec)?,
            None => AvContext::new(),
        };
        let slice: c_int = ffmpeg_next::threading::Type::Slice.into();
        let frame: c_int = ffmpeg_next::threading::Type::Frame.into();
        let threading_kind = slice & frame;
        let config = ffmpeg_next::threading::Config {
            kind: threading_kind.into(),
            count: 0,
        };
        encoder_context.set_threading(config);

        // Some formats require this flag to be set or the output will
        // not be playable by dumb players.
        if global_header {
            encoder_context.set_flags(AvCodecFlags::GLOBAL_HEADER);
        }

        let mut encoder = encoder_context.encoder().video()?;
        settings.apply_to(&mut encoder);

        // Just use the ffmpeg global time base which is precise enough
        // that we should never get in trouble.
        encoder.set_time_base(TIME_BASE);

        let encoder = encoder.open_with(settings.codec_options().to_dict())?;
        let encoder_time_base = ffi::get_encoder_time_base(&encoder);

        writer_stream.set_parameters(&encoder);

        let scaler_needed = source_format != destination_format
            || source_width != destination_width
            || source_height != destination_height;

        let scaler = if scaler_needed {
            Some(AvScaler::get(
                source_format,
                source_width,
                source_height,
                destination_format,
                destination_width,
                destination_height,
                scaler_flags,
            )?)
        } else {
            None
        };

        // Set up subtitle stream if requested
        let subtitle_state = if let Some(format) = subtitle_format {
            let subtitle_codec_id: AvCodecId = format.into();
            let subtitle_codec =
                ffmpeg::encoder::find(subtitle_codec_id).ok_or(Error::UninitializedCodec)?;

            // Add subtitle stream
            let mut subtitle_stream = writer.add_stream(Some(subtitle_codec))?;
            let subtitle_stream_index = subtitle_stream.index();

            // Subtitle time base: 1/1000 (milliseconds)
            let subtitle_time_base = AvRational::new(1, 1000);
            subtitle_stream.set_time_base(subtitle_time_base);

            // Set codec parameters directly on the stream
            {
                let mut parameters = subtitle_stream.parameters();
                // Safety: We need to set the codec_id on the parameters
                unsafe {
                    (*parameters.as_mut_ptr()).codec_type =
                        ffmpeg::ffi::AVMediaType::AVMEDIA_TYPE_SUBTITLE;
                    (*parameters.as_mut_ptr()).codec_id = subtitle_codec_id.into();

                    // Set codec_tag to 'tx3g' for MOV_TEXT to ensure FFmpeg's MOV muxer
                    // uses the 'sbtl' handler type instead of 'text'. This is required
                    // for QuickTime, VLC, and other Apple-compatible players to recognize
                    // the subtitle track.
                    if matches!(format, SubtitleFormat::MovText) {
                        (*parameters.as_mut_ptr()).codec_tag = mktag(b't', b'x', b'3', b'g');

                        // Set the tx3g sample description extradata for QuickTime compatibility.
                        // This includes display settings, default style, and font table.
                        let extradata = build_tx3g_extradata();
                        let extradata_size = extradata.len();

                        // Allocate with av_mallocz (FFmpeg will free this later).
                        // Must include AV_INPUT_BUFFER_PADDING_SIZE for safety.
                        let alloc_size =
                            extradata_size + ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize;
                        let extradata_ptr = ffmpeg::ffi::av_mallocz(alloc_size) as *mut u8;

                        if !extradata_ptr.is_null() {
                            std::ptr::copy_nonoverlapping(
                                extradata.as_ptr(),
                                extradata_ptr,
                                extradata_size,
                            );
                            (*parameters.as_mut_ptr()).extradata = extradata_ptr;
                            (*parameters.as_mut_ptr()).extradata_size = extradata_size as c_int;
                        }
                    }
                }
            }

            // Set language metadata
            let mut metadata = ffmpeg::Dictionary::new();
            metadata.set("language", &subtitle_language);
            subtitle_stream.set_metadata(metadata);

            Some(SubtitleEncoderState::new(
                subtitle_stream_index,
                subtitle_time_base,
                format,
            ))
        } else {
            None
        };

        Ok(Self {
            writer,
            writer_stream_index,
            encoder,
            encoder_time_base,
            keyframe_interval: settings.keyframe_interval,
            interleaved,
            scaler,
            source_width,
            source_height,
            source_format: settings.source_format,
            frame_count: 0,
            have_written_header: false,
            have_written_trailer: false,
            subtitle_state,
        })
    }

    /// Apply scaling (or pixel reformatting in this case) on the frame with the scaler we
    /// initialized earlier.
    ///
    /// # Arguments
    ///
    /// * `frame` - Frame to rescale.
    fn scale(&mut self, frame: RawFrame) -> Result<RawFrame> {
        match self.scaler.as_mut() {
            Some(scaler) => {
                let mut frame_scaled = RawFrame::empty();
                scaler
                    .run(&frame, &mut frame_scaled)
                    .map_err(Error::BackendError)?;
                // Copy over PTS from old frame.
                frame_scaled.set_pts(frame.pts());
                Ok(frame_scaled)
            }
            None => Ok(frame), // Passthrough - no conversion needed
        }
    }

    /// Pull an encoded packet from the decoder. This function also handles the possible `EAGAIN`
    /// result, in which case we just need to go again.
    fn encoder_receive_packet(&mut self) -> Result<Option<AvPacket>> {
        let mut packet = AvPacket::empty();
        let encode_result = self.encoder.receive_packet(&mut packet);
        match encode_result {
            Ok(()) => Ok(Some(packet)),
            Err(AvError::Other { errno }) if errno == EAGAIN => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Acquire the time base of the output stream.
    fn stream_time_base(&mut self) -> AvRational {
        self.writer
            .output
            .stream(self.writer_stream_index)
            .unwrap()
            .time_base()
    }

    /// Write encoded packet to output stream.
    ///
    /// # Arguments
    ///
    /// * `packet` - Encoded packet.
    fn write(&mut self, mut packet: AvPacket) -> Result<()> {
        packet.set_stream(self.writer_stream_index);
        packet.set_position(-1);
        packet.rescale_ts(self.encoder_time_base, self.stream_time_base());
        if self.interleaved {
            self.writer.write_interleaved(&mut packet)?;
        } else {
            self.writer.write(&mut packet)?;
        };

        Ok(())
    }

    /// Flush output buffers to disk without finalizing the stream.
    ///
    /// Unlike `flush()`, this does not send EOF to the encoder and can be called
    /// periodically during encoding to ensure data is written to disk. This is
    /// useful for long-running captures where you want to minimize data loss
    /// if the process is interrupted.
    pub fn flush_output(&mut self) -> Result<()> {
        ffi::flush_output(&mut self.writer.output).map_err(Error::BackendError)
    }

    /// Enable immediate packet flushing on the format context.
    ///
    /// When enabled, the muxer will flush packets to disk immediately rather
    /// than buffering them. This is important for fragmented MP4 to ensure
    /// the moov atom and fragments are written to disk promptly, making the
    /// file playable during encoding.
    pub fn set_flush_packets(&mut self, enabled: bool) {
        self.writer.set_flush_packets(enabled);
    }

    /// Write the file header if it hasn't been written yet.
    ///
    /// Normally the header is written lazily on the first `encode()` call.
    /// This method allows you to write it explicitly, which is useful when
    /// combined with `flush_output()` to ensure the header is on disk before
    /// encoding begins (important for fragmented MP4 playback during encoding).
    pub fn write_header(&mut self) -> Result<()> {
        if !self.have_written_header {
            self.writer.write_header()?;
            self.have_written_header = true;
        }
        Ok(())
    }

    /// Flush the encoder, drain any packets that still need processing.
    pub fn flush(&mut self) -> Result<()> {
        // Maximum number of invocations to `encoder_receive_packet`
        // to drain the items still on the queue before giving up.
        const MAX_DRAIN_ITERATIONS: u32 = 100;

        // Notify the encoder that the last frame has been sent.
        self.encoder.send_eof()?;

        // We need to drain the items still in the encoders queue.
        for _ in 0..MAX_DRAIN_ITERATIONS {
            match self.encoder_receive_packet() {
                Ok(Some(packet)) => self.write(packet)?,
                Ok(None) => continue,
                Err(_) => break,
            }
        }

        Ok(())
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// Holds a logical combination of encoder settings.
#[derive(Clone)]
pub struct Settings {
    source_width: u32,
    source_height: u32,
    source_format: AvPixel,
    scaler_flags: AvScalerFlags,
    destination_width: u32,
    destination_height: u32,
    destination_format: AvPixel,
    codec: Option<AvCodec>,
    keyframe_interval: u64,
    frame_rate: i32,
    metadata: Option<HashMap<String, String>>,
    codec_options: CodecOptions,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let codec_name = if let Some(codec) = &self.codec {
            codec.name()
        } else {
            "None"
        };
        let metadata = if let Some(metadata) = &self.metadata {
            metadata.clone()
        } else {
            HashMap::default()
        };
        f.debug_struct("Settings")
            .field("source_width", &self.source_width)
            .field("source_height", &self.source_height)
            .field("source_format", &self.source_format)
            .field("destination_width", &self.destination_width)
            .field("destination_height", &self.destination_height)
            .field("destination_format", &self.destination_format)
            .field("codec", &codec_name)
            .field("keyframe_interval", &self.keyframe_interval)
            .field("frame_rate", &self.frame_rate)
            .field("metadata", &metadata)
            .field("codec_options", &self.codec_options)
            .finish()
    }
}

impl Settings {
    /// Default keyframe interval.
    const KEY_FRAME_INTERVAL: u64 = 12;

    /// This is the assumed FPS for the encoder to use. Note that this does not need to be correct
    /// exactly.
    const FRAME_RATE: i32 = 30;

    pub fn find_codec(codec_id: AvCodecId, default_name: Option<&str>) -> Option<AvCodec> {
        if let Some(default_name) = default_name {
            let result = Self::find_codec_by_name(default_name);
            if result.is_some() {
                return result;
            }
        }
        ffmpeg::encoder::find(codec_id)
    }

    pub fn find_codec_by_name(name: &str) -> Option<AvCodec> {
        ffmpeg::encoder::find_by_name(name)
    }

    /// Create encoder settings for an H264 stream with YUV420p pixel format. This will encode to
    /// arguably the most widely compatible video file since H264 is a common codec and YUV420p is
    /// the most commonly used pixel format.
    #[cfg(feature = "h264")]
    pub fn preset_h264_yuv420p(
        width: usize,
        height: usize,
        realtime: bool,
        metadata: Option<HashMap<String, String>>,
    ) -> Settings {
        let codec_options = if realtime {
            CodecOptions::preset_h264_realtime()
        } else {
            CodecOptions::preset_h264()
        };
        Self::new(
            width,
            height,
            AvPixel::RGB24,
            AvScalerFlags::FAST_BILINEAR,
            width,
            height,
            AvPixel::YUV420P,
            Self::KEY_FRAME_INTERVAL,
            Self::FRAME_RATE,
            Self::find_codec(AvCodecId::H264, Some("libx264")),
            codec_options,
            metadata,
        )
    }

    /// Create encoder settings for an H264 stream with a custom pixel format and options.
    /// This allows for greater flexibility in encoding settings, enabling specific requirements
    /// or optimizations to be set depending on the use case.
    ///
    /// # Arguments
    ///
    /// * `width` - The width of the video stream.
    /// * `height` - The height of the video stream.
    /// * `pixel_format` - The desired pixel format for the video stream.
    /// * `codec_options` - Custom H264 codec options.
    ///
    /// # Return value
    ///
    /// A `Settings` instance with the specified configuration.
    #[cfg(feature = "h264")]
    pub fn preset_h264_custom(
        width: usize,
        height: usize,
        pixel_format: PixelFormat,
        codec_options: CodecOptions,
        metadata: Option<HashMap<String, String>>,
    ) -> Settings {
        Self::new(
            width,
            height,
            AvPixel::RGB24,
            AvScalerFlags::FAST_BILINEAR,
            width,
            height,
            pixel_format,
            Self::KEY_FRAME_INTERVAL,
            Self::FRAME_RATE,
            Self::find_codec(AvCodecId::H264, Some("libx264")),
            codec_options,
            metadata,
        )
    }

    /// Create encoder settings for a VP9 stream with YUV420p pixel format. This will encode to
    /// a widely supported video file that is not patent / licensing encumbered and YUV420p is
    /// the most commonly used pixel format.
    #[cfg(feature = "vp9")]
    pub fn preset_vp9_yuv420p(
        width: usize,
        height: usize,
        codec_options: Option<CodecOptions>,
        metadata: Option<HashMap<String, String>>,
    ) -> Settings {
        Self::new(
            width,
            height,
            AvPixel::RGB24,
            AvScalerFlags::FAST_BILINEAR,
            width,
            height,
            AvPixel::YUV420P,
            Self::KEY_FRAME_INTERVAL,
            Self::FRAME_RATE,
            Self::find_codec(AvCodecId::VP9, Some("libvpx-vp9")),
            codec_options.unwrap_or_default(),
            metadata,
        )
    }

    /// Create encoder settings for a VP9 stream with YUV420p pixel format. This will encode to
    /// a widely supported video file that is not patent / licensing encumbered and YUV420p is
    /// the most commonly used pixel format.
    #[cfg(feature = "vp9")]
    pub fn preset_vp9_yuv420p_realtime(
        width: usize,
        height: usize,
        codec_options: Option<CodecOptions>,
        metadata: Option<HashMap<String, String>>,
    ) -> Settings {
        let mut opts = codec_options.unwrap_or_default();
        opts.set("deadline", "realtime");
        opts.set("row-mt", "1");
        opts.set("cpu-used", "8");
        Self::new(
            width,
            height,
            AvPixel::RGB24,
            AvScalerFlags::FAST_BILINEAR,
            width,
            height,
            AvPixel::YUV420P,
            Self::KEY_FRAME_INTERVAL,
            Self::FRAME_RATE,
            Self::find_codec(AvCodecId::VP9, Some("libvpx-vp9")),
            opts,
            metadata,
        )
    }

    pub fn new(
        source_width: usize,
        source_height: usize,
        source_format: AvPixel,
        scaler_flags: AvScalerFlags,
        destination_width: usize,
        destination_height: usize,
        destination_format: AvPixel,
        keyframe_interval: u64,
        frame_rate: i32,
        codec: Option<AvCodec>,
        codec_options: CodecOptions,
        metadata: Option<HashMap<String, String>>,
    ) -> Settings {
        Self {
            source_width: source_width as u32,
            source_height: source_height as u32,
            source_format,
            scaler_flags,
            destination_width: destination_width as u32,
            destination_height: destination_height as u32,
            destination_format,
            keyframe_interval,
            frame_rate,
            codec,
            metadata,
            codec_options,
        }
    }

    /// Set the keyframe interval.
    pub fn set_keyframe_interval(&mut self, keyframe_interval: u64) {
        self.keyframe_interval = keyframe_interval;
    }

    /// Set the keyframe interval.
    pub fn with_keyframe_interval(mut self, keyframe_interval: u64) -> Self {
        self.set_keyframe_interval(keyframe_interval);
        self
    }

    pub fn resized_to(mut self, destination_width: usize, destination_height: usize) -> Self {
        self.destination_width = destination_width as u32;
        self.destination_height = destination_height as u32;
        self
    }

    /// Apply the settings to an encoder.
    ///
    /// # Arguments
    ///
    /// * `encoder` - Encoder to apply settings to.
    ///
    /// # Return value
    ///
    /// New encoder with settings applied.
    fn apply_to(&self, encoder: &mut AvVideo) {
        encoder.set_width(self.destination_width);
        encoder.set_height(self.destination_height);
        encoder.set_format(self.destination_format);
        encoder.set_frame_rate(Some((self.frame_rate, 1)));
    }

    /// Get codec options.
    fn codec_options(&self) -> &CodecOptions {
        &self.codec_options
    }
}

unsafe impl Send for Encoder {}
unsafe impl Sync for Encoder {}

/// TX3G (mov_text) sample description constants.
///
/// These define the default subtitle appearance for QuickTime-compatible players.
/// Reference: 3GPP TS 26.245 (Timed Text Format)
mod tx3g {
    /// Display flags - controls scrolling, karaoke, etc. 0 = static text.
    pub const DISPLAY_FLAGS: u32 = 0;

    /// Horizontal justification: 0=left, 1=center, 2=right
    pub const JUSTIFY_CENTER: u8 = 0x01;

    /// Vertical justification: 0=top, 1=center, -1(0xFF)=bottom
    pub const JUSTIFY_BOTTOM: u8 = 0xFF;

    /// Background color (RGBA) - transparent black
    pub const BACKGROUND_COLOR: u32 = 0x00000000;

    /// Text box bounds (top, left, bottom, right) - zeros means use full frame
    pub const TEXT_BOX_BOUNDS: [u8; 8] = [0; 8];

    /// Default font ID referenced in style and font table
    pub const DEFAULT_FONT_ID: u16 = 1;

    /// Style flags: 0=normal, 1=bold, 2=italic, 4=underline
    pub const STYLE_NORMAL: u8 = 0x00;

    /// Default font size in points
    pub const FONT_SIZE_PT: u8 = 18;

    /// Text color (RGBA) - opaque white
    pub const TEXT_COLOR_WHITE: u32 = 0xFFFFFFFF;

    /// Default font name - "Serif" is a standard fallback name
    pub const FONT_NAME: &[u8] = b"Serif";

    /// FontTableBox type tag
    pub const FTAB_TAG: &[u8; 4] = b"ftab";

    /// FontTableBox header size (box_size + tag + entry_count = 4 + 4 + 2)
    pub const FTAB_HEADER_SIZE: u32 = 10;

    /// FontRecord overhead (font_id + name_length = 2 + 1)
    pub const FONT_RECORD_OVERHEAD: u32 = 3;
}

/// Build the tx3g (mov_text) sample description extradata.
///
/// This contains the display settings, default style, and font table required
/// for QuickTime and other Apple players to properly recognize subtitle tracks.
///
/// Structure (48 bytes for default "Serif" font):
/// - Display flags (4 bytes)
/// - Justification (2 bytes)
/// - Background color RGBA (4 bytes)
/// - BoxRecord: text box bounds (8 bytes)
/// - StyleRecord: default text style (12 bytes)
/// - FontTableBox: font definitions (variable, 18 bytes for "Serif")
/// This structure definition is taken from https://github.com/FFmpeg/FFmpeg/blob/master/libavcodec/movtextenc.c#L189-L216
fn build_tx3g_extradata() -> Vec<u8> {
    use tx3g::*;

    let mut data = Vec::with_capacity(48);

    // Display flags (4 bytes)
    data.extend_from_slice(&DISPLAY_FLAGS.to_be_bytes());

    // Justification (2 bytes)
    data.push(JUSTIFY_CENTER);
    data.push(JUSTIFY_BOTTOM);

    // Background color RGBA (4 bytes)
    data.extend_from_slice(&BACKGROUND_COLOR.to_be_bytes());

    // BoxRecord (8 bytes): text positioning bounds
    data.extend_from_slice(&TEXT_BOX_BOUNDS);

    // StyleRecord (12 bytes)
    data.extend_from_slice(&0u16.to_be_bytes()); // startChar (apply from start)
    data.extend_from_slice(&0u16.to_be_bytes()); // endChar (apply to end)
    data.extend_from_slice(&DEFAULT_FONT_ID.to_be_bytes());
    data.push(STYLE_NORMAL);
    data.push(FONT_SIZE_PT);
    data.extend_from_slice(&TEXT_COLOR_WHITE.to_be_bytes());

    // FontTableBox
    let ftab_size = FTAB_HEADER_SIZE + FONT_RECORD_OVERHEAD + FONT_NAME.len() as u32;
    data.extend_from_slice(&ftab_size.to_be_bytes());
    data.extend_from_slice(FTAB_TAG);
    data.extend_from_slice(&1u16.to_be_bytes()); // entry-count

    // FontRecord
    data.extend_from_slice(&DEFAULT_FONT_ID.to_be_bytes());
    data.push(FONT_NAME.len() as u8);
    data.extend_from_slice(FONT_NAME);

    data
}
