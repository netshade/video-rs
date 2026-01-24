extern crate ffmpeg_next as ffmpeg;

use ffmpeg::codec::Id as AvCodecId;
use ffmpeg::Rational as AvRational;

use crate::time::Time;

/// Subtitle codec format.
///
/// Different containers support different subtitle formats:
/// - MP4: [`SubtitleFormat::MovText`]
/// - MKV: [`SubtitleFormat::Srt`], [`SubtitleFormat::WebVtt`], [`SubtitleFormat::Ass`]
/// - WebM: [`SubtitleFormat::WebVtt`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    /// WebVTT format (for MKV, WebM containers).
    WebVtt,
    /// SubRip/SRT format (for MKV containers).
    Srt,
    /// MOV Text format (for MP4 containers).
    MovText,
    /// ASS/SSA format (for MKV containers).
    Ass,
}

impl From<SubtitleFormat> for AvCodecId {
    fn from(format: SubtitleFormat) -> Self {
        match format {
            SubtitleFormat::WebVtt => AvCodecId::WEBVTT,
            SubtitleFormat::Srt => AvCodecId::SUBRIP,
            SubtitleFormat::MovText => AvCodecId::MOV_TEXT,
            SubtitleFormat::Ass => AvCodecId::ASS,
        }
    }
}

/// A single subtitle cue with text and timing information.
///
/// # Example
///
/// ```
/// use video_rs::subtitle::SubtitleCue;
/// use video_rs::time::Time;
///
/// let cue = SubtitleCue::new(
///     "Hello, world!",
///     Time::from_secs(1.0),
///     Time::from_secs(2.5),
/// );
/// ```
#[derive(Debug, Clone)]
pub struct SubtitleCue {
    /// The subtitle text content.
    pub text: String,
    /// Start time of the subtitle.
    pub start: Time,
    /// Duration of the subtitle display.
    pub duration: Time,
}

impl SubtitleCue {
    /// Create a new subtitle cue.
    ///
    /// # Arguments
    ///
    /// * `text` - The subtitle text content.
    /// * `start` - Start time of the subtitle.
    /// * `duration` - Duration of the subtitle display.
    pub fn new(text: impl Into<String>, start: Time, duration: Time) -> Self {
        Self {
            text: text.into(),
            start,
            duration,
        }
    }
}

/// Internal state for subtitle encoding.
pub(crate) struct SubtitleEncoderState {
    pub(crate) stream_index: usize,
    pub(crate) time_base: AvRational,
    /// The subtitle format used for format-specific packet encoding.
    pub(crate) format: SubtitleFormat,
    /// Tracks the end time of the last subtitle (start + duration).
    /// Used to detect gaps and generate filler packets.
    pub(crate) last_subtitle_end: Option<Time>,
}

impl SubtitleEncoderState {
    pub(crate) fn new(stream_index: usize, time_base: AvRational, format: SubtitleFormat) -> Self {
        Self {
            stream_index,
            time_base,
            format,
            last_subtitle_end: None,
        }
    }
}
