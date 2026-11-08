import { test } from "node:test";
import assert from "node:assert/strict";
import { createAuthRateLimit } from "./authRateLimit.js";

test("auth limiter bounds attempts per address and recovers after the window", () => {
  let time = 0;
  const middleware = createAuthRateLimit({ limit: 2, windowMs: 10000, now: () => time });
  const run = (ip) => {
    const result = { status: 200, headers: {} };
    const res = { set: (k, v) => { result.headers[k] = v; }, status: code => { result.status = code; return res; }, json: () => result };
    middleware({ ip }, res, () => {});
    return result;
  };
  assert.equal(run("a").status, 200);
  assert.equal(run("a").status, 200);
  assert.equal(run("a").status, 429);
  assert.equal(run("a").headers["Retry-After"], "10");
  assert.equal(run("b").status, 200);
  time = 10000;
  assert.equal(run("a").status, 200);
});
test("auth limiter fails closed when its bounded storage is full", () => {
  let time = 0;
  const middleware = createAuthRateLimit({ limit: 1, windowMs: 1000, now: () => time, maxKeys: 1 });
  const run = ip => {
    let status = 200;
    const res = { set() {}, status(code) { status = code; return this; }, json() {} };
    middleware({ ip }, res, () => {});
    return status;
  };
  assert.equal(run("a"), 200);
  assert.equal(run("b"), 503);
  time = 1001;
  assert.equal(run("b"), 200);
});
