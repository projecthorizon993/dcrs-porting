//! What a native client can actually do with media.
//!
//! This is the media counterpart to [`crate::capability`]. That module asks *can a mod's API call be
//! made natively*; this one asks *what happens to its pixels and samples*, which is a different and
//! usually harsher question.
//!
//! The short version, from reading Serein's source: a native client can speak every codec a Discord
//! mod could ask for, and can decode nearly every format — but a plugin gets none of it. The extension
//! runtime has no imports at all, no media capability exists among the 52, and the panel element enum
//! has no image or canvas variant. So a mod that injects a `<video>` element, filters audio frames, or
//! re-encodes a stream has nothing to land on.
//!
//! What *is* portable is the behavioural half: joining a call, muting, picking a device, asking the
//! host to open its screen-share picker. Those become field assignments and host-proposed actions.
//!
//! So the interesting output is a classification with the reason attached, because "unsupported" on its
//! own reads as a missing feature rather than an architectural difference.

use std::fmt;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// A media concern a mod might reach for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaConcern {
    /// Opus voice, sent and received.
    VoiceOpus,
    /// H.264 call and stream video.
    VideoH264,
    /// End-to-end encryption for a call or stream.
    CallEncryption,
    /// Screen capture and its audio track.
    ScreenShare,
    /// Camera capture and encode.
    Camera,
    /// Choosing an input or output device.
    DeviceSelection,
    /// Per-participant or master gain.
    VolumeControl,
    /// Playback of an audio attachment.
    AudioPlayback,
    /// Playback of a video attachment.
    VideoPlayback,
    /// Decoding an image attachment, animated or not.
    ImageDecode,
    /// Reading the raw frames of a stream from inside the client.
    StreamFrameAccess,
    /// Re-encoding or transforming media in the client process.
    Transcoding,
    /// Injecting an `<audio>` or `<video>` element into the page.
    MediaElementInjection,
}

