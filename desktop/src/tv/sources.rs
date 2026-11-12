//! TV sources: M3U playlists and Xtream Codes logins. They stay on this PC,
//! encrypted with Windows DPAPI, and are never sent to Sonar's server or to the
//! people watching a share. Errors and debug output never include addresses.
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SourceKind {
    M3u { url: String },
    Xtream { server: String, username: String, password: String },
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
    /// Names of the channels marked as favorites, shown first when the source opens.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub favorites: Vec<String>,
}

// Addresses and logins are private: never print them.
impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.kind { SourceKind::M3u { .. } => "m3u", SourceKind::Xtream { .. } => "xtream" };
        f.debug_struct("Source").field("id", &self.id).field("name", &self.name).field("kind", &kind).field("favorites", &self.favorites.len()).finish()
    }
}

#[derive(Clone, PartialEq)]
pub struct Channel { pub name: String, pub group: String, url: String }
impl Channel {
    pub fn new(name: impl Into<String>, group: impl Into<String>, url: impl Into<String>) -> Self { Self { name: name.into(), group: group.into(), url: url.into() } }
    /// The stream address, which may contain the login. Only the player reads it.
    pub(crate) fn url(&self) -> &str { &self.url }
}
impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel").field("name", &self.name).field("group", &self.group).finish_non_exhaustive()
    }
}

const MAX_CHANNELS: usize = 20_000;
const MAX_NAME: usize = 120;

fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect::<String>().trim().to_owned()
}
fn stream_url(text: &str) -> Option<String> {
    let url = url::Url::parse(text.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}
fn attribute<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let start = line.find(&format!("{name}=\""))? + name.len() + 2;
    let end = line[start..].find('"')? + start;
    Some(&line[start..end])
}

/// Channels from an M3U / M3U8 playlist (`#EXTINF` lines followed by a stream address).
pub fn parse_m3u(text: &str) -> Vec<Channel> {
    let mut channels = Vec::new();
    let (mut name, mut group) = (None::<String>, String::new());
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            let title = info.rsplit_once(',').map(|(_, title)| clean(title)).unwrap_or_default();
            name = Some(if title.is_empty() { attribute(info, "tvg-name").map(clean).unwrap_or_default() } else { title });
            group = attribute(info, "group-title").map(clean).unwrap_or_default();
        } else if let Some(value) = line.strip_prefix("#EXTGRP:") {
            group = clean(value);
        } else if !line.is_empty() && !line.starts_with('#') {
            if let Some(url) = stream_url(line) {
                let title = name.take().filter(|n| !n.is_empty()).unwrap_or_else(|| format!("Channel {}", channels.len() + 1));
                channels.push(Channel::new(title, std::mem::take(&mut group), url));
                if channels.len() >= MAX_CHANNELS { break; }
            }
            name = None;
        }
    }
    channels
}

/// The Xtream Codes server address, with no path, query or login in it.
pub fn xtream_base(server: &str) -> Result<url::Url, &'static str> {
    let text = if server.contains("://") { server.trim().to_owned() } else { format!("http://{}", server.trim()) };
    let mut url = url::Url::parse(&text).map_err(|_| "Enter the server address from your provider, like http://example.com:8080")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() { return Err("Enter the server address from your provider, like http://example.com:8080"); }
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    let _ = url.set_username("");
    let _ = url.set_password(None);
    Ok(url)
}
fn xtream_part(value: &str) -> Result<&str, &'static str> {
    if value.is_empty() || value.len() > 200 || value.chars().any(|c| c.is_control() || c == '/' || c == '?' || c == '#') { return Err("That username or password has characters Xtream logins cannot use."); }
    Ok(value)
}
/// The API address that lists live channels for a login.
pub fn xtream_api(server: &str, username: &str, password: &str, action: &str) -> Result<Zeroizing<String>, &'static str> {
    let mut url = xtream_base(server)?.join("player_api.php").map_err(|_| "Invalid server address")?;
    url.query_pairs_mut().append_pair("username", xtream_part(username)?).append_pair("password", xtream_part(password)?).append_pair("action", action);
    Ok(Zeroizing::new(url.to_string()))
}

