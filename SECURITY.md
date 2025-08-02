# Security Guidelines

## Environment Variables

**NEVER commit `.env` files to version control.**

### Required Environment Variables

For production deployment on Render, set these environment variables in your Render dashboard:

```bash
# Database - MongoDB Atlas connection string
MONGODB_URI=mongodb+srv://username:password@cluster.mongodb.net/database_name

# JWT Secret - Generate a strong random string
JWT_SECRET=your_very_secure_random_jwt_secret_here

# Node Environment
NODE_ENV=production

# Cloudinary (for image uploads)
CLOUDINARY_CLOUD_NAME=your_cloudinary_cloud_name
CLOUDINARY_API_KEY=your_cloudinary_api_key
CLOUDINARY_API_SECRET=your_cloudinary_api_secret

# Frontend URL (for CORS)
FRONTEND_URL=https://your-render-app-url.onrender.com
```

### Local Development

1. Copy `.env.example` to `.env` in the backend directory
2. Fill in your local development values
3. Never commit the `.env` file

### If Credentials Were Exposed

If you accidentally committed sensitive data:

1. **Immediately rotate all exposed credentials**:
   - Generate new JWT secret
   - Regenerate Cloudinary API keys
   - Change MongoDB password
   
2. **Update environment variables** in Render dashboard

3. **Consider using `git filter-branch` or BFG Repo-Cleaner** to remove sensitive data from git history if it was committed

## Best Practices

- Use strong, randomly generated secrets
- Regularly rotate API keys and secrets
- Use environment-specific configurations
- Never hardcode sensitive values in source code
