// QA de interface dos fluxos de arquivos (árvore) e de pesquisa/substituição, em Chrome headless contra o app simulado (src/mock.ts,
// só no `vite` de desenvolvimento). Usa o mesmo loader do check-editor-ui (editor-ui-harness.mjs): cada cenário roda com contexto novo,
// clipboard fake e a árvore carregada. O mock só entra na fronteira de I/O: fixtures (`__mock.addFile`, `__mock.addFolder`), edição externa
// (`__mock.external`), disco (`__mock.disk`), falha de I/O (`__mock.failOn`) e o clipboard simulado (`__mock.clipboard`).
// Uso: node scripts/check-files-search-ui.mjs   (saída 0 = todos os cenários passaram; 1 = algum falhou)
// Capturas em .playwright-mcp/files-search-*.png: evidência para leitura humana, não asserção.
import { join } from 'node:path';
import assert from 'node:assert/strict';
import { OUT, uiSuite, expand, treeItem, tabs, editorText, focusEditor, columnPoint, primaryCursor } from './editor-ui-harness.mjs';

const { scenario, run } = uiSuite('files-search');

// ---------- helpers ----------

const disk = (page, rel) => page.evaluate((r) => window.__mock.disk(r), rel);
// Linha da árvore pelo caminho relativo (o Tree põe o caminho em `data-tree-path` no nome).
const at = (page, path) => page.locator(`[data-tree-path="${path}"]`);
// O nome do item traz o atalho junto ("Renomear... F2"): casa pelo começo do texto.
const menu = (page, name) => page.getByRole('menuitem', { name });
const rightClick = (page, name, nth = 0) => treeItem(page, name, nth).click({ button: 'right' });
const selected = async (page, name) => (await treeItem(page, name).getAttribute('aria-selected')) === 'true';
const dirtyCount = (page) => page.locator('[aria-label="Alteração não salva"]').count();
const pesquisar = (page) => page.getByRole('button', { name: 'Pesquisar', exact: true }).click();
const arquivos = (page) => page.getByRole('button', { name: 'Arquivos', exact: true }).click();
const field = (page) => page.getByRole('textbox', { name: 'Pesquisar nos arquivos do projeto' });
const replaceField = (page) => page.getByRole('textbox', { name: 'Substituir por' });
// Os substitutos que a prévia mostra na lista, em ordem.
const added = (page) => page.locator('.search-hit-added').allTextContents();
// Confirma o Substituir Tudo no diálogo com o título dado.
const confirmReplace = (page, title) => page.getByRole('dialog', { name: title }).getByRole('button', { name: 'Substituir', exact: true }).click();
const toggle = (page, label) => page.getByRole('button', { name: label, exact: true }).click();
const save = async (page) => { await focusEditor(page); await page.keyboard.press('Control+s'); };
// Diálogo de confirmação de Renomear/Criar/Excluir: o botão com o nome dado dentro do diálogo com o título dado.
const dialogBtn = (page, title, name) => page.getByRole('dialog', { name: title }).getByRole('button', { name, exact: true });

// ---------- cenários ----------

scenario('pasta e arquivo criados dentro de pasta por clique direito; renomear pasta com rascunho mantém o texto sujo e salva no caminho novo; excluir cancelado preserva arquivo e rascunho', async ({ page }) => {
  await expand(page, ['docs']);
  // Criar dentro da pasta, não na raiz: o diálogo diz onde será criado, e o caminho confirma.
  await rightClick(page, 'docs');
  await menu(page, 'Nova Pasta...').click();
  await page.getByRole('textbox', { name: 'Nome da nova pasta' }).fill('projeto');
  await dialogBtn(page, 'Nova pasta', 'Criar').click();
  await at(page, 'docs/projeto').waitFor();
  await rightClick(page, 'projeto');
  await menu(page, 'Novo Arquivo...').click();
  await page.getByRole('textbox', { name: 'Nome do novo arquivo' }).fill('marcador.md');
  await dialogBtn(page, 'Novo arquivo', 'Criar').click();
  await at(page, 'docs/projeto/marcador.md').waitFor();

  // Marcador digitado no arquivo: sujo, sem salvar.
  await treeItem(page, 'marcador.md').dblclick();
  await focusEditor(page);
  await page.keyboard.type('MARCADOR-1');
  await page.locator('[aria-label="Alteração não salva"]').first().waitFor();

  // Renomear a pasta com o arquivo sujo aberto: não pede para salvar antes.
  await rightClick(page, 'projeto');
  await menu(page, 'Renomear...').click();
  await page.getByRole('textbox', { name: 'Novo nome' }).fill('projeto-novo');
  await dialogBtn(page, 'Renomear pasta', 'Renomear').click();
  await at(page, 'docs/projeto-novo/marcador.md').waitFor();
  assert.equal(await at(page, 'docs/projeto').count(), 0, 'a pasta antiga saiu da árvore');
  assert.ok(await dirtyCount(page) >= 1, 'o texto continua sujo depois de renomear a pasta');
  assert.ok((await editorText(page)).includes('MARCADOR-1'), 'o texto digitado continua no editor');
  await page.getByRole('dialog', { name: 'Renomear pasta' }).waitFor({ state: 'detached' });
  await page.screenshot({ path: join(OUT, 'files-search-arvore-renomear.png') });

  // Salvar agora vai para o caminho novo; o antigo não reaparece.
  await save(page);
  await page.locator('[aria-label="Alteração não salva"]').first().waitFor({ state: 'detached' });
  assert.equal(await disk(page, 'docs/projeto-novo/marcador.md'), 'MARCADOR-1', 'gravou no caminho novo');
  assert.equal(await disk(page, 'docs/projeto/marcador.md'), '', 'nada foi gravado no caminho antigo');

  // Rascunho novo, depois excluir a pasta e cancelar: arquivo e rascunho ficam como estão.
  await focusEditor(page);
  await page.keyboard.type(' MARCADOR-2');
  await page.locator('[aria-label="Alteração não salva"]').first().waitFor();
  await rightClick(page, 'projeto-novo');
  await menu(page, 'Excluir...').click();
  const ask = page.getByRole('dialog', { name: 'Excluir pasta?' });
  await ask.waitFor();
  await ask.getByRole('button', { name: 'Cancelar', exact: true }).click();
  await ask.waitFor({ state: 'detached' });
  assert.equal(await at(page, 'docs/projeto-novo/marcador.md').count(), 1, 'exclusão cancelada: o arquivo continua na árvore');
  assert.ok((await editorText(page)).includes('MARCADOR-2'), 'exclusão cancelada: o rascunho continua no editor');
  assert.ok(await dirtyCount(page) >= 1, 'exclusão cancelada: o rascunho continua sujo');
  assert.equal(await disk(page, 'docs/projeto-novo/marcador.md'), 'MARCADOR-1', 'exclusão cancelada: o disco não recebeu o rascunho');
});

