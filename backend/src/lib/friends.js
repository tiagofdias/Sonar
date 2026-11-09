import Friendship from "../models/friendship.model.js";
import Message from "../models/message.model.js";

export const DISAPPEAR_OPTIONS = [0, 3600, 86400];
export const disappearingExpiry = (seconds) => (seconds ? { expiresAt: new Date(Date.now() + seconds * 1000) } : {});
export const pairDisappearAfter = async (a, b) =>
  (await Friendship.findOne({ pair: pairKey(a, b), status: "accepted" }).select("disappearAfter").lean())?.disappearAfter || 0;

export const pairKey = (a, b) => [String(a), String(b)].sort().join(":");

export const areFriends = async (a, b) =>
  Boolean(await Friendship.exists({ pair: pairKey(a, b), status: "accepted" }));

export const befriend = (a, b) =>
  Friendship.create({ pair: pairKey(a, b), users: [a, b], status: "accepted" });

export async function friendIds(userId) {
  const rows = await Friendship.find({ users: userId, status: "accepted" }).select("users").lean();
  return rows.map((row) => row.users.find((id) => String(id) !== String(userId)));
}

// People who were already talking before friends existed stay connected.
// Safe to repeat: an existing pair is never changed.
export async function seedFriendsFromMessages() {
  const pairs = await Message.aggregate([
    { $match: { $expr: { $ne: ["$senderId", "$receiverId"] } } },
    { $group: { _id: { senderId: "$senderId", receiverId: "$receiverId" } } },
  ]);
  const operations = new Map();
  for (const { _id: { senderId, receiverId } } of pairs) {
    const pair = pairKey(senderId, receiverId);
    operations.set(pair, {
      updateOne: {
        filter: { pair },
        update: { $setOnInsert: { pair, users: [senderId, receiverId], status: "accepted" } },
        upsert: true,
      },
    });
  }
  if (operations.size) await Friendship.bulkWrite([...operations.values()], { ordered: false });
}
