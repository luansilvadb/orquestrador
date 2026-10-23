// Preparar e tirar do preparo um bloco do editor, calculando o texto novo do índice no frontend, como o Zed: o backend só grava o
// texto inteiro (`git_write_index`). Funções puras, sem DOM, para o check (tests/check-git-stage.mjs) e para o bloco aberto
// (git-hunk.ts). Textos com `\n`, como o editor. O bloco é o trecho buffer × HEAD do marcador, em posições do buffer (`fromB`/`toB`
// do `Chunk` do @codemirror/merge: `toB` é o começo da linha seguinte, ou o tamanho + 1 quando o trecho vai até o fim).
import { Chunk, type DiffConfig } from '@codemirror/merge';
import { Text } from '@codemirror/state';

export type Span = { fromB: number; toB: number };
type Pair = Pick<Chunk, 'fromA' | 'toA' | 'fromB' | 'toB'>;

// Mesmo padrão do MergeView: acima de 500 caracteres mudados num trecho, ou de 200 ms, o diff cai no modo rápido em vez de travar.
// Os marcadores (git-gutter.ts) usam a mesma configuração, para os trechos daqui baterem com os da tela.
export const DIFF: DiffConfig = { scanLimit: 500, timeout: 200 };

export const textOf = (text: string) => Text.of(text.split('\n'));

/** Os dois trechos do buffer se tocam? Um vazio (linhas apagadas) toca o vizinho encostado; dois com linhas, só se dividem alguma. */
export const touches = (a: Span, b: Span) => (a.fromB === a.toB || b.fromB === b.toB
  ? a.fromB <= b.toB && b.fromB <= a.toB
  : a.fromB < b.toB && b.fromB < a.toB);

/**
 * Troca, em `a`, o trecho A de cada chunk pelo trecho B de `b` (do último para o primeiro, então as posições continuam valendo). As
 * posições "tamanho + 1" do fim do documento caem no tamanho: a quebra virtual do fim não existe no texto.
 */
export function applyChunks(a: string, b: string, chunks: readonly Pair[]) {
  let out = a;
  for (let i = chunks.length - 1; i >= 0; i--) {
    const c = chunks[i];
    out = out.slice(0, c.fromA) + b.slice(c.fromB, Math.min(c.toB, b.length)) + out.slice(Math.min(c.toA, out.length));
  }
  return out;
}

/** Preparar: cada trecho buffer × índice que toca o bloco passa a ter, no índice, as linhas do buffer. */
export function stageHunk(index: string, buffer: string, hunk: Span): string {
  const chunks = Chunk.build(textOf(index), textOf(buffer), DIFF).filter((c) => touches(c, hunk));
  return applyChunks(index, buffer, chunks);
}

/**
 * Tirar do preparo: o bloco (já preparado, então igual no buffer e no índice) é levado para posições do índice pelos trechos buffer ×
 * índice de antes dele; cada trecho índice × HEAD que toca essa faixa volta às linhas do HEAD.
 */
export function unstageHunk(head: string, index: string, buffer: string, hunk: Span): string {
  const vsBuffer = Chunk.build(textOf(index), textOf(buffer), DIFF);
  const toIndex = (pos: number) => {
    let delta = 0;
    for (const c of vsBuffer) if (c.toB <= pos && c.fromB < pos) delta = c.toA - c.toB;
    return pos + delta;
  };
  const span = { fromB: toIndex(hunk.fromB), toB: toIndex(hunk.toB) };
  // HEAD × índice: A é o HEAD e B o índice, então a faixa é comparada no lado B e o texto volta do lado A.
  const chunks = Chunk.build(textOf(head), textOf(index), DIFF).filter((c) => touches(c, span));
  return applyChunks(index, head, chunks.map((c) => ({ fromA: c.fromB, toA: c.toB, fromB: c.fromA, toB: c.toA })));
}
