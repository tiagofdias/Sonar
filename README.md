# Sonar

A free-to-use communication app with a web client and a native Windows desktop client that look and work the same: end-to-end encrypted direct messages and community channels, authenticated realtime delivery, seven-day history with pinning, and WebRTC voice calls and rooms without recording.

Before accepting requests, the backend creates and checks the message expiry indexes for direct messages and community channels, and also for experimental Matrix messages, encrypted device queues and transaction receipts when enabled. The MongoDB account needs permission to create and inspect indexes. A missing permission or conflicting index prevents startup; resolve the index configuration deliberately rather than dropping indexes automatically. MongoDB TTL cleanup is asynchronous, while application history excludes expired messages immediately. Pinned messages have no expiry until unpinned; server backups and copies held by participants require separate retention controls.

Direct messages and channels load the latest 50 live messages first. Use **Load older messages** to retrieve earlier pages without losing your scroll position. The pin button loads a separate paginated pinned-message view, including pins outside the recent conversation page. Unsent drafts stay with their conversation while switching chats and channels. Drafts are memory-only (up to 100 conversations), clear on signout/reload, and are removed for a community when access is revoked.

If a direct contact's encryption key changes, sending is blocked. Open **Conversation details**, compare the displayed safety code with that contact in person or on a call, and confirm only if it matches. A safety code is a string of letters and numbers; no fingerprint reader is needed. Accepting a new key affects this device only; messages encrypted for the previous key may become unreadable. First-use key discovery remains unauthenticated, so independently compare fingerprints for new contacts as well.

Private communities now include invite links, owner-created text channels, member lists, and channel messages encrypted separately for each member. Click the **Communities** icon in the left rail to create or join a space. Invites expire after 24 hours and a new invite revokes the previous link. New members receive future messages only. Early limits: 32 members and 30 channels per community.

The experimental Matrix device API is disabled by default. When explicitly enabled for isolated development with `SONAR_MATRIX_EXPERIMENTAL=true`, it also requires a stable `MATRIX_SERVER_NAME` chosen by the operator. It registers pending devices, binds tracked sessions, provides authorized room snapshots and relays encrypted verification events between devices on the same account. With the frontend built using `VITE_SONAR_MATRIX_EXPERIMENTAL=true`, Privacy & device offers an explicit experimental first-device action proving possession of the existing legacy key without exporting or replacing it. An authorized device may grant server access to a pending device, but the server cannot verify that SAS codes were compared; the clients must enforce verification. This does not migrate legacy history or provide recovery. Its request limiter is process-local, so do not use this experimental API for external or multi-instance testing. Changing the configured name changes Matrix user identities.

The same experimental flag exposes session-bound encrypted room transport: direct-room provisioning for two existing accounts, scoped key lookup and sharing, transaction-based event submission, history, pins, and deletion. With the frontend experimental flag, conversation privacy details also offers a separate direct-message test view that refreshes after a content-free Socket.IO notification or reconnect. It requires prepared and verified devices and does not migrate the existing chat. Notifications are process-local and manual refresh remains available. Community room provisioning, full message features, recovery and production client messaging remain incomplete; these endpoints must not be treated as a complete messaging system. Their limiter is also process-local.

With both experimental flags enabled, a direct conversation's privacy details also exposes a manual contact-device SAS comparison. Both participants must prepare their browser devices and compare all displayed numbers themselves. This experimental verification is separate from the existing legacy-key fingerprint and does not migrate or enable the current chat's messages.

Owners can remove members or transfer ownership from the member list's **Manage** menu. Non-owners can **Leave community** from the sidebar. Owners must transfer ownership before leaving. Removing a member or transferring ownership revokes the current invite. Removed members lose server access and are excluded from future message envelopes; copies they already received cannot be erased.

Click your avatar at the bottom of the left rail for **Account settings**. Edit your display name, change your password, inspect/copy your encryption fingerprint, review individual sign-in sessions, or sign out everywhere. Password changes require the current password, use at least 10 characters (maximum 72 UTF-8 bytes), and invalidate every existing session. New accounts use the same password limits. Existing shorter passwords can still log in. Changing your password does not affect your message key or history.

