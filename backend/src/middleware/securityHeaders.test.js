import { test } from "node:test";
import assert from "node:assert/strict";
import { contentSecurityPolicy, securityHeaders } from "./securityHeaders.js";
import { userRateLimit } from "./userRateLimit.js";
import { createAccountRateLimit } from "./accountRateLimit.js";

const response = () => { const headers = {}; return { headers, statusCode: 200, set(name, value) { headers[name] = value; }, status(code) { this.statusCode = code; return this; }, json(body) { this.body = body; return this; } }; };

test("the content policy forbids foreign scripts, framing and plugins", () => {
  const policy = contentSecurityPolicy("sonar.example");
  assert.match(policy, /script-src 'self' 'wasm-unsafe-eval'(;|$)/);
  assert.doesNotMatch(policy.match(/script-src[^;]*/)[0], /'unsafe-inline'| 'unsafe-eval'|\*|https?:/);
  assert.match(policy, /frame-ancestors 'none'/);
  assert.match(policy, /object-src 'none'/);
  assert.match(policy, /connect-src 'self' wss:\/\/sonar\.example ws:\/\/sonar\.example/);
  assert.match(contentSecurityPolicy("evil.example; script-src *"), /connect-src 'self'(;|$)/, "a forged Host header cannot add to the policy");
});

test("every response carries the browser protections, and HSTS only in production", () => {
  for (const production of [true, false]) {
    const res = response();
    let passed = false;
    securityHeaders({ production })({ headers: { host: "sonar.example" } }, res, () => { passed = true; });
    assert.equal(passed, true);
    assert.equal(res.headers["X-Frame-Options"], "DENY");
    assert.equal(res.headers["X-Content-Type-Options"], "nosniff");
    assert.equal(res.headers["Cross-Origin-Opener-Policy"], "same-origin");
    assert.equal(Boolean(res.headers["Strict-Transport-Security"]), production);
  }
});

test("each account has its own request limit that resets after the window", () => {
  let time = 1000;
  const limit = userRateLimit({ limit: 2, windowMs: 1000, now: () => time });
  const attempt = (user) => { const res = response(); let passed = false; limit({ user: { _id: user } }, res, () => { passed = true; }); return passed ? 200 : res.statusCode; };
  assert.deepEqual([attempt("a"), attempt("a"), attempt("a"), attempt("b")], [200, 200, 429, 200]);
  time += 1001;
  assert.equal(attempt("a"), 200);
});

test("sign-in attempts are limited per email, whatever its letter case", () => {
  const limit = createAccountRateLimit({ limit: 2, now: () => 1000 });
  const attempt = (email) => { const res = response(); let passed = false; limit({ body: { email } }, res, () => { passed = true; }); return passed ? 200 : res.statusCode; };
  assert.deepEqual([attempt("Ana@example.invalid"), attempt("ana@example.invalid "), attempt("ANA@example.invalid"), attempt("bo@example.invalid")], [200, 200, 429, 200]);
  assert.equal(attempt({ $ne: "" }), 200, "a non-text email is left for the route to refuse");
});
