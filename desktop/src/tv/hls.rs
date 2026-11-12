//! HLS playlists: the master list of qualities and the live list of segments.
//! Sonar always takes the highest quality (most bandwidth) the channel offers.
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub struct Variant { pub uri: Url, pub bandwidth: u64, pub resolution: Option<(u32, u32)>, pub codecs: Option<String> }

#[derive(Debug, Clone, PartialEq)]
pub enum Key { None, Aes128 { uri: Url, iv: Option<[u8; 16]> } }

#[derive(Debug, Clone, PartialEq)]
pub struct Segment { pub sequence: u64, pub uri: Url, pub duration: f64, pub key: Key, pub map: Option<Url> }

#[derive(Debug, Clone, PartialEq)]
pub struct Media { pub target_duration: f64, pub segments: Vec<Segment>, pub ended: bool }

#[derive(Debug, Clone, PartialEq)]
pub enum Playlist { Master(Vec<Variant>), Media(Media) }

fn attributes(text: &str) -> Vec<(String, String)> {
    // KEY=VALUE pairs separated by commas; values may be quoted and contain commas.
    let mut out = Vec::new();
    let (mut key, mut value, mut in_key, mut quoted) = (String::new(), String::new(), true, false);
    for c in text.chars() {
        match (in_key, c) {
            (true, '=') => in_key = false,
            (true, ',') => key.clear(),
            (true, _) => key.push(c),
            (false, '"') => quoted = !quoted,
            (false, ',') if !quoted => { out.push((key.trim().to_ascii_uppercase(), std::mem::take(&mut value))); key.clear(); in_key = true; }
            (false, _) => value.push(c),
        }
    }
    if !key.is_empty() { out.push((key.trim().to_ascii_uppercase(), value)); }
    out
}
fn get<'a>(list: &'a [(String, String)], name: &str) -> Option<&'a str> { list.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()) }
fn resolve(base: &Url, uri: &str) -> Option<Url> {
    let url = base.join(uri.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}
fn hex_iv(text: &str) -> Option<[u8; 16]> {
    let hex = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"))?;
    let padded = format!("{hex:0>32}");
    if padded.len() != 32 { return None; }
    let mut iv = [0u8; 16];
    for (i, byte) in iv.iter_mut().enumerate() { *byte = u8::from_str_radix(&padded[i * 2..i * 2 + 2], 16).ok()?; }
    Some(iv)
}

pub fn parse(text: &str, base: &Url) -> Result<Playlist, &'static str> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some("#EXTM3U") { return Err("Not an HLS playlist"); }
    let mut variants = Vec::new();
    let mut pending_variant: Option<Vec<(String, String)>> = None;
    let (mut target, mut sequence, mut ended) = (6.0, 0u64, false);
    let (mut duration, mut key, mut map) = (None::<f64>, Key::None, None::<Url>);
    let mut segments = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()).skip(1) {
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") { pending_variant = Some(attributes(rest)); }
        else if let Some(rest) = line.strip_prefix("#EXT-X-TARGETDURATION:") { target = rest.parse().unwrap_or(6.0); }
        else if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") { sequence = rest.parse().unwrap_or(0); }
        else if line == "#EXT-X-ENDLIST" { ended = true; }
        else if let Some(rest) = line.strip_prefix("#EXTINF:") { duration = rest.split(',').next().and_then(|d| d.parse().ok()); }
        else if let Some(rest) = line.strip_prefix("#EXT-X-KEY:") {
            let list = attributes(rest);
            key = match get(&list, "METHOD") {
                Some("NONE") | None => Key::None,
                Some("AES-128") => Key::Aes128 { uri: get(&list, "URI").and_then(|u| resolve(base, u)).ok_or("The channel's encryption key address is invalid")?, iv: get(&list, "IV").and_then(hex_iv) },
                Some(_) => return Err("This channel uses a kind of encryption Sonar cannot play (DRM)"),
            };
        }
        else if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") { map = get(&attributes(rest), "URI").and_then(|u| resolve(base, u)); }
        else if line.starts_with('#') { continue; }
        else if let Some(list) = pending_variant.take() {
            if let Some(uri) = resolve(base, line) {
                let resolution = get(&list, "RESOLUTION").and_then(|r| r.split_once('x')).and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
                variants.push(Variant { uri, bandwidth: get(&list, "BANDWIDTH").and_then(|b| b.parse().ok()).unwrap_or(0), resolution, codecs: get(&list, "CODECS").map(str::to_owned) });
            }
        } else if let Some(uri) = resolve(base, line) {
            segments.push(Segment { sequence, uri, duration: duration.take().unwrap_or(target), key: key.clone(), map: map.clone() });
            sequence += 1;
        }
    }
    if !variants.is_empty() { return Ok(Playlist::Master(variants)); }
    Ok(Playlist::Media(Media { target_duration: target, segments, ended }))
}