#[derive(Deserialize)]
struct XtreamCategory { category_id: serde_json::Value, category_name: Option<String> }
#[derive(Deserialize)]
struct XtreamStream { name: Option<String>, stream_id: serde_json::Value, category_id: Option<serde_json::Value> }
fn id_text(value: &serde_json::Value) -> Option<String> {
    match value { serde_json::Value::Number(n) => Some(n.to_string()), serde_json::Value::String(s) if s.bytes().all(|b| b.is_ascii_digit()) && !s.is_empty() && s.len() < 20 => Some(s.clone()), _ => None }
}
/// Channels from the Xtream `get_live_categories` and `get_live_streams` answers.
pub fn parse_xtream(server: &str, username: &str, password: &str, categories: &str, streams: &str) -> Result<Vec<Channel>, &'static str> {
    let base = xtream_base(server)?;
    let (user, pass) = (xtream_part(username)?, xtream_part(password)?);
    let groups: std::collections::HashMap<String, String> = serde_json::from_str::<Vec<XtreamCategory>>(categories).unwrap_or_default().into_iter()
        .filter_map(|c| Some((id_text(&c.category_id)?, clean(&c.category_name.unwrap_or_default())))).collect();
    let list: Vec<XtreamStream> = serde_json::from_str(streams).map_err(|_| "The provider sent a channel list Sonar could not read.")?;
    let mut channels = Vec::new();
    for stream in list.into_iter().take(MAX_CHANNELS) {
        let Some(id) = id_text(&stream.stream_id) else { continue };
        let group = stream.category_id.as_ref().and_then(id_text).and_then(|c| groups.get(&c).cloned()).unwrap_or_default();
        let mut url = base.clone();
        url.path_segments_mut().map_err(|_| "Invalid server address")?.extend(["live", user, pass, &format!("{id}.ts")]);
        channels.push(Channel::new(clean(&stream.name.unwrap_or_default()), group, url.to_string()));
    }
    Ok(channels)
}

// ----- Sorting favorites into sections ----------------------------------------

/// The sections favorites are grouped into, in the order they are shown. A keyword
/// matches the start of a word in the channel name; one ending in a space must be the whole word.
const KINDS: [(&str, &[&str]); 7] = [
    ("Sports", &["sport", "eleven", "dazn", "btv", "benfica", "canal 11", "a bola", "espn", "bein", "eurosport", "nba", "nfl", "f1 ", "golf", "tennis", "fight", "futebol", "football", "liga", "motor"]),
    ("News", &["news", "noticias", "cnn", "euronews", "rtp3", "rtp 3", "tvi24", "tvi 24", "cmtv", "bloomberg", "cnbc", "al jazeera", "france 24", "dw ", "parlamento"]),
    ("Kids", &["disney", "panda", "cartoon", "nick", "kids", "baby", "junior", "jr ", "boomerang", "cbeebies", "biggs", "sic k "]),
    ("Documentaries", &["discovery", "geographic", "nat geo", "history", "historia", "odisseia", "animal", "planet", "travel", "24kitchen", "food", "documentar", "science", "investigation"]),
    ("Music", &["music", "musica", "mtv", "vh1", "trace", "mezzo", "kiss"]),
    ("Movies & series", &["cine", "tvcine", "cinemundo", "movie", "film", "hollywood", "axn", "fox", "amc", "hbo", "syfy", "series", "comedy", "paramount", "star movies", "star channel", "star life", "studio"]),
    ("General", &["rtp", "sic ", "tvi", "porto canal", "globo", "record", "bbc one", "itv", "tf1", "rai "]),
];

fn plain_words(text: &str) -> String {
    let mut out = String::from(" ");
    for c in text.to_lowercase().chars() {
        out.push(match c { 'á' | 'à' | 'â' | 'ã' | 'ä' => 'a', 'é' | 'è' | 'ê' => 'e', 'í' | 'ì' => 'i', 'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o', 'ú' | 'ù' | 'ü' => 'u', 'ç' => 'c', c if c.is_alphanumeric() => c, _ => ' ' });
    }
    out.push(' ');
    out
}

/// The section a channel belongs in (its rank, then its name): by what its name says it
/// shows, or else the provider's own category.
pub fn section(channel: &Channel) -> (usize, String) {
    let words = plain_words(&channel.name);
    for (rank, (kind, keywords)) in KINDS.iter().enumerate() {
        if keywords.iter().any(|k| words.contains(&format!(" {k}"))) { return (rank, (*kind).to_owned()); }
    }
    (KINDS.len(), if channel.group.is_empty() { "Other".to_owned() } else { channel.group.clone() })
}

/// Name order with numbers compared as numbers: "Sport TV 2" before "Sport TV 10".
pub fn natural_order(a: &str, b: &str) -> std::cmp::Ordering {
    fn parts(text: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in text.to_lowercase().chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() { Some((d, part)) if *d == digit => part.push(c), _ => out.push((digit, c.to_string())) }
        }
        out
    }
    let (x, y) = (parts(a), parts(b));
    for (p, q) in x.iter().zip(&y) {
        let order = if p.0 && q.0 { let (m, n) = (p.1.trim_start_matches('0'), q.1.trim_start_matches('0')); m.len().cmp(&n.len()).then_with(|| m.cmp(n)) } else { p.1.cmp(&q.1) };
        if order.is_ne() { return order; }
    }
    x.len().cmp(&y.len())
}

