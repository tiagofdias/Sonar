//! Fetches a channel and turns it into demuxed units: plain MPEG-TS streams
//! (Xtream `.ts` links) and HLS (live or not, AES-128 encrypted or not), always
//! at the highest quality offered. Errors never include the address, which
//! often contains the person's login.
use super::demux::{Demuxer, Unit};
use super::hls::{self, Key, Playlist};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use zeroize::Zeroizing;

#[derive(Debug)]
pub enum Event { Units(Vec<Unit>), Ended, Failed(String) }

const MAX_PLAYLIST: usize = 2 * 1024 * 1024;
const MAX_SEGMENT: usize = 64 * 1024 * 1024;

pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("Sonar-TV/1.0")
        .build()
        .map_err(|_| "Could not start the TV player.".to_string())
}

/// What went wrong, in words, without the address (reqwest errors include it).
pub(crate) fn problem(error: reqwest::Error) -> String {
    match error.status().map(|s| s.as_u16()) {
        Some(401 | 403) => "Your provider refused this channel. Check your login or subscription.".into(),
        Some(404) => "Your provider says this channel does not exist.".into(),
        Some(429) => "Your provider is limiting connections. Close other players using this login.".into(),
        Some(code) if code >= 500 => "Your provider's server had a problem. Try again in a moment.".into(),
        _ if error.is_timeout() => "The channel stopped answering.".into(),
        _ if error.is_connect() => "Could not reach your provider.".into(),
        _ => "The channel could not be loaded.".into(),
    }
}

async fn get_text(client: &reqwest::Client, url: &url::Url) -> Result<(String, url::Url), String> {
    let response = client.get(url.clone()).send().await.and_then(|r| r.error_for_status()).map_err(problem)?;
    let final_url = response.url().clone();
    let bytes = response.bytes().await.map_err(problem)?;
    if bytes.len() > MAX_PLAYLIST { return Err("The channel's playlist is too large.".into()); }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), final_url))
}
pub(crate) async fn get_bytes(client: &reqwest::Client, url: &url::Url, limit: usize) -> Result<Vec<u8>, String> {
    let mut response = client.get(url.clone()).send().await.and_then(|r| r.error_for_status()).map_err(problem)?;
    let mut out = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(problem)? {
        out.extend_from_slice(&chunk);
        if out.len() > limit { return Err("A piece of the channel was too large.".into()); }
    }
    Ok(out)
}

fn decrypt(data: &mut Vec<u8>, key: &[u8; 16], iv: &[u8; 16]) -> Result<(), String> {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    let length = cbc::Decryptor::<aes::Aes128>::new(key.into(), iv.into()).decrypt_padded_mut::<Pkcs7>(data).map_err(|_| "Could not decrypt the channel.".to_string())?.len();
    data.truncate(length);
    Ok(())
}

/// Plays the channel until `stop` is set, sending demuxed units. Backpressure from
/// the bounded channel keeps memory small: nothing is fetched far ahead.
pub async fn run(address: Zeroizing<String>, events: mpsc::Sender<Event>, stop: Arc<AtomicBool>) {
    let result = async {
        let url = url::Url::parse(&address).map_err(|_| "This channel's address is not valid.".to_string())?;
        if !matches!(url.scheme(), "http" | "https") { return Err("This channel's address is not valid.".to_string()); }
        let client = client()?;
        let mut response = client.get(url.clone()).send().await.and_then(|r| r.error_for_status()).map_err(problem)?;
        let base = response.url().clone();
        let first = response.chunk().await.map_err(problem)?.unwrap_or_default();
        if first.starts_with(b"#EXTM3U") || first.starts_with(b"\xEF\xBB\xBF#EXTM3U") {
            let mut text = first.to_vec();
            while let Some(chunk) = response.chunk().await.map_err(problem)? { text.extend_from_slice(&chunk); if text.len() > MAX_PLAYLIST { return Err("The channel's playlist is too large.".into()); } }
            hls_loop(&client, String::from_utf8_lossy(&text).trim_start_matches('\u{feff}').to_owned(), base, &events, &stop).await
        } else {
            ts_loop(&client, &url, first.to_vec(), response, &events, &stop).await
        }
    }.await;
    let _ = events.send(match result { Ok(()) => Event::Ended, Err(message) => Event::Failed(message) }).await;
}

