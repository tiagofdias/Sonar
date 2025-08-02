# Sonar Chat App - Render Deployment Guide

This is a full-stack real-time chat application built with React, Express.js, Socket.IO, and MongoDB.

## 🚀 Deploy to Render with Blueprint

This project is configured to deploy on Render using their Blueprint feature, which will automatically set up both your backend API and frontend static site.

### Prerequisites

1. A [Render](https://render.com) account
2. A GitHub repository with this code
3. Cloudinary account for image uploads (optional)

### Deployment Steps

1. **Fork/Clone this repository** to your GitHub account

2. **Connect to Render**:
   - Go to [Render Dashboard](https://dashboard.render.com)
   - Click "New" → "Blueprint"
   - Connect your GitHub repository
   - Select this repository

3. **Configure Environment Variables**:
   
   The blueprint will automatically create the following services:
   - `sonar-backend` - Express.js API server
   - `sonar-frontend` - React static site
   - `sonar-database` - MongoDB database

   You'll need to set these environment variables in Render:

   **For the Backend Service:**
   - `CLOUDINARY_CLOUD_NAME` - Your Cloudinary cloud name (optional)
   - `CLOUDINARY_API_KEY` - Your Cloudinary API key (optional)
   - `CLOUDINARY_API_SECRET` - Your Cloudinary API secret (optional)

   **Note:** The following are automatically configured by the blueprint:
   - `NODE_ENV=production`
   - `PORT` - Auto-assigned by Render
   - `MONGODB_URI` - Auto-connected to the database
   - `JWT_SECRET` - Auto-generated secure secret

4. **Deploy**:
   - Click "Apply" to deploy your blueprint
   - Render will automatically build and deploy both services
   - The frontend will be automatically configured to connect to the backend

5. **Access Your App**:
   - Your backend API will be available at: `https://sonar-backend-xxx.onrender.com`
   - Your frontend will be available at: `https://sonar-frontend-xxx.onrender.com`

### Features

- ✅ Real-time messaging with Socket.IO
- ✅ User authentication with JWT
- ✅ Image sharing with Cloudinary
- ✅ Responsive UI with Tailwind CSS and DaisyUI
- ✅ Online user status
- ✅ Message history
- ✅ Profile management

### Local Development

1. **Clone the repository**:
   ```bash
   git clone <your-repo-url>
   cd sonar-chat-app
   ```

2. **Install dependencies**:
   ```bash
   npm install
   ```

3. **Set up environment variables**:
   - Copy `backend/.env.example` to `backend/.env`
   - Copy `frontend/.env.example` to `frontend/.env`
   - Fill in your environment variables

4. **Start development servers**:
   ```bash
   npm run dev
   ```

   This will start:
   - Backend on `http://localhost:5001`
   - Frontend on `http://localhost:5173`

### Project Structure

```
sonar-chat-app/
├── render.yaml              # Render Blueprint configuration
├── package.json             # Root package.json with scripts
├── backend/                 # Express.js API
│   ├── src/
│   │   ├── index.js        # Main server file
│   │   ├── controllers/    # Route controllers
│   │   ├── middleware/     # Custom middleware
│   │   ├── models/         # MongoDB models
│   │   ├── routes/         # API routes
│   │   └── lib/           # Utilities and configurations
│   └── package.json
└── frontend/               # React application
    ├── src/
    │   ├── components/     # React components
    │   ├── pages/         # Page components
    │   ├── store/         # Zustand state management
    │   └── lib/           # Frontend utilities
    └── package.json
```

### Troubleshooting

**If deployment fails:**

1. Check the build logs in Render dashboard
2. Ensure all required environment variables are set
3. Verify your `render.yaml` configuration
4. Check that your repository is properly connected

**Common issues:**

- **CORS errors**: The app is configured to handle cross-origin requests between services
- **Database connection**: MongoDB is automatically provisioned and connected
- **Environment variables**: JWT_SECRET is auto-generated for security

### Support

If you encounter any issues:

1. Check Render's documentation: https://render.com/docs
2. Review the deployment logs in your Render dashboard
3. Ensure your environment variables are properly configured

---

**Note**: The free tier on Render may experience cold starts. For production use, consider upgrading to a paid plan for better performance.