scenario('seleção por Shift e Ctrl; recortar e colar em outra pasta com rascunho aberto (remapeado); cópia; arraste; falha de I/O no meio reconcilia só o que saiu', async ({ page }) => {
  await page.evaluate(() => {
    for (const n of ['alfa', 'beta', 'delta', 'gama', 'p1', 'p2', 'p3']) window.__mock.addFile(`docs/${n}.md`, `${n.toUpperCase()}\n`);
  });
  await expand(page, ['docs']);
  await treeItem(page, 'delta.md').waitFor();
  const designOriginal = await disk(page, 'docs/design-system.md');

  // Rascunho em delta.md (aberto e sujo) que vai junto no recorte.
  await treeItem(page, 'delta.md').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+End');
  await page.keyboard.type(' RASC-DELTA');

  // Shift entre dois arquivos vizinhos: só eles entram na seleção.
  await treeItem(page, 'beta.md').click();
  await treeItem(page, 'delta.md').click({ modifiers: ['Shift'] });
  assert.ok(await selected(page, 'beta.md') && await selected(page, 'delta.md'), 'Shift+clique seleciona o intervalo beta–delta');
  assert.equal(await selected(page, 'alfa.md'), false, 'alfa fora do intervalo');

  // Ctrl+X com a seleção: o clipboard simulado recebe os dois, em recorte; a árvore ainda não muda.
  await page.keyboard.press('Control+x');
  await page.waitForFunction(() => window.__mock.clipboard().paths.length === 2);
  const clip = await page.evaluate(() => window.__mock.clipboard());
  assert.equal(clip.cut, true, 'Ctrl+X põe em recorte');
  assert.deepEqual(clip.paths.map((p) => p.split('\\').slice(-2).join('/')).sort(), ['docs/beta.md', 'docs/delta.md'], 'clipboard tem os dois itens da seleção');
  assert.ok(await at(page, 'docs/beta.md').count() === 1, 'recortar não move nada antes de colar');

  // Colar na pasta tests: os dois saem de docs, o rascunho de delta segue o caminho novo.
  await rightClick(page, 'tests');
  await menu(page, 'Colar').click();
  await at(page, 'tests/delta.md').waitFor();
  assert.equal(await at(page, 'docs/beta.md').count(), 0, 'beta saiu de docs');
  assert.equal(await at(page, 'docs/delta.md').count(), 0, 'delta saiu de docs');
  assert.equal(await at(page, 'tests/beta.md').count(), 1, 'beta chegou em tests');
  assert.equal(await page.evaluate(() => window.__mock.clipboard().paths.length), 0, 'recorte colado esvazia o clipboard simulado');
  // A aba ativa virou beta (o último colado); o rascunho de delta continua na aba dele, sujo.
  await tabs(page).filter({ hasText: 'delta.md' }).first().click();
  assert.ok((await editorText(page)).includes('RASC-DELTA'), 'rascunho de delta continua no editor depois do recorte');
  assert.ok(await dirtyCount(page) >= 1, 'delta continua sujo depois do recorte');
  await save(page);
  await page.locator('[aria-label="Alteração não salva"]').first().waitFor({ state: 'detached' });
  assert.equal(await disk(page, 'tests/delta.md'), 'DELTA\n RASC-DELTA', 'salvar gravou o rascunho em tests/delta.md');
  assert.equal(await disk(page, 'docs/delta.md'), '', 'docs/delta.md não existe mais no disco');
  await page.screenshot({ path: join(OUT, 'files-search-arvore-colar.png') });

  // Cópia: gama e rascunho-painel vão para dados; as origens ficam, e o clipboard de texto do usuário não é tocado.
  await treeItem(page, 'gama.md').click();
  await treeItem(page, 'rascunho-painel.md').click({ modifiers: ['Control'] });
  await page.keyboard.press('Control+c');
  await page.waitForFunction(() => window.__mock.clipboard().paths.length === 2);
  assert.equal((await page.evaluate(() => window.__mock.clipboard())).cut, false, 'Ctrl+C põe em cópia');
  await rightClick(page, 'dados');
  await menu(page, 'Colar').click();
  await at(page, 'dados/gama.md').waitFor();
  assert.equal(await at(page, 'docs/gama.md').count(), 1, 'cópia deixa a origem');
  assert.equal(await disk(page, 'dados/gama.md'), 'GAMA\n', 'cópia gravou o texto no destino');
  assert.equal(await at(page, 'dados/rascunho-painel.md').count(), 1, 'a segunda cópia chegou');
  assert.deepEqual(await page.evaluate(() => window.__copied), [], 'nada foi escrito no clipboard de texto');

  // Arraste: design-system.md vai para dados e o texto é o mesmo.
  await treeItem(page, 'design-system.md').dragTo(treeItem(page, 'dados'));
  await at(page, 'dados/design-system.md').waitFor();
  assert.equal(await at(page, 'docs/design-system.md').count(), 0, 'arraste tirou a origem');
  assert.equal(await disk(page, 'dados/design-system.md'), designOriginal, 'arraste preservou o texto');
  await page.screenshot({ path: join(OUT, 'files-search-arvore-arraste.png') });

  // Falha de I/O no meio do lote (p2): p1 saiu; p2 e p3 ficam, e o rascunho de p3 não é mexido.
  await treeItem(page, 'p3.md').dblclick();
  await focusEditor(page);
  await page.keyboard.press('End');
  await page.keyboard.type(' RASC-P3');
  await treeItem(page, 'p1.md').click();
  await treeItem(page, 'p2.md').click({ modifiers: ['Control'] });
  await treeItem(page, 'p3.md').click({ modifiers: ['Control'] });
  await page.keyboard.press('Control+x');
  await page.waitForFunction(() => window.__mock.clipboard().paths.length === 3);
  const order = (await page.evaluate(() => window.__mock.clipboard().paths)).map((p) => p.split('\\').pop());
  assert.deepEqual(order, ['p1.md', 'p2.md', 'p3.md'], 'a colagem processa na ordem da seleção');
  await page.evaluate(() => { window.__mock.failOn = ['docs/p2.md']; });
  await rightClick(page, 'tests');
  await menu(page, 'Colar').click();
  await page.getByText('Falha de I/O simulada ao colar').first().waitFor();
  await page.evaluate(() => { window.__mock.failOn = []; });
  assert.equal(await at(page, 'tests/p1.md').count(), 1, 'p1 foi feito e aparece em tests');
  assert.equal(await at(page, 'docs/p1.md').count(), 0, 'p1 saiu de docs');
  assert.equal(await at(page, 'docs/p2.md').count(), 1, 'p2 (falhou) ficou em docs');
  assert.equal(await at(page, 'tests/p2.md').count(), 0, 'p2 (falhou) não aparece em tests');
  assert.equal(await at(page, 'docs/p3.md').count(), 1, 'p3 (não processado) ficou em docs');
  assert.equal(await at(page, 'tests/p3.md').count(), 0, 'p3 não foi tocado');
  assert.equal(await disk(page, 'tests/p1.md'), 'P1\n', 'p1 chegou com o texto');
  assert.equal(await disk(page, 'docs/p2.md'), 'P2\n', 'p2 não recebeu alteração');
  assert.equal(await disk(page, 'docs/p3.md'), 'P3\n', 'p3 não recebeu alteração no disco');
  await tabs(page).filter({ hasText: 'p3.md' }).first().click();
  assert.ok((await editorText(page)).includes('RASC-P3'), 'rascunho de p3 segue no editor, no caminho de origem');
  assert.ok(await dirtyCount(page) >= 1, 'p3 continua sujo');
  await page.screenshot({ path: join(OUT, 'files-search-arvore-parcial.png') });
});

