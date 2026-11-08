import express from "express";
import { protectRoute } from "../middleware/auth.middleware.js";
import { getMessages, getUsersForSidebar, getUnreadCandidates, sendMessage, pinMessage, deleteMessage } from "../controllers/message.controller.js";

const router = express.Router();

router.get("/users", protectRoute, getUsersForSidebar);
router.get("/unread-candidates", protectRoute, getUnreadCandidates);
router.get("/:id", protectRoute, getMessages);

router.post("/send/:id", protectRoute, sendMessage);
router.patch("/:id/pin", protectRoute, pinMessage);
router.delete("/:id", protectRoute, deleteMessage);

export default router;
