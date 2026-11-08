# Deploying Sonar for free

Sonar needs no paid service. This guide uses two free tiers: a Render free web service (runs the backend and serves the web app) and a MongoDB Atlas free M0 cluster (the database). Free-tier quotas and availability are set by those providers and can change. Never add a payment method or enable automatic upgrades; if a free limit is reached, the service stops instead of billing you.

You can also run everything on a computer you already own; see "Run locally" in [README.md](README.md).

## 1. Database: MongoDB Atlas M0 (free)

1. Create an account at <https://www.mongodb.com/cloud/atlas> and create a **free M0 cluster**. It is a replica set, which Sonar needs for channel transactions.
2. Under **Database Access**, create a database user with a strong generated password.
3. Under **Network Access**, allow the hosting provider's addresses (Render free services have no fixed IP, so `0.0.0.0/0` is the usual choice; the strong database password then matters).
4. Copy the connection string, e.g. `mongodb+srv://USER:PASSWORD@cluster0.xxxxx.mongodb.net/sonar?retryWrites=true&w=majority`.

The database user needs permission to create indexes: Sonar creates its message-expiry indexes at startup.

## 2. App: Render free web service

1. Push this repository to GitHub.
2. In Render choose **New → Blueprint** and select the repository. `render.yaml` creates one free web service.
3. Set the environment variables Render asks for:

| Variable | Value |
|----------|-------|
| `MONGODB_URI` | The Atlas connection string from step 1 |
| `JWT_SECRET` | A random secret of at least 32 characters: `node -e "console.log(require('crypto').randomBytes(48).toString('hex'))"` |
| `FRONTEND_URL` | The service's public address, e.g. `https://sonar-app.onrender.com` (used for CORS, Socket.IO and passkeys) |

4. Deploy, then open the service address and create an account.

Render's free web services sleep after a period of inactivity, so the first request after a pause can take up to a minute.

## 3. Desktop app

Build it with `powershell -NoProfile -File desktop/scripts/package-windows.ps1` (see README), run it, choose **Server** on the sign-in screen and enter the same `https://` address. Web and desktop share accounts and conversations.

## Voice calls across the internet (optional)

Calls between people on the same network work without extra services. To connect people behind strict NATs, run an operator-controlled TURN server (coturn) on hardware you control or a free VM. See [deploy/coturn](deploy/coturn/README.md) and set `TURN_URLS` and `TURN_SHARED_SECRET`. No relay is provisioned by default, and none is required for messaging.

## Troubleshooting

- **"Origin not allowed" on sign-in:** set `FRONTEND_URL` to the exact public address. Current versions also accept the service's own `https://` address behind Render's proxy.
- **"MongoDB connected" never appears:** check the Atlas network access list and the password in `MONGODB_URI`.
- **Startup stops at an index error:** the database user lacks index permissions; see the README's retention notes.