scenario('pesquisa na pasta src não pega src2 e põe ./src no incluir; resultado abre linha e coluna exatas; a prévia aparece na lista e não grava; troca por arquivo grava só aquele; rascunho é barrado no Substituir Tudo', async ({ page }) => {
  await page.evaluate(() => {
    window.__mock.addFolder('src2');
    window.__mock.addFile('src/termo-a.ts', 'const termo = 1;\n');
    window.__mock.addFile('src/termo-b.ts', 'let x = termo + termo;\n');
    window.__mock.addFile('src2/termo-c.ts', 'termo\n');
  });
  await expand(page, ['src']);
  await treeItem(page, 'termo-a.ts').waitFor();

  // Controle: sem pasta, a pesquisa acha src2 também.
  await pesquisar(page);
  await field(page).fill('termo');
  await result(page, 'src2/termo-c.ts').waitFor();

  // Localizar na pasta src: a pasta vai para o incluir como ./src, e src2 (irmã com prefixo igual) não entra. A árvore só aparece na tela Arquivos.
  await arquivos(page);
  await rightClick(page, 'src');
  await menu(page, 'Localizar na Pasta...').click();
  await result(page, 'src/termo-a.ts', 1, 7).waitFor();
  assert.equal(await page.getByRole('textbox', { name: 'Incluir caminhos' }).inputValue(), './src', 'a pasta foi para o campo de incluir');
  const shown = await page.locator('[data-result]').evaluateAll((els) => els.map((el) => el.title));
  assert.ok(shown.length > 0 && shown.every((t) => t.startsWith('src/')), `escopo src: nenhum resultado de src2 (${JSON.stringify(shown)})`);
  // Uma linha por ocorrência: termo-b tem duas na mesma linha.
  await result(page, 'src/termo-b.ts', 1, 9).waitFor();
  await result(page, 'src/termo-b.ts', 1, 17).waitFor();
  await statusText(page, '3 resultados em 2 arquivos').waitFor();

  // Resultado abre na linha e coluna exatas (1:7 de termo-a.ts): o cursor cai no caractere 7 da linha 1.
  await result(page, 'src/termo-a.ts', 1, 7).click();
  await page.locator('.cm-content:visible').first().waitFor();
  await page.waitForTimeout(250);
  const point = await columnPoint(page, 1, 7);
  const cursor = await primaryCursor(page);
  assert.ok(!point.error && cursor && Math.abs(cursor.x - point.x) <= 2, `cursor na coluna 7 da linha 1 (cursor ${cursor?.x}, coluna ${point.x})`);
  await pesquisar(page);

  // Prévia na lista: cada ocorrência riscada com o substituto ao lado, e nada gravado.
  await toggle(page, 'Ativar/Desativar Substituição');
  await replaceField(page).fill('novo');
  await page.waitForFunction(() => document.querySelectorAll('.search-hit-added').length === 3);
  assert.deepEqual(await added(page), ['novo', 'novo', 'novo'], 'o substituto aparece ao lado de cada ocorrência');
  assert.equal(await page.locator('.search-hit-removed').count(), 3, 'cada ocorrência aparece riscada');
  assert.equal(await disk(page, 'src/termo-a.ts'), 'const termo = 1;\n', 'prévia não gravou termo-a');
  assert.equal(await disk(page, 'src/termo-b.ts'), 'let x = termo + termo;\n', 'prévia não gravou termo-b');
  await page.screenshot({ path: join(OUT, 'files-search-previa.png') });

  // Mudar a substituição refaz a prévia.
  await replaceField(page).fill('novo2');
  await page.waitForFunction(() => [...document.querySelectorAll('.search-hit-added')].every((el) => el.textContent === 'novo2'));
  await replaceField(page).fill('novo');
  await page.waitForFunction(() => [...document.querySelectorAll('.search-hit-added')].every((el) => el.textContent === 'novo'));

  // Troca por arquivo (botão na linha do arquivo, com o mouse sobre ela): só termo-a muda; termo-b fica igual no disco.
  await fileOf(page, 'src/termo-a.ts').hover();
  await page.getByRole('button', { name: 'Substituir tudo em src/termo-a.ts' }).click();
  await statusText(page, '1 ocorrência substituída em 1 arquivo por “novo”.').waitFor();
  assert.equal(await disk(page, 'src/termo-a.ts'), 'const novo = 1;\n', 'aplicado: termo-a trocado');
  assert.equal(await disk(page, 'src/termo-b.ts'), 'let x = termo + termo;\n', 'termo-b byte a byte igual');

  // termo-b fica sujo no editor depois da prévia. O Substituir Tudo confirma e deixa termo-b de fora, dizendo por quê.
  await arquivos(page);
  await treeItem(page, 'termo-b.ts').dblclick();
  await focusEditor(page);
  await page.keyboard.type(' // rascunho');
  await page.locator('[aria-label="Alteração não salva"]').first().waitFor();
  await pesquisar(page);
  await page.getByRole('button', { name: 'Substituir Tudo', exact: true }).click();
  await confirmReplace(page, 'Substituir 2 ocorrências em 1 arquivo?');
  await page.getByText('Ficaram de fora: src/termo-b.ts: alteração não salva no editor.').waitFor();
  assert.equal(await disk(page, 'src/termo-b.ts'), 'let x = termo + termo;\n', 'rascunho barrou a aplicação: disco intacto');
  await arquivos(page);
  assert.ok((await editorText(page)).includes('// rascunho'), 'rascunho de termo-b continua no editor');
});