In **Password & passkeys**, choose **Add a passkey**, name it, and confirm your current password. Complete the device prompt, then use **Sign in with a passkey** on the login page. Up to ten passkeys are supported. Removing one requires your password and signs out all sessions. A passkey authenticates your account. On a device you have not used before, Sonar asks for your recovery key once to unlock your message key.

WebAuthn requires a hostname: HTTPS in production or localhost in development. When using the existing `127.0.0.1` preview, Sonar opens a dedicated localhost ceremony window and returns the signed response to the original window; your chat origin and encryption keys stay unchanged. Allow that popup and press Continue there. Production uses the same-origin device prompt. Set `WEBAUTHN_ORIGIN` to the exact public app origin when it differs from `FRONTEND_URL`. Local passkeys are scoped to localhost and will not work on a later production domain.

This is an early foundation, not a complete or audited Discord replacement. Channel roles, attachments, account recovery without the password, and broader moderation remain unfinished. No billing, ads, analytics, externally loaded fonts, or default external ICE provider is used (the Poppins font ships inside both apps under the SIL Open Font License). Hosting and TURN bandwidth still cost the operator money.

## Run locally

Use Node.js 22.12+ and MongoDB Atlas or a MongoDB replica set. Channel sends use transactions to serialize against membership changes; a standalone MongoDB instance is insufficient. The optional `npm run dev:local --prefix backend` starts a persistent local single-node replica set when no MONGODB_URI is configured.

1. Run `npm ci --prefix backend` and `npm ci --prefix frontend`.
2. Copy `backend/.env.example` to `backend/.env`. Set MongoDB and a random JWT secret of at least 32 characters.
3. In one terminal: `npm run dev --prefix backend`.
4. In another: `npm run dev --prefix frontend`.
5. Open http://localhost:5173. Vite proxies API and sockets to port 5000. Test two accounts in different browser profiles.

### Using more than one device

