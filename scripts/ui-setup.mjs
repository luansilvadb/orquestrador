// Setup global do `npm run check:ui`: um Vite só para todos os arquivos, que rodam em paralelo (cada um com o próprio Chromium).
import { startVite, waitForServer } from './editor-ui-harness.mjs';

let vite;
export async function globalSetup() {
  vite = startVite();
  await waitForServer();
}
export async function globalTeardown() {
  vite?.stop();
}
