//! Sonar TV: plays the person's own IPTV channels and shares the original stream
//! (not a screen recording) with the people in their call or voice room.
#![allow(dead_code)]
pub mod audio;
pub mod demux;
pub mod fetch;
pub mod hls;
pub mod player;
pub mod sources;
