// Builds the Windows download and signs it so installed copies accept the update.
// Run after bumping `version` in desktop/Cargo.toml: node desktop/scripts/release.mjs
// The signing key stays in ~/.sonar-release. Keep a backup: without it, installed
// copies stop accepting updates and everyone must download by hand once.
// --key-only creates the key and public key file without building.
import { createHash, createPrivateKey, createPublicKey, generateKeyPairSync, sign } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const desktop = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const downloads = resolve(desktop, "../frontend/public/downloads");
const keyFile = process.env.SONAR_RELEASE_KEY || join(homedir(), ".sonar-release", "update-signing-key.pem");

if (!existsSync(keyFile)) {
  mkdirSync(dirname(keyFile), { recursive: true });
  const { privateKey } = generateKeyPairSync("ec", { namedCurve: "P-256" });
  writeFileSync(keyFile, privateKey.export({ type: "pkcs8", format: "pem" }), { mode: 0o600 });
  console.log(`Created a new signing key at ${keyFile}. Back it up somewhere safe.`);
}
const privateKey = createPrivateKey(readFileSync(keyFile));
const publicDer = createPublicKey(privateKey).export({ type: "spki", format: "der" });
writeFileSync(join(desktop, "assets/update-key.pub"), publicDer.toString("base64") + "\n");
if (process.argv.includes("--key-only")) process.exit(0);

const version = /^version = "(.+)"/m.exec(readFileSync(join(desktop, "Cargo.toml"), "utf8"))[1];
const exe = join(desktop, "target/release/sonar-desktop.exe");
const parked = `${exe}.${process.pid}.locked`;
if (existsSync(exe)) renameSync(exe, parked); // A running copy would block the build.
execFileSync("cargo", ["build", "--release"], { cwd: desktop, stdio: "inherit" });
rmSync(parked, { force: true });

const zip = join(downloads, "Sonar-Desktop-windows-x64.zip");
execFileSync("powershell", ["-NoProfile", "-Command", `Compress-Archive -Force -Path '${exe}','${join(desktop, "WINDOWS-PREVIEW.txt")}' -DestinationPath '${zip}'`], { stdio: "inherit" });
const sha256 = createHash("sha256").update(readFileSync(zip)).digest("hex");
const signature = sign("sha256", Buffer.from(`sonar-update\n${version}\n${sha256}`), { key: privateKey, dsaEncoding: "ieee-p1363" }).toString("base64");
writeFileSync(join(downloads, "latest.json"), JSON.stringify({ version, sha256, signature }, null, 2) + "\n");
console.log(`Sonar ${version} ready: ${sha256}`);
