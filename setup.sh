#!/bin/bash

# Setup script for Sonar Chat App

echo "🚀 Setting up Sonar Chat App..."

# Install root dependencies
echo "📦 Installing root dependencies..."
npm install

# Install backend dependencies
echo "📦 Installing backend dependencies..."
cd backend && npm install && cd ..

# Install frontend dependencies  
echo "📦 Installing frontend dependencies..."
cd frontend && npm install && cd ..

# Create environment files if they don't exist
if [ ! -f backend/.env ]; then
    echo "📝 Creating backend .env file..."
    cp backend/.env.example backend/.env
    echo "⚠️  Please edit backend/.env with your configuration"
fi

if [ ! -f frontend/.env ]; then
    echo "📝 Creating frontend .env file..."
    cp frontend/.env.example frontend/.env
    echo "ℹ️  Frontend .env created (optional for local development)"
fi

echo "✅ Setup complete!"
echo ""
echo "🔧 Next steps:"
echo "1. Edit backend/.env with your MongoDB URI and other settings"
echo "2. Start development with: npm run dev"
echo "3. Backend will run on http://localhost:5000" 
echo "4. Frontend will run on http://localhost:5173"
echo ""
echo "🌐 To deploy to Render:"
echo "1. Push your code to GitHub"
echo "2. Create a new Blueprint on Render"
echo "3. Connect your repository"
echo "4. The render.yaml file will handle the rest!"
