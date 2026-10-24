import react, { reactCompilerPreset } from '@vitejs/plugin-react';
import babel from '@rolldown/plugin-babel';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

// Porta fixa: é a que o `devUrl` do Tauri espera. O Vite não precisa vigiar o lado Rust.
// React Compiler: memoiza os componentes, e uma ação num painel não renderiza o App inteiro de novo.
export default defineConfig({
  plugins: [react(), babel({ presets: [reactCompilerPreset()] }), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ['**/src-tauri/**'] } },
  build: { target: 'es2022' },
  // O worker do realce do diff (@pierre/diffs) importa o WASM do shiki sob demanda: só o formato ES divide o worker em chunks.
  worker: { format: 'es' },
});
