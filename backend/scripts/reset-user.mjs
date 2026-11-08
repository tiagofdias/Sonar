import dotenv from "dotenv";
dotenv.config({ path: "backend/.env" });
import bcrypt from "bcryptjs";
import { connectDB } from "../src/lib/db.js";
import User from "../src/models/user.model.js";

const [,, action, email, arg3] = process.argv;

async function run() {
  await connectDB();

  if (!action || action === "list") {
    const users = await User.find({}, "email fullName encryptionKey createdAt");
    console.log("\n📋 Users in database:");
    for (const u of users) {
      console.log(`- Email: ${u.email}`);
      console.log(`  Name: ${u.fullName}`);
      console.log(`  Has paired encryption key: ${Boolean(u.encryptionKey)}`);
      console.log(`  Created: ${u.createdAt}`);
    }
    console.log("\nUsage:");
    console.log("  node backend/scripts/reset-user.mjs list");
    console.log("  node backend/scripts/reset-user.mjs password <email> <newPassword>");
    console.log("  node backend/scripts/reset-user.mjs clear-key <email>");
    console.log("  node backend/scripts/reset-user.mjs reset-all <email> <newPassword>\n");
    process.exit(0);
  }

  if (!email) {
    console.error("❌ Please provide an email address.");
    process.exit(1);
  }

  const user = await User.findOne({ email });
  if (!user) {
    console.error(`❌ User not found with email: ${email}`);
    process.exit(1);
  }

  if (action === "password" || action === "reset-all") {
    const newPassword = arg3 || "Password123!";
    if (newPassword.length < 10) {
      console.error("❌ Password must be at least 10 characters long.");
      process.exit(1);
    }
    const salt = await bcrypt.genSalt(10);
    user.password = await bcrypt.hash(newPassword, salt);
    user.sessionVersion = (user.sessionVersion || 0) + 1;
    if (action === "reset-all") {
      user.encryptionKey = null;
    }
    await user.save();
    console.log(`✅ Password successfully updated for ${email} to: ${newPassword}`);
    if (action === "reset-all") {
      console.log(`✅ Encryption pairing key cleared. You can now log in and pair from any new browser.`);
    }
  } else if (action === "clear-key") {
    user.encryptionKey = null;
    await user.save();
    console.log(`✅ Encryption pairing key cleared for ${email}. You can now log in and pair from this browser.`);
  } else {
    console.error(`❌ Unknown action: ${action}`);
    process.exit(1);
  }

  process.exit(0);
}

run().catch(err => {
  console.error("❌ Error:", err);
  process.exit(1);
});
