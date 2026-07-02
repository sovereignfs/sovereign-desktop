import { defineConfig } from 'vite';

const host = process.env.TAURI_DEV_HOST;

// Vite options tailored for Tauri development, applied in `tauri dev` / `tauri build`.
export default defineConfig({
  // Prevent Vite from obscuring Rust errors.
  clearScreen: false,
  // Tauri expects a fixed port; fail if it is not available.
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    // Tell Vite to ignore watching `src-tauri`.
    watch: { ignored: ['**/src-tauri/**'] },
  },
});