scenario('substituição stale: edição externa depois da prévia gera erro por arquivo e preserva disco e rascunho; vazio apaga só o visto; regex com captura', async ({ page }) => {
  await page.evaluate(() => {
    window.__mock.addFile('src/stale.ts', 'marca-unica: 1\n');
    window.__mock.addFile('src/vazio.ts', 'a termo b termo_x termo c\n');
    window.__mock.addFile('src/versao.ts', 'v1 v22\n');
  });
  await expand(page, ['src']);
  // README com rascunho, sem relação com a substituição: tem de sobreviver a tudo abaixo.
  await treeItem(page, 'README.md').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+End');
  await page.keyboard.type(' RASCUNHO-README');
  await page.locator('[aria-label="Alteração não salva"]').first().waitFor();

  // Prévia com o arquivo como está; depois o disco muda por fora; aplicar dá erro daquele arquivo.
  await pesquisar(page);
  await field(page).fill('marca-unica');
  await toggle(page, 'Ativar/Desativar Substituição');
  await replaceField(page).fill('novo');
  await page.locator('.search-hit-added', { hasText: 'novo' }).waitFor();
  await page.evaluate(() => window.__mock.external('src/stale.ts', 'marca-unica: 2 (externo)\n'));
  await page.getByRole('button', { name: 'Substituir Tudo', exact: true }).click();
  await confirmReplace(page, 'Substituir 1 ocorrência em 1 arquivo?');
  await page.getByRole('alert').filter({ hasText: 'O arquivo mudou desde a prévia' }).waitFor();
  assert.equal(await disk(page, 'src/stale.ts'), 'marca-unica: 2 (externo)\n', 'erro: a edição externa não foi sobrescrita');
  await page.screenshot({ path: join(OUT, 'files-search-stale.png') });
  await arquivos(page);
  assert.ok((await editorText(page)).includes('RASCUNHO-README'), 'erro: o rascunho de README segue no editor');
  assert.ok(await dirtyCount(page) >= 1, 'erro: README continua sujo');
  await pesquisar(page);

  // Vazio apaga só as ocorrências vistas: "termo" inteira sai, "termo_x" fica. Sem texto no Substituir, a lista não mostra prévia.
  await field(page).fill('termo');
  await toggle(page, 'Palavra inteira');
  await replaceField(page).fill('');
  await result(page, 'src/vazio.ts', 1, 3).waitFor();
  assert.equal(await page.locator('.search-hit-removed').count(), 0, 'sem texto no Substituir, nada aparece riscado');
  await page.getByRole('button', { name: 'Substituir Tudo', exact: true }).click();
  const confirm = page.getByRole('dialog', { name: 'Substituir 2 ocorrências em 1 arquivo?' });
  await confirm.getByText(/nada \(apagar\)/).waitFor();
  await confirm.getByRole('button', { name: 'Substituir', exact: true }).click();
  await statusText(page, '2 ocorrências substituídas em 1 arquivo por nada (apagadas).').waitFor();
  assert.equal(await disk(page, 'src/vazio.ts'), 'a  b termo_x  c\n', 'vazio: só as duas ocorrências inteiras saíram');

  // Regex com captura: "v(\d+)" vira "$1.0" só no arquivo filtrado; a prévia mostra o substituto de cada ocorrência.
  await toggle(page, 'Palavra inteira');
  await toggle(page, 'Expressão regular');
  await field(page).fill('v(\\d+)');
  await toggle(page, 'Ativar/Desativar os Detalhes da Pesquisa');
  await page.getByRole('textbox', { name: 'Incluir caminhos' }).fill('src/versao.ts');
  await replaceField(page).fill('$1.0');
  await page.waitForFunction(() => document.querySelectorAll('.search-hit-added').length === 2);
  assert.deepEqual(await added(page), ['1.0', '22.0'], 'a prévia mostra a captura aplicada em cada ocorrência');
  await page.getByRole('button', { name: 'Substituir Tudo', exact: true }).click();
  await confirmReplace(page, 'Substituir 2 ocorrências em 1 arquivo?');
  await statusText(page, '2 ocorrências substituídas em 1 arquivo por “$1.0”.').waitFor();
  assert.equal(await disk(page, 'src/versao.ts'), '1.0 22.0\n', 'regex: captura $1 aplicada nas duas ocorrências');
});

