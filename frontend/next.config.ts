import type { NextConfig } from "next";

// Static export so the Tauri shell can serve the frontend as plain HTML/JS/CSS
// assets (no Node server at runtime). `next build` emits to ./out, which
// tauri.conf.json references as frontendDist (../frontend/out).
const nextConfig: NextConfig = {
  output: "export",
  // Disable Image Optimization: it requires a server and is unsupported under
  // static export. Safe to keep even before next/image is used.
  images: {
    unoptimized: true,
  },
};

export default nextConfig;
