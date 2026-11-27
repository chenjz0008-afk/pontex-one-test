import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// PonteX One 自己的前端入口，不打包上游界面。
export default defineConfig({
  root: path.resolve(__dirname, "src/pontex"),
  base: "./",
  plugins: [react()],
  resolve: { alias: { "@": path.resolve(__dirname, "./src") } },
  build: { outDir: path.resolve(__dirname, "dist/pontex"), emptyOutDir: true },
  css: { postcss: { plugins: [] } },
  server: { port: 3310, strictPort: true },
  clearScreen: false,
});