async fn send(events: &mpsc::Sender<Event>, demuxer: &mut Demuxer, bytes: &[u8]) -> bool {
    let mut units = Vec::new();
    demuxer.push(bytes, &mut units);
    units.is_empty() || events.send(Event::Units(units)).await.is_ok()
}

async fn ts_loop(client: &reqwest::Client, url: &url::Url, first: Vec<u8>, mut response: reqwest::Response, events: &mpsc::Sender<Event>, stop: &AtomicBool) -> Result<(), String> {
    let mut demuxer = Demuxer::new();
    if !send(events, &mut demuxer, &first).await { return Ok(()); }
    // A recording has a known length; a live channel does not and only ends when it drops.
    let (mut length, mut received) = (response.content_length(), first.len() as u64);
    let mut retries = 0;
    loop {
        if stop.load(Ordering::Acquire) { return Ok(()); }
        match response.chunk().await {
            Ok(Some(chunk)) => { retries = 0; received += chunk.len() as u64; if !send(events, &mut demuxer, &chunk).await { return Ok(()); } }
            Ok(None) if length.is_some_and(|total| received >= total) => {
                let mut units = Vec::new();
                demuxer.finish(&mut units);
                if !units.is_empty() { let _ = events.send(Event::Units(units)).await; }
                return Ok(());
            }
            Ok(None) | Err(_) => {
                // Live streams drop now and then: reconnect a few times before giving up.
                retries += 1;
                if retries > 5 { return Err("The channel keeps disconnecting.".into()); }
                tokio::time::sleep(Duration::from_millis(500 * retries)).await;
                response = match client.get(url.clone()).send().await.and_then(|r| r.error_for_status()) { Ok(r) => r, Err(e) if retries >= 5 => return Err(problem(e)), Err(_) => continue };
                (length, received) = (response.content_length(), 0);
            }
        }
    }
}