impl MediaConcern {
    /// Every concern, in declaration order.
    pub const ALL: &'static [Self] = &[
        Self::VoiceOpus,
        Self::VideoH264,
        Self::CallEncryption,
        Self::ScreenShare,
        Self::Camera,
        Self::DeviceSelection,
        Self::VolumeControl,
        Self::AudioPlayback,
        Self::VideoPlayback,
        Self::ImageDecode,
        Self::StreamFrameAccess,
        Self::Transcoding,
        Self::MediaElementInjection,
    ];

    /// The wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VoiceOpus => "voiceOpus",
            Self::VideoH264 => "videoH264",
            Self::CallEncryption => "callEncryption",
            Self::ScreenShare => "screenShare",
            Self::Camera => "camera",
            Self::DeviceSelection => "deviceSelection",
            Self::VolumeControl => "volumeControl",
            Self::AudioPlayback => "audioPlayback",
            Self::VideoPlayback => "videoPlayback",
            Self::ImageDecode => "imageDecode",
            Self::StreamFrameAccess => "streamFrameAccess",
            Self::Transcoding => "transcoding",
            Self::MediaElementInjection => "mediaElementInjection",
        }
    }

    /// The native surface that owns this, if any.
    #[must_use]
    pub const fn native_owner(self) -> Option<&'static str> {
        match self {
            Self::VoiceOpus => Some("voice session"),
            Self::VideoH264 => Some("voice session video"),
            Self::CallEncryption => Some("voice session DAVE"),
            Self::ScreenShare => Some("media_control action"),
            Self::Camera => Some("camera_control action"),
            // Both ride the same capability: read audio preferences, propose a change.
            Self::DeviceSelection | Self::VolumeControl => Some("audio_settings actions"),
            Self::AudioPlayback | Self::VideoPlayback => Some("attachment player"),
            Self::ImageDecode => Some("attachment decoder"),
            // Nothing. These three are the reasons a media plugin does not port, and naming the
            // absence is more useful than inventing an owner for them.
            Self::StreamFrameAccess | Self::Transcoding | Self::MediaElementInjection => None,
        }
    }

    /// How a native client handles this.
    #[must_use]
    pub const fn support(self) -> Support {
        match self {
            // The client owns the codec outright. A mod asking for a different voice codec is asking
            // for something Discord does not accept either.
            Self::VoiceOpus
            | Self::VideoH264
            | Self::CallEncryption
            | Self::ScreenShare
            | Self::Camera
            | Self::DeviceSelection
            | Self::VolumeControl
            | Self::AudioPlayback
            | Self::VideoPlayback
            | Self::ImageDecode => Support::Native,

            // These have no surface at all. Not "not implemented" — no capability, no element, no
            // host function, and the Wasm module cannot import anything to build one.
            Self::StreamFrameAccess | Self::Transcoding | Self::MediaElementInjection => {
                Support::OutOfReach
            }
        }
    }

    /// Whether a plugin can reach this through the extension API.
    ///
    /// Distinct from [`Self::support`]: the client can decode an image just fine, and a plugin still
    /// cannot ask it to, because there is no capability for it.
    #[must_use]
    pub const fn plugin_reachable(self) -> bool {
        match self {
            // The host does all of this itself, and a plugin can only ask it to: screen share opens
            // the native picker without exposing the source list, and volume is a pre-mix gain.
            Self::ScreenShare | Self::Camera | Self::DeviceSelection | Self::VolumeControl => true,

            // No capability covers codecs, decoding, or playback. `api_proxy` excludes CDN fetches,
            // and no action returns a frame, a byte, or a URL.
            Self::StreamFrameAccess
            | Self::Transcoding
            | Self::MediaElementInjection
            | Self::ImageDecode
            | Self::AudioPlayback
            | Self::VideoPlayback
            | Self::VoiceOpus
            | Self::VideoH264
            | Self::CallEncryption => false,
        }
    }

    /// Why this concern lands the way it does.
    #[must_use]
    pub const fn caveat(self) -> &'static str {
        match self {
            Self::VoiceOpus => "Opus is mandatory on Discord; a mod cannot substitute a codec",
            Self::VideoH264 => {
                "H.264 only for sending; AV1 and HEVC are not implemented, so a mod cannot add them"
            }
            Self::CallEncryption => {
                "DAVE is negotiated by the host; a mod cannot weaken or replace it"
            }
            Self::ScreenShare => {
                "the plugin proposes opening the host picker and never sees the source list or frames"
            }
            Self::Camera => {
                "the plugin toggles the camera and picks a host-known device, with no frames"
            }
            Self::DeviceSelection => "the host owns the device list and enforces the change",
            Self::VolumeControl => {
                "participant volume is a pre-mix gain of 0-200% and cannot target the local user"
            }
            Self::AudioPlayback => {
                "the host decodes MP3, WAV, Ogg and Opus; a plugin cannot add a format"
            }
            Self::VideoPlayback => {
                "the host decodes H.264 and HEVC up to 1080p; a plugin cannot add a container"
            }
            Self::ImageDecode => {
                "PNG, JPEG, GIF, WebP and APNG are handled by the host; AVIF and HEIC are not \
                 decoded everywhere"
            }
            Self::StreamFrameAccess => {
                "no attachment bytes or media URLs appear in any plugin snapshot, and no action \
                 returns frames"
            }
            Self::Transcoding => {
                "no capability exposes a codec to Wasm, and a module cannot import one; the only \
                 transcode path shells out to an installed ffmpeg"
            }
            Self::MediaElementInjection => {
                "the panel element enum has no image, canvas, or media variant, and there is no \
                 DOM to inject into"
            }
        }
    }
}

/// How much of a concern a native client carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    /// The client owns this outright; a ported mod becomes a field or a host-proposed action.
    Native,
    /// The client has the machinery but a plugin cannot reach it.
    NativeNotReachable,
    /// No mechanism exists on either side.
    OutOfReach,
}

