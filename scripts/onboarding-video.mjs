// Grava o app simulado com Playwright e entrega o MP4 otimizado em docs/.
import { spawn } from 'node:child_process';
import { copyFile, mkdir, mkdtemp, rm, stat, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { BASE, OUT, ROOT, launchBrowser, openApp, startVite, treeItem, waitForServer } from './editor-ui-harness.mjs';

const SIZE = { width: 1600, height: 900 };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function run(command, args, { capture = false } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: ROOT, windowsHide: true, stdio: ['ignore', capture ? 'pipe' : 'inherit', 'inherit'] });
    let stdout = '';
    child.stdout?.on('data', (chunk) => { stdout += chunk; });
    child.once('error', reject);
    child.once('close', (code) => code === 0 ? resolve(stdout) : reject(new Error(`${command} terminou com código ${code}`)));
  });
}

async function waitForTerminalText(page, text) {
  await page.waitForFunction((needle) => [...document.querySelectorAll('.xterm-rows')].some((el) => el.textContent.includes(needle)), text);
}

async function startScreencast(context, page, directory) {
  const cdp = await context.newCDPSession(page);
  const frames = [];
  let queue = Promise.resolve();
  let captureError;
  let index = 0;
  cdp.on('Page.screencastFrame', (event) => {
    const receivedAt = performance.now();
    const name = `frame-${String(index++).padStart(5, '0')}.png`;
    queue = queue.then(async () => {
      try {
        const png = Buffer.from(event.data, 'base64');
        await writeFile(join(directory, name), png);
        frames.push({
          name,
          timestamp: Number(event.metadata.timestamp),
          receivedAt,
          width: png.readUInt32BE(16),
          height: png.readUInt32BE(20),
        });
      } catch (error) {
        captureError ??= error;
      }
      try {
        await cdp.send('Page.screencastFrameAck', { sessionId: event.sessionId });
      } catch (error) {
        captureError ??= error;
      }
    });
  });
  await cdp.send('Page.enable');
  await cdp.send('Page.startScreencast', { format: 'png', maxWidth: SIZE.width, maxHeight: SIZE.height, everyNthFrame: 1 });

  let stopPromise;
  return {
    stop() {
      stopPromise ??= (async () => {
        await cdp.send('Page.stopScreencast');
        const stoppedAt = performance.now();
        await queue;
        if (captureError) throw captureError;
        if (!frames.length) throw new Error('O screencast não capturou quadros.');
        const last = frames.at(-1);
        const endTimestamp = last.timestamp + Math.max(0, (stoppedAt - last.receivedAt) / 1000);
        return { frames, endTimestamp };
      })();
      return stopPromise;
    },
  };
}

async function encode(frames, endTimestamp, directory, output) {
  const list = frames.map((frame, index) => {
    const nextTimestamp = frames[index + 1]?.timestamp ?? endTimestamp;
    const duration = nextTimestamp - frame.timestamp;
    if (!Number.isFinite(duration) || duration <= 0) throw new Error(`Duração de quadro inválida: ${duration}.`);
    return `file '${frame.name}'\nduration ${duration.toFixed(6)}`;
  });
  list.push(`file '${frames.at(-1).name}'`);
  const concatList = join(directory, 'frames.txt');
  await writeFile(concatList, `${list.join('\n')}\n`);
  const needsScale = frames.some((frame) => frame.width !== SIZE.width || frame.height !== SIZE.height);
  const filter = `${needsScale ? `scale=${SIZE.width}:${SIZE.height}:flags=lanczos,` : ''}fps=30,format=yuv420p`;
  await run('ffmpeg', [
    '-hide_banner', '-loglevel', 'error', '-y', '-f', 'concat', '-safe', '0', '-i', concatList,
    '-vf', filter, '-c:v', 'libx264', '-preset', 'slow', '-crf', '14', '-tune', 'stillimage', '-movflags', '+faststart', '-an', output,
  ]);
  const data = JSON.parse(await run('ffprobe', [
    '-v', 'error', '-show_entries', 'format=duration,bit_rate', '-show_entries', 'stream=codec_type,codec_name,width,height,pix_fmt', '-of', 'json', output,
  ], { capture: true }));
  const duration = Number(data.format.duration);
  const video = data.streams.find((stream) => stream.codec_type === 'video');
  const bytes = (await stat(output)).size;
  if (!video || video.width !== SIZE.width || video.height !== SIZE.height) throw new Error('O MP4 não ficou em 1600×900.');
  if (data.streams.some((stream) => stream.codec_type === 'audio')) throw new Error('O MP4 deveria estar sem áudio.');
  if (duration < 55 || duration > 90) throw new Error(`Duração fora de 55–90 s: ${duration.toFixed(2)} s.`);
  if (bytes >= 40 * 1024 * 1024) throw new Error(`O MP4 excedeu 40 MB: ${bytes} bytes.`);
  return { duration, bytes, bitRate: Number(data.format.bit_rate), codec: video.codec_name, pixelFormat: video.pix_fmt };
}

