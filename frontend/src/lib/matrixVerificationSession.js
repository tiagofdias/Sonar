// An interactive verification owns the protected SDK store for its entire
// lifetime. Closing the callback releases it and cancels local flow handles;
// it does not authorize a device or replace the stored identity.
import * as sdk from "@matrix-org/matrix-sdk-crypto-wasm";
import { withBrowserMatrixIdentity } from "./matrixIdentityVault.js";
import { publishMatrixVerificationKeys, refreshMatrixVerificationDevices, establishMatrixVerificationSession } from "./matrixVerificationKeys.js";
import { incomingMatrixVerificationRequests, matrixVerificationFlow } from "./matrixVerificationFlow.js";
import { receiveMatrixVerification } from "./matrixVerificationTransport.js";

export async function withBrowserMatrixVerification(userId, api, callback) {
  if (typeof callback !== "function" || typeof api?.get !== "function" || typeof api?.post !== "function") throw new Error("Verification service unavailable.");
  if ((await api.get("/matrix/device/identity")).data?.userId !== userId) throw new Error("The server identity changed. Verification is blocked.");
  return withBrowserMatrixIdentity(userId, async (machine, descriptor) => {
    const devices = (await api.get("/matrix/device/list")).data?.devices;
    const current = Array.isArray(devices) && devices.length <= 100 ? devices.filter(device => device?.current === true) : [];
    if (current.length !== 1 || current[0].deviceId !== descriptor.deviceId || current[0].ed25519 !== descriptor.ed25519 || !["authorized", "pending"].includes(current[0].serverAuthorization)) throw new Error("The local device identity changed. Verification is blocked.");
    await publishMatrixVerificationKeys(sdk, machine, descriptor, api);
    await refreshMatrixVerificationDevices(sdk, machine, descriptor, api);
    const flows = new Set();
    let active = true;
    const requireActive = () => { if (!active) throw new Error("Verification session is closed."); };
    const session = {
      deviceId: descriptor.deviceId,
      serverAuthorization: current[0].serverAuthorization,
      async refresh() { requireActive(); await refreshMatrixVerificationDevices(sdk, machine, descriptor, api); },
      async receive() { requireActive(); await refreshMatrixVerificationDevices(sdk, machine, descriptor, api); await receiveMatrixVerification(sdk, machine, descriptor, api); return incomingMatrixVerificationRequests(sdk, machine, descriptor); },
      async createFlow(targetDeviceId) {
        requireActive();
        await establishMatrixVerificationSession(sdk, machine, descriptor, targetDeviceId, api);
        const flow = matrixVerificationFlow(sdk, machine, descriptor, targetDeviceId, api);
        flows.add(flow);
        return flow;
      },
    };
    try { return await callback(session); }
    finally { active = false; for (const flow of flows) flow.close(); }
  });
}
