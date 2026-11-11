import express from "express";
import { protectRoute } from "../middleware/auth.middleware.js";
import { getMessages, getUsersForSidebar, getUnreadCandidates, sendMessage, pinMessage, editMessage, deleteMessage } from "../controllers/message.controller.js";

import { userRateLimit } from "../middleware/userRateLimit.js";

const router = express.Router();
// Generous for real conversations, but one account cannot flood another or the database.
const sendLimit = userRateLimit({ limit: 120, windowMs: 60000 });

router.get("/users", protectRoute, getUsersForSidebar);
router.get("/unread-candidates", protectRoute, getUnreadCandidates);
router.get("/:id", protectRoute, getMessages);

router.post("/send/:id", protectRoute, sendLimit, sendMessage);
router.patch("/:id/pin", protectRoute, pinMessage);
router.patch("/:id", protectRoute, sendLimit, editMessage);
router.delete("/:id", protectRoute, deleteMessage);

export default router;
