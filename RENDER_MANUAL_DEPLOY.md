# Manual Render Deployment Guide

This guide walks you through deploying the Sonar Chat App on Render manually (without render.yaml).

## Prerequisites

1. A [Render](https://render.com) account
2. Your code pushed to a GitHub repository
3. A MongoDB database (MongoDB Atlas recommended)
4. Cloudinary account (optional, for image uploads)

---

## Step 1: Create a Web Service on Render

1. Go to [Render Dashboard](https://dashboard.render.com)
2. Click **"New +"** → **"Web Service"**
3. Connect your GitHub repository
4. Select your Sonar repository

---

## Step 2: Configure the Web Service

Fill in the following settings:

### Basic Settings
- **Name**: `sonar-chat-app` (or your preferred name)
- **Region**: Choose closest to your users
- **Branch**: `main` (or your default branch)
- **Root Directory**: Leave blank
- **Runtime**: `Node`

### Build & Deploy Settings
- **Build Command**: 
  ```bash
  npm install --prefix backend && npm install --prefix frontend && npm run build --prefix frontend && cp -r frontend/dist backend/dist
  ```
  
- **Start Command**:
  ```bash
  cd backend && npm start
  ```

### Instance Type
- **Free** (or choose a paid plan for better performance)

---

## Step 3: Set Environment Variables

Click on **"Environment"** tab and add these variables:

### Required Variables

| Key | Value | Notes |
|-----|-------|-------|
| `NODE_ENV` | `production` | Sets the app to production mode |
| `PORT` | `10000` | Render will auto-assign, but 10000 is default |
| `MONGODB_URI` | `your_mongodb_connection_string` | Get this from MongoDB Atlas |
| `JWT_SECRET` | `generate_a_random_secure_string` | Use a strong random string (min 32 chars) |

### Optional Variables (for image uploads)

| Key | Value |
|-----|-------|
| `CLOUDINARY_CLOUD_NAME` | Your Cloudinary cloud name |
| `CLOUDINARY_API_KEY` | Your Cloudinary API key |
| `CLOUDINARY_API_SECRET` | Your Cloudinary API secret |

**💡 To generate a secure JWT_SECRET:**
```bash
node -e "console.log(require('crypto').randomBytes(32).toString('hex'))"
```

---

## Step 4: MongoDB Atlas Setup (if you haven't already)

1. Go to [MongoDB Atlas](https://www.mongodb.com/cloud/atlas)
2. Create a free cluster
3. Create a database user with a username and password
4. Whitelist IP: `0.0.0.0/0` (allow access from anywhere) for Render
5. Get your connection string:
   - Click **"Connect"** → **"Connect your application"**
   - Copy the connection string
   - Replace `<password>` with your database user password
   - Replace `<dbname>` with your database name (e.g., `sonar_db`)
   
Example:
```
mongodb+srv://username:password@cluster0.xxxxx.mongodb.net/sonar_db?retryWrites=true&w=majority
```

---

## Step 5: Deploy

1. Click **"Create Web Service"**
2. Render will automatically build and deploy your app
3. Monitor the build logs for any errors
4. Once deployed, you'll get a URL like: `https://sonar-chat-app.onrender.com`

---

## Step 6: Verify Deployment

1. Visit your Render URL
2. Try creating an account and logging in
3. Test the chat functionality
4. Check the logs in Render dashboard if you encounter issues

---

## Common Issues & Solutions

### Issue: Build fails with "cp: command not found"
**Solution**: Render uses Linux, so the `cp` command should work. If using Windows locally, use the `build:linux` script.

### Issue: "Cannot find module 'express'"
**Solution**: Ensure the build command includes `npm install --prefix backend`

### Issue: "MongoDB connection failed"
**Solution**: 
- Check your `MONGODB_URI` is correct
- Ensure MongoDB Atlas whitelist includes `0.0.0.0/0`
- Verify your database user credentials

### Issue: Frontend shows but API calls fail
**Solution**: 
- Check that `NODE_ENV=production` is set
- Verify the backend is serving the frontend build from `backend/dist`
- Check browser console for errors

### Issue: WebSocket/Socket.IO not connecting
**Solution**: Render's free tier may have limitations. Ensure your Socket.IO client is configured to use polling as fallback.

---

## Environment Variables Summary

Copy this checklist and replace with your actual values:

```bash
# Required
NODE_ENV=production
PORT=10000
MONGODB_URI=mongodb+srv://username:password@cluster0.xxxxx.mongodb.net/sonar_db?retryWrites=true&w=majority
JWT_SECRET=your_secure_random_string_here

# Optional (for Cloudinary image uploads)
CLOUDINARY_CLOUD_NAME=your_cloud_name
CLOUDINARY_API_KEY=your_api_key
CLOUDINARY_API_SECRET=your_api_secret
```

---

## Updating Your Deployment

When you push changes to your GitHub repository:
1. Render will automatically detect the changes
2. It will rebuild and redeploy your app
3. Monitor the deploy logs in Render dashboard

To manually trigger a deploy:
1. Go to your web service in Render
2. Click **"Manual Deploy"** → **"Deploy latest commit"**

---

## Monitoring & Logs

- **View Logs**: Go to your web service → **"Logs"** tab
- **Health Check**: Your app has a `/health` endpoint at `https://your-app.onrender.com/health`
- **Metrics**: Check the **"Metrics"** tab for performance data

---

## Cost Optimization

**Free Tier Limitations:**
- Spins down after 15 minutes of inactivity
- First request after spin-down takes 30-60 seconds
- 750 hours/month free

**To prevent spin-down:**
- Upgrade to a paid plan ($7/month)
- Or use an external service to ping your app every 10 minutes

---

## Security Checklist

- ✅ `.env` file is NOT committed to git (check `.gitignore`)
- ✅ JWT_SECRET is a strong random string
- ✅ MongoDB user has limited permissions
- ✅ CORS is properly configured in `backend/src/index.js`
- ✅ Environment variables are set in Render dashboard, not in code

---

## Support

If you encounter issues:
1. Check Render logs for error messages
2. Verify all environment variables are set correctly
3. Test your MongoDB connection separately
4. Check the `/health` endpoint: `https://your-app.onrender.com/health`

---

## Next Steps After Deployment

- Set up a custom domain (in Render dashboard)
- Configure SSL/HTTPS (automatic with Render)
- Set up monitoring and alerting
- Consider upgrading to a paid plan for better performance
- Implement backup strategy for your MongoDB data
