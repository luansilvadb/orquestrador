import { defineConfig } from 'vite';

// Porta fixa: é a que o `devUrl` do Tauri espera. O Vite não precisa vigiar o lado Rust.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ['**/src-tauri/**'] } },
  build: { target: 'es2022' },
});
