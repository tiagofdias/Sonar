import axios from "axios";

const getBaseURL = () => {
  if (import.meta.env.MODE === "development") {
    return "http://localhost:5001/api";
  }
  
  // In production, use the backend URL from environment or construct from service name
  const backendUrl = import.meta.env.VITE_BACKEND_URL;
  if (backendUrl) {
    return `${backendUrl}/api`;
  }
  
  // Default to the expected Render backend service URL
  return "https://sonar-backend.onrender.com/api";
};

export const axiosInstance = axios.create({
  baseURL: getBaseURL(),
  withCredentials: true,
});
