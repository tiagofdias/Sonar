import { test } from "node:test";
import assert from "node:assert/strict";
import {
  createHash,
  generateKeyPairSync,
  randomBytes,
  sign,
} from "node:crypto";
import express from "express";
import cookieParser from "cookie-parser";
import mongoose from "mongoose";
import bcrypt from "bcryptjs";
import jwt from "jsonwebtoken";
import { MongoMemoryReplSet } from "mongodb-memory-server";
import { isoCBOR } from "@simplewebauthn/server/helpers";
import passkeyRoutes from "./passkey.route.js";
import User from "../models/user.model.js";
import Passkey from "../models/passkey.model.js";
import Challenge from "../models/passkeyChallenge.model.js";

const hash = (data) => createHash("sha256").update(data).digest();
const encoded = (data) => Buffer.from(data).toString("base64url");
const origin = "http://localhost:5173";
test("passkey ceremonies verify real signed authenticator data", async (t) => {
  process.env.WEBAUTHN_ORIGIN = origin;
  const database = await MongoMemoryReplSet.create({ replSet: { count: 1 } });
  let server;
  try {
    await mongoose.connect(database.getUri("sonar-passkey-tests"));
    const password = "TenChars!!";
    const user = await User.create({
      fullName: "Passkey tester",
      email: "passkey@example.invalid",
      password: await bcrypt.hash(password, 4),
      encryptionKey: "keep-this-encryption-key",
    });
    const other = await User.create({
      fullName: "Other user",
      email: "other@example.invalid",
      password: await bcrypt.hash(password, 4),
    });
    const cookieFor = (account) =>
      `jwt=${jwt.sign({ userId: String(account._id) }, process.env.JWT_SECRET)}`;
    const app = express();
    app.use(express.json());
    app.use(cookieParser());
    app.use("/api/auth/passkeys", passkeyRoutes);
    server = await new Promise((resolve) => {
      const instance = app.listen(0, "127.0.0.1", () => resolve(instance));
    });
    const url = `http://127.0.0.1:${server.address().port}/api/auth/passkeys`;
    async function request(path = "", method = "GET", body, cookies = "") {
      const result = await fetch(url + path, {
        method,
        headers: { "Content-Type": "application/json", Cookie: cookies },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      return {
        status: result.status,
        body: await result.json(),
        cookies: result.headers
          .getSetCookie()
          .map((cookie) => cookie.split(";")[0]),
      };
    }
    const pair = generateKeyPairSync("ec", { namedCurve: "prime256v1" });
    const jwk = pair.publicKey.export({ format: "jwk" });
    const credentialId = randomBytes(32);
    const cose = isoCBOR.encode(
      new Map([
        [1, 2],
        [3, -7],
        [-1, 1],
        [-2, Buffer.from(jwk.x, "base64url")],
        [-3, Buffer.from(jwk.y, "base64url")],
      ]),
    );
    function registration(options, overrides = {}) {
      const idLength = Buffer.alloc(2);
      idLength.writeUInt16BE(credentialId.length);
      const authData = Buffer.concat([
        hash("localhost"),
        Buffer.from([overrides.flags ?? 0x45]),
        Buffer.alloc(4),
        Buffer.alloc(16),
        idLength,
        credentialId,
        cose,
      ]);
      const attestation = isoCBOR.encode(
        new Map([
          ["fmt", "none"],
          ["attStmt", new Map()],
          ["authData", authData],
        ]),
      );
      return {
        id: encoded(credentialId),
        rawId: encoded(credentialId),
        type: "public-key",
        response: {
          attestationObject: encoded(attestation),
          clientDataJSON: encoded(
            JSON.stringify({
              type: "webauthn.create",
              challenge: options.challenge,
              origin: overrides.origin || origin,
              crossOrigin: false,
            }),
          ),
          transports: ["internal"],
        },
        clientExtensionResults: { credProps: { rk: true } },
      };
    }
    function assertion(options, counter = 1, overrides = {}) {
      const count = Buffer.alloc(4);
      count.writeUInt32BE(counter);
      const authData = Buffer.concat([
        hash(overrides.rpID || "localhost"),
        Buffer.from([overrides.flags ?? 0x05]),
        count,
      ]);
      const clientData = Buffer.from(
        JSON.stringify({
          type: "webauthn.get",
          challenge: options.challenge,
          origin: overrides.origin || origin,
          crossOrigin: false,
        }),
      );
      const signature = sign(
        "sha256",
        Buffer.concat([authData, hash(clientData)]),
        overrides.privateKey || pair.privateKey,
      );
      return {
        id: encoded(credentialId),
        rawId: encoded(credentialId),
        type: "public-key",
        response: {
          clientDataJSON: encoded(clientData),
          authenticatorData: encoded(authData),
          signature: encoded(signature),
          userHandle: encoded(String(user._id)),
        },
        clientExtensionResults: {},
      };
    }
    let keyId;
    await t.test(
      "enrollment requires login, password confirmation, and user verification",
      async () => {
        assert.equal((await request()).status, 401);
        assert.equal(
          (
            await request(
              "/register/options",
              "POST",
              { name: "Laptop", currentPassword: "wrong" },
              cookieFor(user),
            )
          ).status,
          400,
        );
        const setup = await request(
          "/register/options",
          "POST",
          { name: "Laptop", currentPassword: password },
          cookieFor(user),
        );
        assert.equal(setup.status, 200);
        assert.equal(
          setup.body.options.authenticatorSelection.userVerification,
          "required",
        );
        const failed = await request(
          "/register/verify",
          "POST",
          { response: registration(setup.body.options, { flags: 0x41 }) },
          [cookieFor(user), ...setup.cookies].join("; "),
        );
        assert.equal(failed.status, 400);
        const retry = await request(
          "/register/options",
          "POST",
          { name: "Laptop", currentPassword: password },
          cookieFor(user),
        );
        const created = await request(
          "/register/verify",
          "POST",
          { response: registration(retry.body.options) },
          [cookieFor(user), ...retry.cookies].join("; "),
        );
        assert.equal(created.status, 201);
        keyId = created.body._id;
        assert.equal(created.body.publicKey, undefined);
        assert.equal(
          (
            await request(
              "/register/verify",
              "POST",
              { response: registration(retry.body.options) },
              [cookieFor(user), ...retry.cookies].join("; "),
            )
          ).status,
          400,
        );
        assert.equal(
          (await request("", "GET", undefined, cookieFor(other))).body.length,
          0,
        );
      },
    );
    await t.test(
      "discoverable passkey login verifies signature and issues a session",
      async () => {
        const setup = await request("/login/options", "POST", {});
        const response = assertion(setup.body.options);
        assert.equal(
          (await request("/login/verify", "POST", { response })).status,
          400,
        );
        const result = await request(
          "/login/verify",
          "POST",
          { response },
          setup.cookies.join("; "),
        );
        assert.equal(result.status, 200);
        assert.equal(result.body.email, user.email);
        assert.equal(result.body.encryptionKey, "keep-this-encryption-key");
        assert.ok(result.cookies.some((cookie) => cookie.startsWith("jwt=")));
        assert.equal((await Passkey.findById(keyId)).counter, 1);
        assert.equal(
          (
            await request(
              "/login/verify",
              "POST",
              { response },
              setup.cookies.join("; "),
            )
          ).status,
          400,
        );
      },
    );
    await t.test(
      "rejects bad origin, relying party, signature, missing verification and stale counters",
      async () => {
        const wrongKey = generateKeyPairSync("ec", {
          namedCurve: "prime256v1",
        }).privateKey;
        for (const overrides of [
          { origin: "https://evil.example" },
          { rpID: "evil.example" },
          { privateKey: wrongKey },
          { flags: 0x01 },
        ]) {
          const setup = await request("/login/options", "POST", {});
          assert.equal(
            (
              await request(
                "/login/verify",
                "POST",
                { response: assertion(setup.body.options, 2, overrides) },
                setup.cookies.join("; "),
              )
            ).status,
            400,
          );
        }
        const stale = await request("/login/options", "POST", {});
        assert.equal(
          (
            await request(
              "/login/verify",
              "POST",
              { response: assertion(stale.body.options, 1) },
              stale.cookies.join("; "),
            )
          ).status,
          400,
        );
        const expired = await request("/login/options", "POST", {});
        await Challenge.updateMany(
          { purpose: "authentication" },
          { expiresAt: new Date(0) },
        );
        assert.equal(
          (
            await request(
              "/login/verify",
              "POST",
              { response: assertion(expired.body.options, 2) },
              expired.cookies.join("; "),
            )
          ).status,
          400,
        );
      },
    );
    await t.test(
      "passkey removal is account scoped and revokes previous sessions",
      async () => {
        assert.equal(
          (
            await request(
              `/${keyId}`,
              "DELETE",
              { currentPassword: password },
              cookieFor(other),
            )
          ).status,
          404,
        );
        assert.equal(
          (
            await request(
              `/${keyId}`,
              "DELETE",
              { currentPassword: "wrong" },
              cookieFor(user),
            )
          ).status,
          400,
        );
        assert.equal(
          (
            await request(
              `/${keyId}`,
              "DELETE",
              { currentPassword: password },
              cookieFor(user),
            )
          ).status,
          200,
        );
        assert.equal(await Passkey.countDocuments({ userId: user._id }), 0);
        assert.equal(
          (await request("", "GET", undefined, cookieFor(user))).status,
          401,
        );
        assert.equal(
          (await User.findById(user._id)).encryptionKey,
          "keep-this-encryption-key",
        );
        const setup = await request("/login/options", "POST", {});
        assert.equal(
          (
            await request(
              "/login/verify",
              "POST",
              { response: assertion(setup.body.options, 2) },
              setup.cookies.join("; "),
            )
          ).status,
          400,
        );
      },
    );
    await t.test("passkey authentication options and verification have separate limits", async () => {
      let options, verify;
      for (let i = 0; i < 31; i++) options = await request("/login/options", "POST", {});
      assert.equal(options.status, 429);
      for (let i = 0; i < 31; i++) verify = await request("/login/verify", "POST", {});
      assert.equal(verify.status, 429);
    });
  } finally {
    if (server) await new Promise((resolve) => server.close(resolve));
    await mongoose.disconnect();
    await database.stop();
  }
});
