import { defineConfig } from "vite";

const api = "http://127.0.0.1:8787";

export default defineConfig({
  server: {
    host: "127.0.0.1",
    port: 5173,
    proxy: { "/api": api },
  },
  preview: {
    host: "127.0.0.1",
    port: 4173,
    proxy: { "/api": api },
  },
});
