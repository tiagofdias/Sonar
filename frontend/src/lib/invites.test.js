import { test } from "node:test";
import assert from "node:assert/strict";
import { findInvite } from "./invites.js";

const token = "ab".repeat(24);

test("finds invite links in chat messages", () => {
  assert.deepEqual(findInvite(`http://localhost:5173/?invite=${token}`), { token, only: true });
  assert.deepEqual(findInvite(`come join https://sonar.example/?a=1&invite=${token.toUpperCase()}`), { token, only: false });
  assert.equal(findInvite("just a message"), null);
  assert.equal(findInvite(`https://sonar.example/?invite=${token}ff`), null);
  assert.equal(findInvite(`https://sonar.example/?invite=${token.slice(2)}`), null);
  assert.equal(findInvite(`https://sonar.example/?notinvite=${token}`), null);
  assert.equal(findInvite(undefined), null);
});