/// One concern's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Verdict {
    /// Which concern.
    pub concern: MediaConcern,
    /// How the client handles it.
    pub support: Support,
    /// Whether a plugin can reach it.
    pub plugin_reachable: bool,
    /// The native surface that owns it, if any.
    pub native_owner: Option<&'static str>,
    /// Why.
    pub caveat: &'static str,
}

/// A media assessment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaReport {
    /// Every verdict, in a stable order.
    pub verdicts: Vec<Verdict>,
    /// Concerns the source actually touched, or `None` when it touched none.
    pub detected: Option<Vec<MediaConcern>>,
}

/// The markers each concern leaves behind in a source.
///
/// A table rather than code, because the set is the judgement and the scan is trivial. Keeping it
/// declarative means a marker can be tuned or removed without touching the scanner.
///
/// Every entry is a call, a property, or a library symbol. A bare English word is never enough:
/// `camera` matches `--camera-accent` in a theme, and `.play()` matches any line of prose.
const MARKERS: &[(MediaConcern, &[&str])] = &[
    (
        MediaConcern::VoiceOpus,
        &[
            "opus",
            "rtp",
            "ssrc",
            "voiceconnection",
            "voice gateway",
            "audio codec",
        ],
    ),
    (
        MediaConcern::VideoH264,
        &["h264", "h.264", "rtx_payload", "go live", "golive"],
    ),
    (
        MediaConcern::CallEncryption,
        &["dave", "e2ee", "end-to-end", "mls", "voice encryption"],
    ),
    (
        MediaConcern::ScreenShare,
        &[
            "getdisplaymedia",
            "screencapture",
            "screen share",
            "screenshare",
            "desktopcapturer",
        ],
    ),
    (
        MediaConcern::Camera,
        &[
            "getusermedia",
            "videoinput",
            "webcam",
            "mediadevices",
            "togglecamera",
            "setcamera",
        ],
    ),
    (
        MediaConcern::DeviceSelection,
        &[
            "enumeratedevices",
            "setaudioinput",
            "setaudiooutput",
            "setsinkid",
            "audioinput",
            "audiooutput",
        ],
    ),
    (
        MediaConcern::VolumeControl,
        &[
            "setvolume",
            "gainnode",
            "volumecompensator",
            "participant volume",
            "setlocalvolume",
        ],
    ),
    (
        MediaConcern::AudioPlayback,
        &[
            "newwindow('<audio",
            "createelement('audio",
            "new audio(",
            "<audio",
            "audionode",
        ],
    ),
    (
        MediaConcern::VideoPlayback,
        &[
            "newwindow('<video",
            "createelement('video",
            "<video",
            "videoelement",
        ],
    ),
    (
        MediaConcern::ImageDecode,
        &[
            "createimagebitmap",
            "imagedecoder",
            "offscreencanvas",
            "imagebitmap",
            "apngdecoder",
            "webpdecoder",
        ],
    ),
    (
        MediaConcern::StreamFrameAccess,
        &[
            "rtp packet",
            "addtrack",
            "insertable",
            "srcobject",
            "getusermedia",
            "getdisplaymedia",
            "capturestream",
            "media stream track",
        ],
    ),
    (
        MediaConcern::Transcoding,
        &[
            "transcod",
            "reencode",
            "codec encoder",
            "videoencoder",
            "audioworklet",
        ],
    ),
    (
        MediaConcern::MediaElementInjection,
        &[
            "createelement('video",
            "createelement('audio",
            "outerhtml",
            "insertadjacenthtml",
            "appendchild(video",
        ],
    ),
];

impl MediaReport {
    /// Assesses every concern.
    #[must_use]
    pub fn full() -> Self {
        let verdicts = MediaConcern::ALL
            .iter()
            .map(|&concern| Verdict {
                concern,
                support: concern.support(),
                plugin_reachable: concern.plugin_reachable(),
                native_owner: concern.native_owner(),
                caveat: concern.caveat(),
            })
            .collect();
        Self {
            verdicts,
            detected: None,
        }
    }