const vite = startVite();
let browser, context, tempDir, screencast;
try {
  await waitForServer();
  await mkdir(OUT, { recursive: true });
  tempDir = await mkdtemp(join(OUT, 'onboarding-video-'));
  browser = await launchBrowser();

  // A primeira compilação do Vite fica fora da gravação.
  const warm = await browser.newContext({ viewport: SIZE });
  const warmPage = await warm.newPage();
  warmPage.setDefaultNavigationTimeout(120000);
  await warmPage.goto(BASE, { timeout: 120000 });
  await warmPage.getByRole('navigation', { name: 'Telas' }).waitFor();
  await warm.close();

  context = await browser.newContext({ viewport: SIZE, deviceScaleFactor: 1 });
  const page = await context.newPage();
  page.setDefaultTimeout(120000);
  page.setDefaultNavigationTimeout(120000);
  await openApp(page);
  await treeItem(page, 'README.md').click();
  await page.locator('.cm-content:visible').first().waitFor();

  screencast = await startScreencast(context, page, tempDir);
  await sleep(4000);
  await sleep(4000);

  await page.getByRole('button', { name: 'Abrir terminal', exact: true }).last().click();
  const choice = page.getByRole('dialog', { name: 'Orquestrador' });
  await choice.waitFor();
  await sleep(3000);
  await choice.getByRole('button', { name: 'Codex', exact: true }).click();
  await choice.getByRole('combobox').nth(0).click();
  await page.getByRole('option', { name: 'gpt-5.5', exact: true }).click();
  await choice.getByRole('combobox').nth(1).click();
  await page.getByRole('option', { name: 'Alto', exact: true }).click();
  await choice.getByRole('button', { name: 'Abrir terminal', exact: true }).click();
  await page.locator('.xterm:visible').first().waitFor();
  await sleep(4000);

  await sleep(3500);
  await page.locator('.xterm:visible').first().click();
  await page.keyboard.type('Analise o projeto e proponha um plano curto.', { delay: 60 });
  await page.keyboard.press('Enter');
  await sleep(1300);
  await page.evaluate(() => window.__mock.print('orchestrator', '\r\nPlano:\r\n1. Revisar a documentação.\r\n2. Dividir o trabalho em duas tarefas.\r\n'));
  await waitForTerminalText(page, 'Revisar a documentação.');
  await sleep(4000);

  await page.evaluate(() => {
    const choice1 = { tool: 'codex', model: 'gpt-5.5', effort: 'medium' };
    const choice2 = { tool: 'claude', model: 'sonnet', effort: 'high' };
    window.__mock.addNode({ id: 'executor-1', project: 'project-1', batch: 'onboarding', index: 0, title: 'Revisar documentação', state: 'running', choice: choice1 });
    window.__mock.addNode({ id: 'executor-2', project: 'project-1', batch: 'onboarding', index: 1, title: 'Revisar estrutura', state: 'running', choice: choice2 });
  });
  await page.locator('[data-tab="executor-1"][role="tab"]').waitFor();
  await page.locator('[data-tab="executor-2"][role="tab"]').waitFor();
  await page.locator('[data-tab="executor-1"][role="tab"]').click();
  await page.locator('.xterm:visible').first().waitFor();
  await page.evaluate(() => window.__mock.print('executor-1', 'Documentação revisada. Sugestões organizadas por prioridade.\r\n'));
  await waitForTerminalText(page, 'Documentação revisada.');
  await sleep(4000);
  await page.locator('[data-tab="executor-2"][role="tab"]').click();
  await page.evaluate(() => window.__mock.print('executor-2', 'Estrutura revisada. Arquivos principais mapeados.\r\n'));
  await waitForTerminalText(page, 'Estrutura revisada.');
  await sleep(4000);
  await page.locator('[data-tab="orchestrator"][role="tab"]').click();
  await page.evaluate(() => {
    window.__mock.print('orchestrator', '\r\nAs duas tarefas foram concluídas. Segue o resumo do trabalho.\r\n');
    window.__mock.nodeState({ project: 'project-1', id: 'executor-1', state: 'done', choice: { tool: 'codex', model: 'gpt-5.5', effort: 'medium' } });
    window.__mock.nodeState({ project: 'project-1', id: 'executor-2', state: 'done', choice: { tool: 'claude', model: 'sonnet', effort: 'high' } });
  });
  await sleep(4000);

  await page.getByRole('navigation', { name: 'Telas' }).getByRole('button', { name: /^Git/ }).click();
  const staged = page.locator('section').filter({ has: page.getByRole('button', { name: /^Preparadas\b/ }) });
  await staged.locator('button[title^="docs/design-system.md: "]').dblclick();
  const diff = page.locator('[data-body]:visible .compare-stage');
  await diff.locator('pre[data-diff-type] [data-deletions] [data-content]')
    .filter({ hasText: 'Monoespaçado só em terminal.' }).waitFor();
  await diff.locator('pre[data-diff-type] [data-additions] [data-content]')
    .filter({ hasText: 'Monoespaçado só em terminal e valores técnicos.' }).waitFor();
  await sleep(4000);
  const message = page.getByRole('textbox', { name: /^Mensagem do commit/ });
  await message.click();
  await page.keyboard.type('Atualiza a documentação', { delay: 65 });
  await sleep(2500);
  await page.getByRole('button', { name: 'Commit', exact: true }).click();
  await page.waitForFunction(() => window.__mock.calls.some((call) => call.cmd === 'git_commit' && !call.error));
  await sleep(4000);

  await page.getByRole('navigation', { name: 'Telas' }).getByRole('button', { name: 'Arquivos', exact: true }).click();
  await treeItem(page, 'README.md').click();
  await page.locator('.cm-content:visible').first().waitFor();
  await sleep(6000);

  const { frames, endTimestamp } = await screencast.stop();
  screencast = null;
  await context.close();
  context = null;
  const temporaryMp4 = join(tempDir, 'onboarding.mp4');
  const output = join(ROOT, 'docs', 'onboarding.mp4');
  const result = await encode(frames, endTimestamp, tempDir, temporaryMp4);
  await copyFile(temporaryMp4, output);
  console.log(`Vídeo: docs/onboarding.mp4 · 1600×900 · ${result.duration.toFixed(2)} s · ${(result.bytes / 1024 / 1024).toFixed(2)} MB · ${result.bitRate} bit/s · ${result.codec} · ${result.pixelFormat}`);
} finally {
  await screencast?.stop().catch(() => {});
  await context?.close().catch(() => {});
  await browser?.close().catch(() => {});
  vite.stop();
  if (tempDir) await rm(tempDir, { recursive: true, force: true });
}