/// The order the TV panel lists channels in first: favorites in their sections, or
/// (with no favorites) every channel. Next / previous channel follow it.
pub fn play_order(channels: &[Channel], favorites: &[String]) -> Vec<usize> {
    if favorites.is_empty() { return (0..channels.len()).collect(); }
    let wanted: std::collections::HashSet<&str> = favorites.iter().map(String::as_str).collect();
    let mut list: Vec<(usize, String, usize)> = channels.iter().enumerate().filter(|(_, c)| wanted.contains(c.name.as_str())).map(|(i, c)| { let (rank, kind) = section(c); (rank, kind, i) }).collect();
    list.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)).then_with(|| natural_order(&channels[a.2].name, &channels[b.2].name)));
    list.into_iter().map(|(_, _, i)| i).collect()
}

// ----- Loading a source's channel list ---------------------------------------------

const MAX_LIST: usize = 40 * 1024 * 1024;

/// Downloads a source's channels from the provider (never from Sonar's server).
pub async fn load_channels(source: &Source) -> Result<Vec<Channel>, String> {
    let client = super::fetch::client()?;
    let text = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).into_owned();
    match &source.kind {
        SourceKind::M3u { url } => {
            let url = url::Url::parse(url.trim()).ok().filter(|u| matches!(u.scheme(), "http" | "https")).ok_or("That playlist address is not valid.")?;
            let list = parse_m3u(&text(super::fetch::get_bytes(&client, &url, MAX_LIST).await?));
            if list.is_empty() { return Err("No channels were found in that playlist.".into()); }
            Ok(list)
        }
        SourceKind::Xtream { server, username, password } => {
            let api = |action: &str| -> Result<url::Url, String> { url::Url::parse(xtream_api(server, username, password, action)?.as_str()).map_err(|_| "Invalid server address".to_string()) };
            let streams = text(super::fetch::get_bytes(&client, &api("get_live_streams")?, MAX_LIST).await?);
            // A refused login answers with account details marked "auth":0 instead of a list.
            if streams.trim_start().starts_with('{') { return Err("Your provider did not accept that username and password.".into()); }
            let categories = text(super::fetch::get_bytes(&client, &api("get_live_categories")?, MAX_LIST).await.unwrap_or_default());
            let list = parse_xtream(server, username, password, &categories, &streams).map_err(str::to_owned)?;
            if list.is_empty() { return Err("Your provider sent no live channels for this login.".into()); }
            Ok(list)
        }
    }
}

// ----- Saved on this PC only, encrypted with DPAPI ------------------------------

const SCOPE: &str = "sonar-tv-sources-v1";
fn file() -> std::path::PathBuf { crate::store::root().join("tv-sources.bin") }