When you create an account, Sonar shows a **recovery key** once. Save it: your message key is locked with it on your device, and the server only stores the locked copy. On a new device (web, desktop or another browser) you sign in and enter the recovery key once. If you forget your password, choose **Forgot your password?** on the sign-in page and use the recovery key; no email is needed, and you get a new recovery key afterwards. If you lose the recovery key while still signed in somewhere, create a new one in **Account settings → Privacy & device**. Older accounts are moved to a recovery key the next time they sign in with a password. See [ARCHITECTURE.md](ARCHITECTURE.md#recovery-key-and-multi-device-backup) for the format and its limits.

For production use HTTPS, configure the exact FRONTEND_URL, and set NODE_ENV=production. Behind a TLS-terminating proxy (Render, Fly.io, etc.) requests from the app's own `https://` address are accepted even if FRONTEND_URL is missing; previously every login and signup was rejected with "Origin not allowed". Build with `npm run build --prefix frontend`, then `npm start --prefix backend`. The backend serves the build. Configure operator-controlled STUN/TURN in backend/.env for internet calls. Without ICE services, only compatible direct networks work.

## Voice calls

Open a direct conversation with an online member and press the phone button. The recipient can accept or decline. Calls have mute, deafen, a duration indicator, busy handling, and disconnect cleanup. No audio is recorded or stored. **Account settings → Voice & audio** has a local microphone test, input-device selection, echo/noise controls, and relay-only mode. Microphone settings stay in memory and apply to the next call.

Community owners can create a voice room with the **+** beside **VOICE ROOMS**, or choose **Voice room** in the channel creation dialog. Click a room to join. Rooms support up to six people in a peer-to-peer mesh with live participant/mute status. The room banner stays available while navigating between communities and direct messages. Use its mute/deafen controls or **Leave voice room** button. An account can join one room or direct call at a time. Community removal and socket disconnect remove room access and close peer connections. Joining shares your microphone audio with everyone in that room, including people who join later; rooms have no individual ringing/accept step.

Three-browser room audio has been verified locally with simulated microphones and audio received between every pair, along with navigation, mute/deafen, removal, and leave cleanup. Six-person capacity is enforced by signaling tests; physical-device and six-person cross-network performance remain unverified. Large-room/SFU voice is not implemented. Room state is transient and single-process; deployment across multiple backend instances needs shared coordination.

The **Your speakers** card selects call output, adjusts output volume, and plays a locally generated test chime without microphone access. Output settings apply to active calls; microphone settings apply to subsequent calls. Device selection uses browser audio-output permissions where supported. Use **Choose speakers** when available, or **Refresh devices** after granting microphone permission to expose permitted outputs. Browsers without selection support use the system default. A failed explicit output selection is not silently replaced with another speaker. The test stops and releases its media element/blob on completion, stop, device change, or settings close. Audio preferences remain memory-only.

Local two-browser calls have been verified with simulated microphones, inbound audio packets, mute/deafen, and capture cleanup. This does not verify physical audio devices or arbitrary internet networks. For internet connectivity, run an operator-controlled TURN service (for example coturn with `use-auth-secret`), set comma-separated `TURN_URLS`, and set the same `TURN_SHARED_SECRET` in the backend. Optional `STUN_URLS` can point to operator-controlled STUN. `/api/voice/config` issues anonymous one-hour TURN credentials and active calls refresh their ICE configuration before expiry; the shared secret stays server-side. The old VITE_ICE_SERVERS setting is no longer used. Set `VOICE_RELAY_ONLY=true` to require relay connections for all calls, or enable it per browser in Voice settings when TURN is available. No relay service is bundled or provisioned by this update.

Incoming calls ring with an original tone of low water drops and slow waves, `frontend/public/sounds/ringtone.wav`, on the speakers chosen in Voice settings. Web and desktop play that same file; `node desktop/scripts/generate-ringtone.mjs` recreates it. Rebuild the desktop app after changing it.

A call that rings and is never answered, including a declined one, appears in the conversation for both people. The caller's app sends it as an ordinary encrypted message.

In a call or voice room you can share a screen or a single app at 1080p, 1440p or 4K. The desktop app shares that app's sound only, and encodes on the graphics card when it can.

## Checks

- `npm test --prefix frontend`: keys, encryption round trips, tampering, IVs, and contact key changes.
- `npm test --prefix backend`: direct-message unit checks plus community integration tests against an isolated temporary local MongoDB. These tests do not connect to Atlas. First run may download the MongoDB test binary.
- `npm run build --prefix frontend`: production build.
- `npm audit --prefix frontend` and `npm audit --prefix backend`: advisories.
- `npm run test:browser --prefix backend`: isolated encrypted-history pagination, drafts, direct voice, speaker/microphone controls, three-person community room audio and removal, passkey enrollment/login, mobile layout, and password changes. Uses Chromium virtual authenticators and simulated microphones. Requires a Playwright Chromium install, or set `SONAR_BROWSER_EXECUTABLE` to an installed Chromium/Edge executable. Uses a separate local replica set, never Atlas. Screenshots are saved in ignored `backend/.local/`.

See [ARCHITECTURE.md](ARCHITECTURE.md) for privacy boundaries and remaining production work. Legacy plaintext is not automatically migrated or destroyed.

### Relay setup and verification

An operator-owned coturn starting configuration is available in
[`deploy/coturn`](deploy/coturn/README.md). It is a template and does not provision a relay.

1. Provision an operator-controlled coturn server with a public address. Configure `use-auth-secret`, a strong unique `static-auth-secret`, and a realm. If the server is behind NAT, configure its public/private address mapping. Follow the [coturn configuration reference](https://github.com/coturn/coturn/blob/master/examples/etc/turnserver.conf) for your network, TLS certificates, quotas, and access restrictions.
2. Allow the configured TURN listener ports and UDP relay port range through both the host firewall and hosting-provider firewall. Restrict relay access to prevent use as an open relay, and block access to sensitive private networks. TURN may incur bandwidth and hosting costs.
3. Set `TURN_URLS` to the server's reachable TURN URL(s) and `TURN_SHARED_SECRET` to the matching secret in the backend environment. Keep the secret private and restart the backend. Optionally set `VOICE_RELAY_ONLY=true` after the relay is working.
4. In Sonar, open **Voice & audio ? Call connectivity ? Test relay connection**. This requests a relay allocation without using the microphone. A successful result confirms only that this browser obtained a relay candidate; test both participants on separate networks, then verify actual audio and a call lasting over an hour.

No relay is provisioned by these instructions. Existing voice connections do not automatically restart ICE after network changes; refreshing the ICE configuration alone does not guarantee an existing allocation survives credential expiry.

### Account request limits

Login and signup have per-IP limits (30 and 10 attempts respectively in 15 minutes, counting successful attempts). Passkey login challenges and verification each have separate limits, and password confirmation is limited per account. By default these limits are process-local and reset on restart. Set `MONGO_SHARED_AUTH_LIMITS=true` on every backend instance to use atomic MongoDB counters with an expiry index; instances must use the same database and `JWT_SECRET`, and have synchronized clocks. Database/index failures return 503 rather than falling back to weaker local limits. A broad passkey request limit remains process-local. MongoDB retains hashed limiter keys until its asynchronous TTL cleanup runs; distributed abuse can still consume database capacity.

Express ignores forwarded client IPs by default, so users behind a proxy may share one limit. On hosts that do not publish their proxy addresses (Render, Fly, Heroku), set `TRUST_PROXY_HOPS` to the number of proxies in front of the app (usually `1`) instead; never set it higher than the real number, or clients could forge their address. Sign-in and recovery are also limited per account (20 and 10 attempts in 15 minutes), signed-in actions such as sending messages, username lookups and creating communities have per-account limits, and every response carries a strict Content-Security-Policy, clickjacking protection and, in production, HSTS (`backend/src/middleware/securityHeaders.js`). Who is online is only sent to a person's friends and community co-members.

**Watch Together** (Apps button in a call or voice room) lets everyone in the call watch YouTube in sync: anyone can pause, seek or skip for everyone, the queue can be added to, reordered and trimmed, and reactions float over the video. If someone's video is held by an ad or buffering, everyone waits; after 30 seconds the group can continue without them. Sponsor segments are skipped for everyone using SponsorBlock data (looked up by hash prefix, CC BY-NC-SA 4.0). Videos play from YouTube's official player (youtube-nocookie.com) in each browser, driven by messages so YouTube's script never runs in Sonar's page; the server keeps only the queue and position in memory (`backend/src/lib/watch.js`). The desktop app does not embed a browser: its Watch together button opens the person's own browser with a one-time, two-minute link that grants a watch-only connection for that call. Set `YOUTUBE_API_KEY` to allow importing playlists. If applicable, set `TRUST_PROXY_CIDRS` to a comma-separated list of specific proxy addresses or CIDRs controlled by the operator, and ensure those proxies overwrite forwarded headers. An incorrect trust configuration can let clients spoof addresses. Leave it unset for direct connections. Fixed windows can permit bursts at their boundaries; these limits are one layer of abuse protection, not a complete defense.

### Usernames

Every account has a unique `@username` (3 to 24 letters, numbers, dots or underscores). People choose one at sign-up and can change it in Your profile; the People panel finds members by username or name. Accounts created before usernames existed get one automatically when the server starts.

### Community management

Owners can rename or delete communities, text channels, and voice rooms. Deleting a channel or community removes its stored messages (including pinned messages), disconnects affected voice rooms, and cannot be undone in Sonar. A community must retain at least one text channel. Existing backups and copies saved by participants are not erased.

### Zero-charge deployment goal

Sonar must not require a paid service for its owner or users. An operator can run the open-source application, MongoDB, and coturn on hardware they already control, subject to electricity, internet, capacity, and public network access. An eligible free hosting tier is another option, but its quotas and availability are controlled by the provider and can change. Do not configure automatic paid upgrades or paid resources; set hard limits where available and let the service stop accepting new usage when free capacity is exhausted. No hosting, relay, or paid service has been provisioned by this project.

### Unread indicators and notifications

Unread dots for direct messages and community channels are session-only and track new events received while Sonar is connected. They are not read receipts and do not include messages missed while offline or after a reload. In Privacy & device, an optional browser notification can show a generic direct-message alert while the page is in the background. It includes no sender or message text, works only while Sonar is open, and resets on reload or sign-out.

### Replies

Replies in direct messages and text channels store the original message ID inside the encrypted reply, not a copy of its text. If the original message is not in the currently loaded visible history, has expired, or was deleted, the reference shows as unavailable. Replies expire independently after seven days unless pinned. Older clients that do not understand replies may display the encoded payload as text.

### Sign-in sessions

Under **Password & passkeys**, the session list shows the sign-in method and dates for up to 100 active tracked sessions. You can sign out one session or use **Sign out everywhere**. Session records contain no device name or IP address and expire after seven days. Older cookies issued before session tracking remain valid until expiry or an account-wide sign-out; they cannot be individually listed or revoked. Signing out does not remove browser encryption keys.

Individual session revocation is enforced against the shared database for HTTP requests and socket authentication/events. Immediate disconnection of an already connected socket is currently local to the backend process; multi-instance production requires a shared Socket.IO adapter or equivalent revocation propagation before relying on immediate disconnect across instances. Idle sockets also revalidate their session approximately every 15 seconds and disconnect on failed or stalled checks, but may receive events before the next check completes.

### Reactions

In normal direct-message and channel history, use the smile button to add or remove a reaction. Reactions are encrypted events; the emoji and target message ID are inside the ciphertext, though the service still sees timing and other message metadata. Each event expires after seven days independently of the original message, even when the original is pinned. Reactions are calculated from loaded history, so loading older pages can reveal earlier reactions. They are unavailable in the pinned-only view. Older clients may show the encoded event as a message.

### Moderators

A community owner can grant or remove the moderator role from the member list. Moderators can remove or ban ordinary members; they cannot remove or ban the owner or other moderators, manage roles, create invites, manage channels, or transfer ownership. Removing or banning a member revokes the current invite and disconnects that member from community voice rooms. Removed members may rejoin with a later valid invite; banned accounts cannot. Only the owner can view and remove bans. Unbanning also revokes the current invite, and the account remains outside until it joins with a fresh invite. A community can have up to 500 banned account IDs. Bans apply to accounts and cannot prevent someone from creating another account. Roles and membership are visible to community members and the service; the owner-only ban list remains visible to the service.

### Native desktop app

`desktop/` is a native Rust client (Iced with software rendering; no browser engine, WebView, Tauri or Electron). It has the same screens and features as the web client (direct messages, communities, voice calls and voice rooms, account settings, passkeys) and opens the same conversations with the same account. It uses about 30-40 MiB of memory (measured on one machine).

On Windows x64, install the Rust stable MSVC toolchain and the Visual Studio C++ build tools, then:

- `cargo run --release --manifest-path desktop/Cargo.toml` runs the app. Choose **Server** on the sign-in screen to point it at your Sonar address (`https://...`; plain `http://` only for a server on the same computer).
- `powershell -NoProfile -File desktop/scripts/package-windows.ps1` writes a portable ZIP and SHA-256 checksum under `desktop/dist`. Add `-InnoCompiler <path-to-ISCC.exe>` to also build an unsigned per-user installer.
- `cargo test --manifest-path desktop/Cargo.toml` runs the native tests, including the Web Crypto and key-backup compatibility fixtures.
- `node desktop/scripts/generate-icons.mjs` refreshes the icon set from the web client's Lucide package.

The key and session are stored in `%LOCALAPPDATA%\Sonar`, protected with Windows DPAPI for the current user. Development builds accept `SONAR_DEV_EMAIL`, `SONAR_DEV_PASSWORD`, `SONAR_DEV_SERVER`, `SONAR_DEV_SCRIPT`, `SONAR_DEV_OFFSCREEN` and `SONAR_DEV_SILENT_AUDIO` for automated UI checks; release builds ignore them. Windows Hello passkeys and a physical-microphone call to a browser have been verified on one Windows 11 machine. The installer is unsigned.

### Voice room moderation

Owners and moderators can lock or unlock a voice room and disconnect participants from it. A locked room blocks new joins; people already inside remain connected. Disconnecting someone does not ban them, and they can rejoin unless the room is locked. Moderators cannot disconnect the owner or other moderators. Room locks persist in the community database; room occupancy remains process-local.

## Publishing a desktop update

Bump `version` in `desktop/Cargo.toml`, then run `node desktop/scripts/release.mjs`. It builds the Windows download and signs it with the key in `~/.sonar-release`, which must stay private and backed up. Commit the new zip and `latest.json`; installed copies pick the update up from the site. Push alerts generate their keys automatically unless `VAPID_PUBLIC_KEY` and `VAPID_PRIVATE_KEY` are set.
