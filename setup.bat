@echo off

echo 🚀 Setting up Sonar Chat App...

REM Install root dependencies
echo 📦 Installing root dependencies...
npm install

REM Install backend dependencies
echo 📦 Installing backend dependencies...
cd backend
npm install
cd ..

REM Install frontend dependencies
echo 📦 Installing frontend dependencies...
cd frontend
npm install
cd ..

REM Create environment files if they don't exist
if not exist backend\.env (
    echo 📝 Creating backend .env file...
    copy backend\.env.example backend\.env
    echo ⚠️  Please edit backend\.env with your configuration
)

if not exist frontend\.env (
    echo 📝 Creating frontend .env file...
    copy frontend\.env.example frontend\.env
    echo ℹ️  Frontend .env created ^(optional for local development^)
)

echo ✅ Setup complete!
echo.
echo 🔧 Next steps:
echo 1. Edit backend\.env with your MongoDB URI and other settings
echo 2. Start development with: npm run dev
echo 3. Backend will run on http://localhost:5000
echo 4. Frontend will run on http://localhost:5173
echo.
echo 🌐 To deploy to Render:
echo 1. Push your code to GitHub
echo 2. Create a new Blueprint on Render
echo 3. Connect your repository
echo 4. The render.yaml file will handle the rest!

pause