scenario('árvore e editores abertos: Exibir como Árvore agrupa por pasta e recolhe a pasta; só nos editores abertos limita aos arquivos com aba', async ({ page }) => {
  await page.evaluate(() => {
    window.__mock.addFolder('src/arv');
    window.__mock.addFile('src/arv/um.ts', 'arvoremarca\n');
    window.__mock.addFile('src/arv/dois.ts', 'arvoremarca\n');
    window.__mock.addFile('docs/tres.md', 'arvoremarca\n');
  });
  await expand(page, ['src', 'arv']);
  await treeItem(page, 'dois.ts').waitFor();
  await treeItem(page, 'um.ts').dblclick();
  await tabs(page).filter({ hasText: 'um.ts' }).waitFor();
  await focusEditor(page);
  await pesquisar(page);
  await field(page).fill('arvoremarca');
  await statusText(page, '3 resultados em 3 arquivos').waitFor();

  // Árvore: "src/arv" vira uma linha só (pasta sem arquivo e com uma subpasta), com a contagem de baixo; recolher tira os arquivos dela.
  await toggle(page, 'Exibir como Árvore');
  const pasta = page.locator('button[data-result][title="src/arv"]');
  await pasta.filter({ hasText: 'src/arv' }).waitFor();
  assert.equal((await pasta.textContent()).replace('src/arv', '').trim(), '2', 'a pasta conta as ocorrências de dentro');
  await page.screenshot({ path: join(OUT, 'files-search-arvore.png') });
  await pasta.click();
  await result(page, 'src/arv/um.ts').waitFor({ state: 'detached' });
  await result(page, 'docs/tres.md').waitFor();
  await toggle(page, 'Exibir como Lista');
  await result(page, 'src/arv/um.ts').waitFor();

  // Só nos editores abertos: fica só um.ts, que tem aba.
  await toggle(page, 'Ativar/Desativar os Detalhes da Pesquisa');
  await toggle(page, 'Pesquisar só nos editores abertos');
  await statusText(page, '1 resultado em 1 arquivo').waitFor();
  await result(page, 'src/arv/um.ts').waitFor();
  await toggle(page, 'Pesquisar só nos editores abertos');
  await statusText(page, '3 resultados em 3 arquivos').waitFor();
});

