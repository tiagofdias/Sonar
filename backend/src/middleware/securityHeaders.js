// Browser protections for every response. The web client loads nothing from other
// sites, so the policy can be strict: an injected script could otherwise use the
// encryption keys held in the page.
export function contentSecurityPolicy(host) {
  // 'self' covers same-origin WebSockets in current browsers; the explicit wss: entry covers older ones.
  const socket = host && /^[a-z0-9.-]+(:\d{1,5})?$/i.test(host) ? ` wss://${host} ws://${host}` : "";
  return [
    "default-src 'self'",
    // 'wasm-unsafe-eval' only allows compiling WebAssembly (the noise filter), not eval().
    "script-src 'self' 'wasm-unsafe-eval'",
    // The toast library and React add small inline styles; scripts stay strict.
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob:",
    "media-src 'self' blob: data:",
    "font-src 'self' data:",
    `connect-src 'self'${socket}`,
    "worker-src 'self' blob:",
    "manifest-src 'self'",
    "object-src 'none'",
    "base-uri 'self'",
    "form-action 'self'",
    "frame-ancestors 'none'",
  ].join("; ");
}
export function securityHeaders({ production = process.env.NODE_ENV === "production" } = {}) {
  return (req, res, next) => {
    res.set("Content-Security-Policy", contentSecurityPolicy(req.headers.host));
    res.set("X-Frame-Options", "DENY");
    res.set("X-Content-Type-Options", "nosniff");
    res.set("Referrer-Policy", "no-referrer");
    res.set("Cross-Origin-Opener-Policy", "same-origin");
    res.set("Cross-Origin-Resource-Policy", "same-origin");
    res.set("Permissions-Policy", "camera=(), geolocation=(), payment=(), usb=(), microphone=(self), display-capture=(self), publickey-credentials-get=(self), publickey-credentials-create=(self)");
    // Browsers keep using HTTPS for this site for a year once they have seen it.
    if (production) res.set("Strict-Transport-Security", "max-age=31536000; includeSubDomains");
    next();
  };
}
