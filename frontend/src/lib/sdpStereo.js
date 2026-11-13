// Adds stereo to the Opus line of an SDP answer, so a music or TV share keeps both channels.
export function preferStereo(sdp) {
  const opus = sdp.match(/a=rtpmap:(\d+) opus\/48000\/2/i)?.[1];
  if (!opus) return sdp;
  return sdp.replace(new RegExp(`(a=fmtp:${opus} [^\\r\\n]*)`), (line) => (line.includes("stereo=1") ? line : `${line};stereo=1;sprop-stereo=1`));
}