    /// Assesses only what a source touched.
    ///
    /// A keyword scan is crude and that is the point: the report exists to tell someone whether a
    /// plugin is worth rewriting at all, not to certify that it is. Comments are blanked first, so a
    /// file that only *mentions* a codec reports nothing.
    #[must_use]
    pub fn scan(source: &str) -> Self {
        let lower = strip_comments(source).to_ascii_lowercase();
        let detected: Vec<MediaConcern> = MARKERS
            .iter()
            .filter(|(_, markers)| markers.iter().any(|marker| lower.contains(marker)))
            .map(|(concern, _)| *concern)
            .collect();

        let mut report = Self::full();
        report.detected = (!detected.is_empty()).then_some(detected);
        report
    }

    /// Concerns in the source that a plugin cannot port.
    #[must_use]
    pub fn blocking(&self) -> Vec<MediaConcern> {
        self.detected
            .as_ref()
            .map(|found| {
                found
                    .iter()
                    .copied()
                    .filter(|c| !c.plugin_reachable())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    /// Whether every concern the source touched can be ported.
    #[must_use]
    pub fn is_portable(&self) -> bool {
        self.blocking().is_empty()
    }

    /// Overall verdict, for a one-line report.
    #[must_use]
    pub fn verdict(&self) -> &'static str {
        if self.detected.is_none() {
            return "NO MEDIA - this source touches nothing a client would need a codec for";
        }
        if self.is_portable() {
            return "PORTABLE - every media concern maps onto a native control";
        }
        "NOT PORTABLE - at least one concern has no mechanism on either side; see below"
    }

    /// Rendered as plain text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "media report");
        let _ = writeln!(out, "  verdict: {}", self.verdict());
        if self.detected.is_none() {
            return out;
        }
        let _ = writeln!(
            out,
            "  detected {} concern(s)",
            self.detected.as_ref().map_or(0, Vec::len)
        );
        for verdict in &self.verdicts {
            let Some(found) = &self.detected else { break };
            if !found.contains(&verdict.concern) {
                continue;
            }
            let reach = if verdict.plugin_reachable {
                "portable"
            } else {
                "blocking"
            };
            let owner = verdict.native_owner.unwrap_or("nothing");
            let _ = writeln!(
                out,
                "    {:<22} {:<8} native owner: {owner}",
                verdict.concern.as_str(),
                reach
            );
            let _ = writeln!(out, "      {}", verdict.caveat);
        }
        out
    }
}

impl fmt::Display for MediaReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

/// Blanks out comments, preserving byte length so offsets still line up.
///
/// A comment *about* a codec is not a codec. Scanning prose makes every report noisier than the thing
/// it reports, and the fix is a few lines rather than a dependency.
fn strip_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            let end = source[i..].find('\n').map_or(bytes.len(), |n| i + n);
            out.push_str(&" ".repeat(end - i));
            i = end;
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let mut j = i + 2;
            while j + 1 < bytes.len() && !(bytes[j] == b'*' && bytes[j + 1] == b'/') {
                j += 1;
            }
            let end = (j + 2).min(bytes.len());
            out.push_str(&" ".repeat(end - i));
            i = end;
        } else {
            let width = utf8_width(bytes[i]);
            out.push_str(&source[i..i + width]);
            i += width;
        }
    }
    out
}