// Resultado da pesquisa pelo caminho, linha e coluna (o botão traz `caminho:linha:coluna` no título).
const result = (page, path, line = 1, col = 1) => page.locator(`[data-result][title="${path}:${line}:${col}"]`);
// Linha de resultado inteira (o `[data-row]` que contém o resultado): é nele que fica o Dispensar da linha.
const rowOf = (page, path, line, col = 1) => page.locator('[data-row]', { has: result(page, path, line, col) });
// Cabeçalho de arquivo inteiro (o `[data-file]` cujo botão tem o caminho como título).
const fileOf = (page, path) => page.locator('[data-file]', { has: page.locator(`button[title="${path}"]`) });
const dismissBtn = (scope) => scope.getByRole('button', { name: 'Dispensar', exact: true });
// Texto exato do resumo (role=status) ou de qualquer outro trecho visível.
const statusText = (page, text) => page.getByText(text, { exact: true });
// Linha em edição do editor visível (o `.cm-activeLine` acompanha o cursor), esperada até bater com o texto.
const waitActiveLine = (page, text) => page.waitForFunction(
  (t) => document.querySelector('.cm-content:not([aria-hidden]) .cm-activeLine')?.textContent === t,
  text,
);

scenario('histórico: ArrowUp traz a consulta anterior e refaz a busca; ArrowDown volta para a mais recente', async ({ page }) => {
  await page.evaluate(() => {
    window.__mock.addFile('src/hist-a.ts', 'alfahist\n');
    window.__mock.addFile('src/hist-b.ts', 'betahist\n');
  });
  await expand(page, ['src']);
  await pesquisar(page);
  await field(page).fill('alfahist');
  await field(page).press('Enter');
  await result(page, 'src/hist-a.ts').waitFor();
  await field(page).fill('betahist');
  await field(page).press('Enter');
  await result(page, 'src/hist-b.ts').waitFor();

  // ArrowUp: a consulta anterior entra no campo e a busca roda de novo com ela (hist-a aparece, hist-b sai).
  await field(page).press('ArrowUp');
  assert.equal(await field(page).inputValue(), 'alfahist', 'ArrowUp traz a consulta anterior, não a atual');
  await result(page, 'src/hist-a.ts').waitFor();
  assert.equal(await result(page, 'src/hist-b.ts').count(), 0, 'ArrowUp refez a busca com a consulta anterior');
  await page.screenshot({ path: join(OUT, 'files-search-historico.png') });

  // ArrowDown: volta para a consulta mais recente e refaz a busca.
  await field(page).press('ArrowDown');
  assert.equal(await field(page).inputValue(), 'betahist', 'ArrowDown volta para a consulta mais recente');
  await result(page, 'src/hist-b.ts').waitFor();
});

scenario('dispensar: linha tira a linha e o resumo diminui; cabeçalho tira o arquivo; o disco não muda', async ({ page }) => {
  await page.evaluate(() => {
    window.__mock.addFile('src/disp-a.ts', 'dispensa um\ndispensa dois\n');
    window.__mock.addFile('src/disp-b.ts', 'dispensa tres\n');
  });
  await expand(page, ['src']);
  await pesquisar(page);
  await field(page).fill('dispensa');
  await field(page).press('Enter');
  await statusText(page, '3 resultados em 2 arquivos').waitFor();
  const antes = { a: await disk(page, 'src/disp-a.ts'), b: await disk(page, 'src/disp-b.ts') };

  // Dispensar a segunda linha de disp-a (o botão aparece com o mouse sobre a linha): ela sai, e o resumo cai para 2.
  await rowOf(page, 'src/disp-a.ts', 2).hover();
  await dismissBtn(rowOf(page, 'src/disp-a.ts', 2)).click();
  await result(page, 'src/disp-a.ts', 2).waitFor({ state: 'detached' });
  await statusText(page, '2 resultados em 2 arquivos').waitFor();
  assert.equal(await result(page, 'src/disp-a.ts', 1).count(), 1, 'dispensar uma linha não tira as outras do arquivo');

  // Dispensar o cabeçalho de disp-b: o arquivo inteiro sai, e o resumo cai para 1 em 1.
  await fileOf(page, 'src/disp-b.ts').hover();
  await dismissBtn(fileOf(page, 'src/disp-b.ts')).click();
  await fileOf(page, 'src/disp-b.ts').waitFor({ state: 'detached' });
  await statusText(page, '1 resultado em 1 arquivo').waitFor();
  await page.screenshot({ path: join(OUT, 'files-search-dispensar.png') });

  assert.equal(await disk(page, 'src/disp-a.ts'), antes.a, 'dispensar não gravou em disp-a');
  assert.equal(await disk(page, 'src/disp-b.ts'), antes.b, 'dispensar não gravou em disp-b');
});