#[cfg(windows)]
pub fn load() -> Vec<Source> {
    let Ok(bytes) = std::fs::read(file()) else { return Vec::new() };
    let Ok(plain) = crate::key_store::crypt(&bytes, SCOPE, false) else { return Vec::new() };
    serde_json::from_slice(&plain).unwrap_or_default()
}
#[cfg(windows)]
pub fn save(sources: &[Source]) -> Result<(), String> {
    let plain = Zeroizing::new(serde_json::to_vec(sources).map_err(|_| "Could not save your TV sources.")?);
    if plain.len() > 256 * 1024 { return Err("Too many TV sources saved. Remove one first.".into()); }
    let sealed = crate::key_store::crypt(&plain, SCOPE, true).map_err(|_| "Could not protect your TV sources on this PC.")?;
    let _ = std::fs::create_dir_all(crate::store::root());
    std::fs::write(file(), sealed.as_slice()).map_err(|_| "Could not save your TV sources.".to_string())
}
#[cfg(not(windows))]
pub fn load() -> Vec<Source> { Vec::new() }
#[cfg(not(windows))]
pub fn save(_: &[Source]) -> Result<(), String> { Err("Saving TV sources needs Windows.".into()) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn m3u_playlists_give_names_groups_and_addresses() {
        let text = "#EXTM3U\r\n#EXTINF:-1 tvg-id=\"a\" tvg-name=\"Alpha HD\" group-title=\"News\",Alpha HD\r\nhttp://tv.example/live/1.m3u8\r\n#EXTINF:-1 group-title=\"Sport\",\r\nhttps://tv.example/2.ts\r\n#EXTINF:-1,Broken\r\nnot a url\r\n#EXTINF:-1,Third\r\n#EXTGRP:Kids\r\nhttp://tv.example/3.ts\r\n";
        let channels = parse_m3u(text);
        assert_eq!(channels.iter().map(|c| (c.name.as_str(), c.group.as_str())).collect::<Vec<_>>(), [("Alpha HD", "News"), ("Channel 2", "Sport"), ("Third", "Kids")]);
        assert_eq!(channels[0].url(), "http://tv.example/live/1.m3u8");
        assert!(parse_m3u("#EXTINF:-1,X\njavascript:alert(1)\nfile:///C:/x\n").is_empty(), "only http and https streams");
    }
    #[test]
    fn xtream_logins_build_channel_addresses_without_leaking_them() {
        let categories = r#"[{"category_id":"7","category_name":"Desporto"}]"#;
        let streams = r#"[{"name":"Sport 1","stream_id":101,"category_id":"7"},{"name":"Bad","stream_id":"../x"}]"#;
        let channels = parse_xtream("tv.example:8080/some/path?x=1", "ana", "s3cret", categories, streams).unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].url(), "http://tv.example:8080/live/ana/s3cret/101.ts");
        assert_eq!(channels[0].group, "Desporto");
        let printed = format!("{:?} {:?}", channels[0], Source { id: "1".into(), name: "Mine".into(), kind: SourceKind::Xtream { server: "tv.example".into(), username: "ana".into(), password: "s3cret".into() }, favorites: Vec::new() });
        assert!(!printed.contains("s3cret") && !printed.contains("ana") && !printed.contains("tv.example"), "{printed}");
        assert_eq!(xtream_api("https://tv.example", "ana", "p&w", "get_live_streams").unwrap().as_str(), "https://tv.example/player_api.php?username=ana&password=p%26w&action=get_live_streams");
        assert!(xtream_api("tv.example", "an/a", "x", "a").is_err());
        assert!(xtream_base("ftp://tv.example").is_err());
    }
    #[test]
    fn favorites_are_sorted_into_sections_by_what_they_show() {
        let kind = |name: &str, group: &str| section(&Channel::new(name, group, "http://x/1.ts")).1;
        assert_eq!(kind("PT: SPORT TV 1 FHD", "Portugal"), "Sports");
        assert_eq!(kind("SIC Notícias HD", "Portugal"), "News");
        assert_eq!(kind("SIC HD", "Portugal"), "General");
        assert_eq!(kind("RTP 1", "Portugal"), "General");
        assert_eq!(kind("Disney Channel", ""), "Kids");
        assert_eq!(kind("National Geographic", ""), "Documentaries");
        assert_eq!(kind("TVCine Top", ""), "Movies & series");
        assert_eq!(kind("MTV Portugal", ""), "Music");
        assert_eq!(kind("Canal Q", "Entretenimento"), "Entretenimento");
        assert_eq!(kind("Mystery", ""), "Other");
        let mut names = ["Sport TV 10", "Sport TV 2", "sport tv 1", "Sport TV+"];
        names.sort_by(|a, b| natural_order(a, b));
        assert_eq!(names, ["sport tv 1", "Sport TV 2", "Sport TV 10", "Sport TV+"]);
    }
    #[test]
    fn next_channel_follows_the_favorites_as_listed() {
        let channels: Vec<Channel> = ["Sport TV 2", "RTP 1", "Sport TV 1", "CNN Portugal", "Cinema"].iter().map(|n| Channel::new(*n, "", "http://x/1.ts")).collect();
        let names = |order: Vec<usize>| order.into_iter().map(|i| channels[i].name.clone()).collect::<Vec<_>>();
        assert_eq!(names(play_order(&channels, &[])), ["Sport TV 2", "RTP 1", "Sport TV 1", "CNN Portugal", "Cinema"]);
        assert_eq!(names(play_order(&channels, &["RTP 1".into(), "Sport TV 2".into(), "CNN Portugal".into(), "Sport TV 1".into()])), ["Sport TV 1", "Sport TV 2", "CNN Portugal", "RTP 1"]);
    }
    #[test]
    fn sources_saved_before_favorites_still_load() {
        let old: Source = serde_json::from_str(r#"{"id":"1","name":"Mine","kind":{"kind":"m3u","url":"http://tv.example/list.m3u"}}"#).unwrap();
        assert!(old.favorites.is_empty());
        let mut source = old.clone();
        source.favorites.push("Sport 1".into());
        let again: Source = serde_json::from_slice(&serde_json::to_vec(&source).unwrap()).unwrap();
        assert_eq!(again.favorites, ["Sport 1"]);
        assert!(!serde_json::to_string(&old).unwrap().contains("favorites"));
    }
}