/// How many bytes the character starting with `first` occupies.
///
/// A continuation or invalid byte counts as one, which cannot desynchronize the scan: comment markers
/// are ASCII, so they can only appear at a real character boundary.
fn utf8_width(first: u8) -> usize {
    match first {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_concern_has_a_verdict_and_a_reason() {
        let report = MediaReport::full();
        assert_eq!(report.verdicts.len(), MediaConcern::ALL.len());
        for verdict in &report.verdicts {
            assert!(
                !verdict.caveat.is_empty(),
                "{:?} has no explanation",
                verdict.concern
            );
            if verdict.support == Support::Native {
                assert!(
                    verdict.native_owner.is_some(),
                    "{:?} claims native support",
                    verdict.concern
                );
            }
        }
    }

    #[test]
    fn the_three_out_of_reach_concerns_have_no_owner() {
        for concern in [
            MediaConcern::StreamFrameAccess,
            MediaConcern::Transcoding,
            MediaConcern::MediaElementInjection,
        ] {
            assert_eq!(concern.support(), Support::OutOfReach);
            assert!(concern.native_owner().is_none());
            assert!(!concern.plugin_reachable());
        }
    }

    #[test]
    fn the_natively_owned_controls_are_plugin_reachable() {
        // These are the ones worth porting, so a plugin must actually be able to ask for them.
        for concern in [
            MediaConcern::ScreenShare,
            MediaConcern::Camera,
            MediaConcern::DeviceSelection,
            MediaConcern::VolumeControl,
        ] {
            assert_eq!(concern.support(), Support::Native, "{concern:?}");
            assert!(
                concern.plugin_reachable(),
                "{concern:?} should be reachable"
            );
        }
    }

    #[test]
    fn a_codec_the_client_already_speaks_is_not_a_plugin_reach() {
        // The client decodes these; a plugin still cannot make it decode them, because there is no
        // capability for it. Conflating the two would promise a port that cannot happen.
        for concern in [
            MediaConcern::ImageDecode,
            MediaConcern::AudioPlayback,
            MediaConcern::VideoPlayback,
        ] {
            assert_eq!(concern.support(), Support::Native, "{concern:?}");
            assert!(!concern.plugin_reachable(), "{concern:?}");
        }
    }

    #[test]
    fn a_source_touching_nothing_media_is_reported_as_such() {
        let report = MediaReport::scan("export function start() { SettingsStore.open(); }");
        assert!(report.detected.is_none());
        assert!(report.is_portable());
        assert!(report.render().contains("NO MEDIA"));
    }

    #[test]
    fn a_behavioural_media_mod_is_portable() {
        let source = "export function toggleMute() {
                const device = await navigator.mediaDevices.enumerateDevices();
                Connection.setLocalVolume(0);
                Stream.setAudioSettings({ gain: 1.5 });
            }";
        let report = MediaReport::scan(source);
        assert!(report.detected.is_some());
        assert!(report.is_portable(), "blocking: {:?}", report.blocking());
        assert!(report.verdict().starts_with("PORTABLE"));
    }

    #[test]
    fn a_media_injection_mod_is_not_portable() {
        let source = "export function inject() {
                const video = document.createElement('video');
                video.srcObject = stream;
                document.querySelector('.chat').appendChild(video);
                return video;
            }";
        let report = MediaReport::scan(source);
        let blocking = report.blocking();
        assert!(
            blocking.contains(&MediaConcern::MediaElementInjection),
            "{blocking:?}"
        );
        assert!(
            blocking.contains(&MediaConcern::StreamFrameAccess),
            "{blocking:?}"
        );
        assert!(!report.is_portable());
        assert!(report.render().contains("NOT PORTABLE"));
    }

    #[test]
    fn screen_capture_scan_does_not_fire_on_a_theme() {
        // "camera" as a bare substring is too broad to be useful; a theme never mentions it, but a
        // plugin about avatars should not be reported as touching a codec either.
        let report = MediaReport::scan(":root { --camera-accent: #5865f2; }");
        assert!(
            !report
                .detected
                .as_ref()
                .is_some_and(|f| f.contains(&MediaConcern::Camera))
        );
    }

    #[test]
    fn prose_about_codecs_does_not_count_as_using_one() {
        // A comment mentioning `webp` or `.play()` is not a plugin decoding an image or playing audio.
        // Markers loose enough to catch prose are loose enough to make every report noise.
        let report = MediaReport::scan(
            "/* this plugin does not transcode; it plays a sound and reads a webp */\n\
             export const note = 'the effects subcommand shows capability gates';",
        );
        assert!(report.detected.is_none(), "spurious: {:?}", report.detected);
    }
}
