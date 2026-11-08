use rtc::peer_connection::transport::RTCIceCandidateInit;

pub fn valid_remote_candidate(candidate: &RTCIceCandidateInit) -> bool {
    let valid_optional = |value: &Option<String>| value.as_ref().is_none_or(|s| s.len() <= 256 && !s.contains(['\r', '\n']));
    candidate.candidate.starts_with("candidate:") && candidate.candidate.len() <= 4096 && !candidate.candidate.contains(['\r', '\n']) && valid_optional(&candidate.sdp_mid) && valid_optional(&candidate.username_fragment) && candidate.url.is_none() && (candidate.sdp_mid.is_some() || candidate.sdp_mline_index.is_some())
}