scenario('menu de contexto: Copiar caminho põe o caminho relativo no clipboard de texto, sem tocar no clipboard de arquivos', async ({ page }) => {
  await page.evaluate(() => window.__mock.addFile('src/copia.ts', 'copiavel\n'));
  await expand(page, ['src']);
  await pesquisar(page);
  await field(page).fill('copiavel');
  await field(page).press('Enter');
  await result(page, 'src/copia.ts').waitFor();

  await result(page, 'src/copia.ts').click({ button: 'right' });
  await menu(page, 'Copiar caminho').click();
  await page.waitForFunction(() => window.__copied.length === 1);
  assert.deepEqual(await page.evaluate(() => window.__copied), ['src/copia.ts'], 'o clipboard de texto recebeu o caminho relativo');
  assert.deepEqual(await page.evaluate(() => window.__mock.clipboard()), { paths: [], cut: false }, 'o clipboard de arquivos do simulado não foi tocado');
});

scenario('preservar maiúsculas: a prévia põe a ocorrência em maiúsculas com maiúsculas, em minúsculas com minúsculas, e a inicial maiúscula só na primeira letra', async ({ page }) => {
  await page.evaluate(() => window.__mock.addFile('src/caixa.ts', 'Termo termo TERMO\n'));
  await expand(page, ['src']);
  await pesquisar(page);
  await field(page).fill('termo');
  await field(page).press('Enter');
  await result(page, 'src/caixa.ts').waitFor();

  await toggle(page, 'Ativar/Desativar Substituição');
  await replaceField(page).fill('novo');
  await toggle(page, 'Preservar maiúsculas');
  await page.waitForFunction(() => [...document.querySelectorAll('.search-hit-added')].map((el) => el.textContent).join(' ') === 'Novo novo NOVO');
  assert.equal(await disk(page, 'src/caixa.ts'), 'Termo termo TERMO\n', 'a prévia não gravou nada');
  await page.screenshot({ path: join(OUT, 'files-search-preservar-caixa.png') });

  await page.getByRole('button', { name: 'Substituir Tudo', exact: true }).click();
  await confirmReplace(page, 'Substituir 3 ocorrências em 1 arquivo?');
  await statusText(page, '3 ocorrências substituídas em 1 arquivo por “novo”.').waitFor();
  assert.equal(await disk(page, 'src/caixa.ts'), 'Novo novo NOVO\n', 'a aplicação gravou a caixa de cada ocorrência');
});

scenario('Ctrl+Shift+F com seleção: a palavra selecionada no editor vira a consulta e a busca roda', async ({ page }) => {
  await page.evaluate(() => window.__mock.addFile('src/selecao.ts', 'sinalunico aqui\n'));
  await expand(page, ['src']);
  await treeItem(page, 'selecao.ts').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+Home');
  await page.keyboard.press('Control+Shift+ArrowRight');
  await page.keyboard.press('Control+Shift+F');

  await result(page, 'src/selecao.ts').waitFor();
  assert.equal(await field(page).inputValue(), 'sinalunico', 'a seleção do editor entrou no campo da pesquisa');
  await statusText(page, '1 resultado em 1 arquivo').waitFor();
  await page.screenshot({ path: join(OUT, 'files-search-atalho-selecao.png') });
});

scenario('F4 e Shift+F4 andam pelos resultados: abrem a linha no editor, marcam aria-current e Shift+F4 volta', async ({ page }) => {
  await page.evaluate(() => window.__mock.addFile('src/f4.ts', 'marca f4\nlinha sem\nmarca f4 de novo\n'));
  await expand(page, ['src']);
  await pesquisar(page);
  await field(page).fill('marca f4');
  await field(page).press('Enter');
  await statusText(page, '2 resultados em 1 arquivo').waitFor();

  // F4 sem resultado marcado vai ao primeiro; cada F4 seguinte anda; Shift+F4 volta.
  await page.keyboard.press('F4');
  await result(page, 'src/f4.ts', 1).and(page.locator('[aria-current="true"]')).waitFor();
  await waitActiveLine(page, 'marca f4');
  await page.screenshot({ path: join(OUT, 'files-search-f4.png') });

  await page.keyboard.press('F4');
  await result(page, 'src/f4.ts', 3).and(page.locator('[aria-current="true"]')).waitFor();
  await waitActiveLine(page, 'marca f4 de novo');
  assert.equal(await page.locator('[data-result][aria-current="true"]').count(), 1, 'só um resultado marcado por vez');

  await page.keyboard.press('Shift+F4');
  await result(page, 'src/f4.ts', 1).and(page.locator('[aria-current="true"]')).waitFor();
  await waitActiveLine(page, 'marca f4');
});

