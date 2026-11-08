import Message from "../models/message.model.js";
import ChannelMessage from "../models/channelMessage.model.js";
import MatrixRoomEvent from "../models/matrixRoomEvent.model.js";
import MatrixToDevice from "../models/matrixToDevice.model.js";
import MatrixToDeviceTransaction from "../models/matrixToDeviceTransaction.model.js";
import MatrixEventTransaction from "../models/matrixEventTransaction.model.js";

// Do not silently start if the database cannot enforce message retention.
// Never drop or alter an existing index: the operator must resolve conflicts.
export async function ensureMessageRetentionIndexes({ matrix = false } = {}) {
  for (const model of [Message, ChannelMessage, ...(matrix ? [MatrixRoomEvent, MatrixToDevice, MatrixToDeviceTransaction, MatrixEventTransaction] : [])]) {
    try {
      await model.collection.createIndex({ expiresAt: 1 }, { expireAfterSeconds: 0 });
      const indexes = await model.collection.indexes();
      const valid = indexes.some(index => Object.keys(index.key).length === 1 && index.key.expiresAt === 1 && index.expireAfterSeconds === 0 && !index.partialFilterExpression && !index.sparse);
      if (!valid) throw new Error("missing or restricted TTL index");
    } catch (error) {
      throw new Error(`Message retention index unavailable for ${model.collection.name}; check database permissions and index conflicts.`, { cause: error });
    }
  }
  if (matrix) {
    try { await MatrixRoomEvent.collection.createIndex({ attachmentId: 1 }, { unique: true, sparse: true }); }
    catch (error) { throw new Error("Attachment identity index unavailable; check database permissions and index conflicts.", { cause: error }); }
  }
}
