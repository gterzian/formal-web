//! The few facts the W3C algorithms read from a session description: its
//! media descriptions, their mids and ICE username fragments, and whether one
//! of them negotiates data. The full parse ([[RFC9429]]) runs in the WebRTC
//! process.

/// One media description (`m=` section) of a session description.
pub(crate) struct MediaDescription {
    /// The media type of the `m=` line (`audio`, `video`, `application`).
    pub(crate) media: String,
    /// The `a=mid` value, when present.
    pub(crate) mid: Option<String>,
    /// The `a=ice-ufrag` value, from the section or the session level.
    pub(crate) ice_ufrag: Option<String>,
}

/// The media descriptions of `sdp`, in order.
pub(crate) fn media_descriptions(sdp: &str) -> Vec<MediaDescription> {
    let mut session_ufrag = None;
    let mut sections: Vec<MediaDescription> = Vec::new();
    for line in sdp.lines().map(str::trim_end) {
        if let Some(rest) = line.strip_prefix("m=") {
            sections.push(MediaDescription {
                media: rest.split(' ').next().unwrap_or_default().to_owned(),
                mid: None,
                ice_ufrag: None,
            });
        } else if let Some(mid) = line.strip_prefix("a=mid:") {
            if let Some(section) = sections.last_mut() {
                section.mid = Some(mid.to_owned());
            }
        } else if let Some(ufrag) = line.strip_prefix("a=ice-ufrag:") {
            match sections.last_mut() {
                Some(section) => section.ice_ufrag = Some(ufrag.to_owned()),
                None => session_ufrag = Some(ufrag.to_owned()),
            }
        }
    }
    for section in &mut sections {
        if section.ice_ufrag.is_none() {
            section.ice_ufrag = session_ufrag.clone();
        }
    }
    sections
}

/// Whether some media description of `sdp` negotiates data (an
/// `m=application` section for SCTP).
pub(crate) fn negotiates_data(sdp: &str) -> bool {
    media_descriptions(sdp)
        .iter()
        .any(|section| section.media == "application")
}