scenario('atualizar ao salvar: salvar um arquivo que deixou de ter o termo tira o resultado da lista, sem nova busca', async ({ page }) => {
  await page.evaluate(() => window.__mock.addFile('src/trocavel.ts', 'trocavel aqui\nfica\n'));
  await expand(page, ['src']);
  await pesquisar(page);
  await field(page).fill('trocavel');
  await field(page).press('Enter');
  await result(page, 'src/trocavel.ts').waitFor();

  // Edita o arquivo pela árvore (a pesquisa sai da tela só para isso), tira o termo e salva.
  await arquivos(page);
  await treeItem(page, 'trocavel.ts').dblclick();
  await focusEditor(page);
  await page.keyboard.press('Control+a');
  await page.keyboard.type('sem o termo\nfica\n');
  await page.locator('[aria-label="Alteração não salva"]').first().waitFor();
  await save(page);
  await page.locator('[aria-label="Alteração não salva"]').first().waitFor({ state: 'detached' });

  // Nenhuma busca nova: a lista é refeita sozinha com a gravação.
  await pesquisar(page);
  await result(page, 'src/trocavel.ts').waitFor({ state: 'detached' });
  await statusText(page, 'Nenhum resultado.').waitFor();
  assert.equal(await field(page).inputValue(), 'trocavel', 'a consulta digitada continua no campo');
  assert.equal(await disk(page, 'src/trocavel.ts'), 'sem o termo\nfica\n', 'o disco recebeu a gravação');
  await page.screenshot({ path: join(OUT, 'files-search-atualiza-salvar.png') });
});


scenario('visual do Explorer: ícones do tema Symbols, cores e letras do Git (pasta herda com •), ignorado em cinza; barra com Recolher Tudo e Novo Arquivo; seção do projeto recolhe a árvore', async ({ page }) => {
  const color = (path) => at(page, path).evaluate((el) => getComputedStyle(el).color);
  const letter = (path) => at(page, path).evaluate((el) => el.nextElementSibling?.textContent ?? '');
  const icon = (path) => at(page, path).evaluate((el) => el.closest('[role="treeitem"]').querySelector('img').dataset.icon);

  // Status do mock: README.md modificado; em docs há conflito (o mais forte), então a pasta fica vermelha com "•".
  // O status chega pelo polling do App, depois da árvore: espera a primeira letra aparecer.
  await page.waitForFunction(() => document.querySelector('[data-tree-path="README.md"]')?.nextElementSibling?.textContent === 'M');
  assert.equal(await color('README.md'), 'rgb(226, 192, 141)', 'modificado em amarelo');
  assert.equal(await letter('README.md'), 'M');
  assert.equal(await color('docs'), 'rgb(228, 103, 107)', 'pasta com conflito dentro herda o vermelho');
  assert.equal(await letter('docs'), '•');
  // `.gitignore` do mock: dist e *.log. Cinza e sem letra; o que não muda fica sem cor própria.
  assert.equal(await color('dist'), 'rgb(140, 140, 140)');
  assert.equal(await color('debug.log'), 'rgb(140, 140, 140)');
  assert.equal(await letter('debug.log'), '');
  assert.equal(await letter('package.json'), '');
  assert.notEqual(await color('package.json'), 'rgb(140, 140, 140)');
  // Ícones por nome de pasta e por extensão, como no Antigravity.
  assert.equal(await icon('docs'), 'folder-blue');
  assert.equal(await icon('src'), 'folder-orange-code');
  assert.equal(await icon('README.md'), 'markdown');
  assert.equal(await icon('package.json'), 'node');

  // Dentro de pasta aberta: o arquivo alterado tem a letra dele; a pasta ignorada passa o cinza aos filhos (o check-ignore os marca).
  await expand(page, ['docs', 'dist']);
  assert.equal(await letter('docs/rascunho-painel.md'), 'U');
  assert.equal(await color('dist/index.js'), 'rgb(140, 140, 140)');
  // Captura com a animação de abrir já terminada e o ponteiro na árvore (mostra as guias de recuo e as ações da barra).
  await at(page, 'docs/rascunho-painel.md').hover();
  await page.waitForTimeout(300);
  await page.screenshot({ path: join(OUT, 'files-search-explorer-visual.png') });

  // Recolher Tudo fecha as pastas abertas; a barra só mostra as ações com o ponteiro no painel.
  await page.getByRole('button', { name: 'Recolher Tudo', exact: true }).click();
  await at(page, 'docs/rascunho-painel.md').waitFor({ state: 'hidden' });
  assert.equal(await treeItem(page, 'docs').getAttribute('aria-expanded'), 'false');

  // Novo Arquivo da barra, com só a pasta docs selecionada (o clique simples também a abre): cria dentro dela.
  await treeItem(page, 'docs').click();
  assert.equal(await selected(page, 'docs'), true);
  await page.getByRole('button', { name: 'Novo Arquivo...', exact: true }).click();
  await page.getByRole('textbox', { name: 'Nome do novo arquivo' }).fill('pela-barra.md');
  await dialogBtn(page, 'Novo arquivo', 'Criar').click();
  await at(page, 'docs/pela-barra.md').waitFor();

  // A seção do projeto recolhe e reabre a árvore inteira.
  const section = page.getByRole('button', { name: 'exemplo', exact: true });
  await section.click();
  assert.equal(await section.getAttribute('aria-expanded'), 'false');
  await treeItem(page, 'docs').waitFor({ state: 'hidden' });
  await section.click();
  await treeItem(page, 'docs').waitFor();
});

process.exitCode = await run();
