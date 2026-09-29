use js_engine::gc_struct;

/// <https://w3c.github.io/webrtc-pc/#dom-rtcstatsreport>
#[gc_struct]
pub(crate) struct RTCStatsReport {
    /// The report's entries: each stats object's id and its JSON text, in
    /// the order the WebRTC process reported them.
    /// <https://w3c.github.io/webrtc-pc/#dfn-stats-object>
    #[ignore_trace]
    entries: Vec<(String, String)>,
}

impl RTCStatsReport {
    /// <https://w3c.github.io/webrtc-pc/#dfn-gather-the-stats>
    pub(crate) fn from_json(report: &str) -> Self {
        // The stats report is a map: each entry's key is the id member of
        // the stats object it holds.
        let entries = match serde_json::from_str::<serde_json::Value>(report) {
            Ok(serde_json::Value::Object(map)) => map
                .into_iter()
                .map(|(id, value)| (id, value.to_string()))
                .collect(),
            Ok(serde_json::Value::Array(values)) => values
                .into_iter()
                .filter_map(|value| {
                    let id = value.get("id")?.as_str()?.to_owned();
                    Some((id, value.to_string()))
                })
                .collect(),
            Ok(_) | Err(_) => Vec::new(),
        };
        Self { entries }
    }

    /// <https://w3c.github.io/webrtc-pc/#dom-rtcstatsreport>
    pub(crate) fn entries(&self) -> &[(String, String)] {
        // The RTCStatsReport is a maplike whose value pairs are the stats
        // objects by id.
        &self.entries
    }
}
