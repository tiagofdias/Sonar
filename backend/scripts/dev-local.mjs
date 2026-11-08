import { MongoMemoryReplSet } from "mongodb-memory-server";
import { mkdir, access, writeFile } from "node:fs/promises";
import { randomBytes } from "node:crypto";
import { fileURLToPath } from "node:url";
import { resolve, dirname } from "node:path";
import { spawn } from "node:child_process";
import dotenv from "dotenv";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const envPath = resolve(root, ".env");
try {
  await access(envPath);
} catch {
  await writeFile(
    envPath,
    `NODE_ENV=development\nPORT=5000\nJWT_SECRET=${randomBytes(48).toString("hex")}\nFRONTEND_URL=http://127.0.0.1:5173\n`,
    { flag: "wx" },
  );
}
dotenv.config({ path: envPath });
let database;
if (!process.env.MONGODB_URI) {
  const dbPath = resolve(root, ".local", "mongodb");
  await mkdir(dbPath, { recursive: true });
  console.log("Starting local MongoDB (first run may download the server)...");
  database = await MongoMemoryReplSet.create({
    instanceOpts: [{ dbPath }],
    replSet: {
      name: "sonar-local",
      count: 1,
      storageEngine: "wiredTiger",
      ip: "127.0.0.1",
    },
  });
  process.env.MONGODB_URI = database.getUri("sonar");
}
const child = spawn(process.execPath, ["src/index.js"], {
  cwd: root,
  env: process.env,
  stdio: "inherit",
  windowsHide: true,
});
let stopping = false;
async function stop(code = 0) {
  if (stopping) return;
  stopping = true;
  child.kill();
  await database?.stop({ doCleanup: false });
  process.exit(code);
}
child.on("exit", (code) => stop(code ?? 0));
process.on("SIGINT", () => stop());
process.on("SIGTERM", () => stop());