async fn hls_loop(client: &reqwest::Client, text: String, base: url::Url, events: &mpsc::Sender<Event>, stop: &AtomicBool) -> Result<(), String> {
    let mut playlist = hls::parse(&text, &base).map_err(str::to_owned)?;
    let mut media_url = base;
    if let Playlist::Master(variants) = &playlist {
        let best = hls::best(variants).ok_or("The channel lists no qualities.")?.uri.clone();
        let (text, final_url) = get_text(client, &best).await?;
        media_url = final_url;
        playlist = hls::parse(&text, &media_url).map_err(str::to_owned)?;
    }
    let mut demuxer = Demuxer::new();
    let mut keys: HashMap<url::Url, [u8; 16]> = HashMap::new();
    let mut next: Option<u64> = None;
    loop {
        let Playlist::Media(media) = playlist else { return Err("The channel's playlist is not playable.".into()) };
        // Join a live channel three segments from the newest, like players do.
        let start = next.unwrap_or_else(|| {
            let skip = if media.ended { 0 } else { media.segments.len().saturating_sub(3) };
            media.segments.get(skip).map_or(0, |s| s.sequence)
        });
        for segment in media.segments.iter().filter(|s| s.sequence >= start) {
            if stop.load(Ordering::Acquire) { return Ok(()); }
            if segment.map.is_some() { return Err("This channel uses a newer video format (fMP4) that Sonar cannot share yet.".into()); }
            let mut data = get_bytes(client, &segment.uri, MAX_SEGMENT).await?;
            if let Key::Aes128 { uri, .. } = &segment.key {
                let key = match keys.get(uri) {
                    Some(key) => *key,
                    None => {
                        let bytes = get_bytes(client, uri, 64).await?;
                        let key: [u8; 16] = bytes.as_slice().try_into().map_err(|_| "The channel's decryption key is invalid.")?;
                        keys.insert(uri.clone(), key);
                        key
                    }
                };
                decrypt(&mut data, &key, &hls::iv_for(segment))?;
            }
            if !send(events, &mut demuxer, &data).await { return Ok(()); }
            next = Some(segment.sequence + 1);
        }
        if media.ended {
            let mut units = Vec::new();
            demuxer.finish(&mut units);
            if !units.is_empty() { let _ = events.send(Event::Units(units)).await; }
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs_f64((media.target_duration / 2.0).clamp(0.5, 5.0))).await;
        if stop.load(Ordering::Acquire) { return Ok(()); }
        let (text, final_url) = get_text(client, &media_url).await?;
        media_url = final_url;
        playlist = hls::parse(&text, &media_url).map_err(str::to_owned)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn fixture(name: &str) -> Vec<u8> { std::fs::read(format!("{}/tests/tv/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap() }
    /// A tiny local web server that serves fixed paths and records what was asked for.
    fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut request = [0u8; 4096];
                let read = stream.read(&mut request).unwrap_or(0);
                let path = String::from_utf8_lossy(&request[..read]).split_whitespace().nth(1).unwrap_or("").to_owned();
                log.lock().unwrap().push(path.clone());
                let body = routes.iter().find(|(p, _)| *p == path).map(|(_, b)| b.clone());
                let head = match &body { Some(b) => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()), None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into() };
                let _ = stream.write_all(head.as_bytes());
                if let Some(b) = body { let _ = stream.write_all(&b); }
            }
        });
        (address, seen)
    }
    async fn collect(address: String) -> (Vec<Unit>, Option<String>) {
        let (tx, mut rx) = mpsc::channel(8);
        tokio::spawn(run(Zeroizing::new(address), tx, Arc::new(AtomicBool::new(false))));
        let (mut units, mut failed) = (Vec::new(), None);
        while let Some(event) = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.unwrap() {
            match event { Event::Units(u) => units.extend(u), Event::Ended => break, Event::Failed(m) => { failed = Some(m); break } }
        }
        (units, failed)
    }
    fn video_bytes(units: &[Unit]) -> Vec<u8> { units.iter().filter_map(|u| if let Unit::Video { data, .. } = u { Some(data.clone()) } else { None }).flatten().collect() }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hls_takes_the_best_quality_and_decrypts_aes_segments() {
        use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        let ts = fixture("h264_high_50fps.ts");
        let half = (ts.len() / 188 / 2) * 188;
        let key = [7u8; 16];
        let iv = { let mut iv = [0u8; 16]; iv[15] = 1; iv };
        let encrypted = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &iv.into()).encrypt_padded_vec_mut::<Pkcs7>(&ts[half..]);
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100000\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=9000000,RESOLUTION=1920x1080\nhigh.m3u8\n";
        let high = "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:0.5,\nseg0.ts\n#EXT-X-KEY:METHOD=AES-128,URI=\"/key\"\n#EXTINF:0.5,\nseg1.ts\n#EXT-X-ENDLIST\n";
        let (address, seen) = serve(vec![("/master.m3u8", master.into()), ("/high.m3u8", high.into()), ("/seg0.ts", ts[..half].to_vec()), ("/seg1.ts", encrypted), ("/key", key.to_vec())]);
        let (units, failed) = collect(format!("{address}/master.m3u8")).await;
        assert_eq!(failed, None);
        assert_eq!(video_bytes(&units), fixture("h264_high_50fps.h264"), "every frame arrives unchanged, including the decrypted half");
        let asked = seen.lock().unwrap().clone();
        assert!(asked.contains(&"/high.m3u8".to_owned()) && !asked.contains(&"/low.m3u8".to_owned()), "only the best quality is fetched: {asked:?}");
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn xtream_ts_streams_play_and_errors_hide_the_address() {
        let (address, _) = serve(vec![("/live/ana/s3cret/101.ts", fixture("h264_high_50fps.ts"))]);
        let (units, _) = collect(format!("{address}/live/ana/s3cret/101.ts")).await;
        assert_eq!(video_bytes(&units), fixture("h264_high_50fps.h264"));
        let (_, failed) = collect(format!("{address}/live/ana/s3cret/999.ts")).await;
        let message = failed.expect("a missing channel fails");
        assert!(!message.contains("s3cret") && !message.contains("127.0.0.1"), "{message}");
        assert!(message.contains("does not exist"), "{message}");
    }
}