/// The highest quality: most bandwidth, then largest picture.
pub fn best(variants: &[Variant]) -> Option<&Variant> {
    variants.iter().max_by_key(|v| (v.bandwidth, v.resolution.map_or(0, |(w, h)| w as u64 * h as u64)))
}

/// AES-128 segments use the media sequence number as the IV when none is given.
pub fn iv_for(segment: &Segment) -> [u8; 16] {
    match &segment.key { Key::Aes128 { iv: Some(iv), .. } => *iv, _ => { let mut iv = [0u8; 16]; iv[8..].copy_from_slice(&segment.sequence.to_be_bytes()); iv } }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> Url { Url::parse("https://cdn.example/live/channel/index.m3u8?token=abc").unwrap() }
    #[test]
    fn the_highest_quality_variant_is_chosen() {
        let text = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1200000,RESOLUTION=1280x720,CODECS=\"avc1.4d401f,mp4a.40.2\"\n720/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=6500000,RESOLUTION=1920x1080,CODECS=\"avc1.640028,mp4a.40.2\"\nhttps://other.example/1080.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=600000\n/low.m3u8\n";
        let Playlist::Master(variants) = parse(text, &base()).unwrap() else { panic!("expected a master playlist") };
        assert_eq!(variants.len(), 3);
        assert_eq!(variants[0].uri.as_str(), "https://cdn.example/live/channel/720/index.m3u8");
        assert_eq!(variants[2].uri.as_str(), "https://cdn.example/low.m3u8");
        let top = best(&variants).unwrap();
        assert_eq!((top.bandwidth, top.resolution), (6_500_000, Some((1920, 1080))));
        assert_eq!(top.codecs.as_deref(), Some("avc1.640028,mp4a.40.2"));
    }
    #[test]
    fn live_media_playlists_list_segments_keys_and_sequence() {
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:500\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x0000000000000000000000000000ABCD\n#EXTINF:4.004,\nseg500.ts\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:3.9,title, with comma\nseg501.ts\n";
        let Playlist::Media(media) = parse(text, &base()).unwrap() else { panic!("expected a media playlist") };
        assert_eq!(media.target_duration, 4.0);
        assert!(!media.ended);
        assert_eq!(media.segments.iter().map(|s| (s.sequence, s.uri.path().to_owned(), s.duration)).collect::<Vec<_>>(), [(500, "/live/channel/seg500.ts".to_owned(), 4.004), (501, "/live/channel/seg501.ts".to_owned(), 3.9)]);
        assert!(matches!(&media.segments[0].key, Key::Aes128 { uri, .. } if uri.path() == "/live/channel/key.bin"));
        assert_eq!(iv_for(&media.segments[0])[14..], [0xAB, 0xCD]);
        assert_eq!(media.segments[1].key, Key::None);
        let mut fallback = media.segments[0].clone();
        fallback.key = Key::Aes128 { uri: base(), iv: None };
        assert_eq!(iv_for(&fallback)[8..], 500u64.to_be_bytes());
    }
    #[test]
    fn drm_and_junk_are_refused() {
        assert!(parse("#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\"\n#EXTINF:4,\na.ts\n", &base()).is_err());
        assert!(parse("<html>", &base()).is_err());
        let Playlist::Media(media) = parse("#EXTM3U\n#EXTINF:4,\njavascript:alert(1)\n#EXTINF:4,\nfile:///C:/x.ts\n", &base()).unwrap() else { panic!() };
        assert!(media.segments.is_empty(), "only http and https segments");
    }
}
