// Used only by the isolated browser/native integration test.
import * as sdk from "@matrix-org/matrix-sdk-crypto-wasm";
import { withBrowserMatrixIdentity } from "./matrixIdentityVault.js";
import { publishMatrixVerificationKeys } from "./matrixVerificationKeys.js";
export async function publishForTest(user, api) {
  await withBrowserMatrixIdentity(user, (machine, descriptor) => publishMatrixVerificationKeys(sdk, machine, descriptor, api));
}
