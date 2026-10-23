// Setup global do `npm run check:ui`: um Vite só para todos os arquivos, que rodam em paralelo (cada um com o próprio Chromium).
import { BASE, launchBrowser, startVite, waitForServer } from './editor-ui-harness.mjs';

let vite;
export async function globalSetup() {
  vite = startVite();
  await waitForServer();
  // Primeira carga transforma o app inteiro (o React Compiler passa por cada módulo): feita aqui, os testes não esbarram no timeout do goto.
  const browser = await launchBrowser();
  try {
    await (await browser.newPage()).goto(BASE, { timeout: 120000 });
  } finally {
    await browser.close();
  }
}
export async function globalTeardown() {
  vite?.stop();
}
