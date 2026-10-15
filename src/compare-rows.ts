// Modelo do comparador contínuo: o arquivo inteiro (patch com todo o contexto) vira linhas, em lado a lado e unificado.
// Cada linha de mudança aponta para a sua unidade: o bloco do Git (-U3), que é o que as ações agem, ou, sem blocos
// (commit), cada trecho de mudança. Não há nada visual aqui: quem desenha é o motor de diff.

/** Um lado de uma linha: número da linha no arquivo daquele lado e o texto (sem o sinal). */
export type CompareSide = { n: number; text: string };
/** Uma linha do lado a lado. `unit` é -1 no contexto. Numa linha de mudança, um lado pode estar vazio (padding). */
export type SplitRow = { left: CompareSide | null; right: CompareSide | null; change: boolean; unit: number };
/** Uma linha do unificado: `old`/`new` são os números de cada lado, `null` onde a linha não existe. */
export type UnifiedRow = { kind: 'ctx' | 'del' | 'add'; old: number | null; new: number | null; text: string; unit: number };
/** Uma unidade de navegação: a primeira linha do arquivo novo dela, as linhas (inclusive) em cada modo e o que ela muda. */
export type Unit = { line: number; split: [number, number]; unified: [number, number]; kind: 'add' | 'del' | 'mod' };
export type CompareModel = { split: SplitRow[]; unified: UnifiedRow[]; units: Unit[] };
/** O intervalo de um bloco no patch de blocos (-U3): início e tamanho de cada lado, como no cabeçalho `@@`. */
export type BlockRange = { oldAt: number; oldLen: number; newAt: number; newLen: number };

const HUNK = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/;

/** Os intervalos dos blocos de um patch, na ordem em que aparecem (a mesma numeração da revisão). */
export function blockRanges(patch: string): BlockRange[] {
  const out: BlockRange[] = [];
  for (const line of patch.split('\n')) {
    const h = HUNK.exec(line);
    if (h) out.push({ oldAt: Number(h[1]), oldLen: h[2] === undefined ? 1 : Number(h[2]), newAt: Number(h[3]), newLen: h[4] === undefined ? 1 : Number(h[4]) });
  }
  return out;
}

/**
 * As chaves das linhas de mudança de uma linha do modelo, como o motor as marca no `data-line`: `d` + número antigo (remoção) e
 * `a` + número novo (inclusão). Contexto não tem chave. Com elas, a posição real e o clique acham a linha sem alinhar nada.
 */
export function changeKeys(row: SplitRow | UnifiedRow): string[] {
  if (row.unit < 0) return [];
  if ('kind' in row) return [row.kind === 'add' ? `a${row.new}` : `d${row.old}`];
  return [...(row.change && row.left ? [`d${row.left.n}`] : []), ...(row.change && row.right ? [`a${row.right.n}`] : [])];
}

/**
 * O comparador de um patch com o arquivo inteiro. `ranges` são os blocos (-U3) para as unidades; sem eles, cada trecho
 * de mudança (linhas seguidas que não são contexto) é uma unidade.
 */
export function compareOf(full: string, ranges: BlockRange[] | null): CompareModel {
  type Line = { kind: 'ctx' | 'del' | 'add'; old: number | null; new: number | null; text: string; unit: number };
  const lines: Line[] = [];
  let o = 0, n = 0, inside = false;
  for (const raw of full.split('\n')) {
    const h = HUNK.exec(raw);
    if (h) { o = Number(h[1]); n = Number(h[3]); inside = true; continue; }
    // Antes do primeiro `@@` é o cabeçalho do arquivo; a última quebra de linha não tem linha.
    if (!inside || raw === '') continue;
    const sign = raw[0];
    const text = raw.slice(1);
    if (sign === ' ') lines.push({ kind: 'ctx', old: o++, new: n++, text, unit: -1 });
    else if (sign === '-') lines.push({ kind: 'del', old: o++, new: null, text, unit: -1 });
    else if (sign === '+') lines.push({ kind: 'add', old: null, new: n++, text, unit: -1 });
  }
  // A unidade de cada mudança: o bloco que contém a linha; sem blocos, o trecho.
  let runOf = -1, prev: Line['kind'] = 'ctx';
  for (const l of lines) {
    if (l.kind === 'ctx') { prev = 'ctx'; continue; }
    if (prev === 'ctx') runOf++;
    prev = l.kind;
    if (!ranges) { l.unit = runOf; continue; }
    const i = l.kind === 'del'
      ? ranges.findIndex((r) => r.oldLen > 0 && l.old! >= r.oldAt && l.old! < r.oldAt + r.oldLen)
      : ranges.findIndex((r) => r.newLen > 0 && l.new! >= r.newAt && l.new! < r.newAt + r.newLen);
    l.unit = i;
  }
  const unified: UnifiedRow[] = lines.map((l) => ({ ...l }));

  // Lado a lado: o contexto nos dois lados; cada trecho de mudança alinha as remoções com as inclusões, e sobra vazio.
  const split: SplitRow[] = [];
  for (let i = 0; i < lines.length;) {
    const l = lines[i];
    if (l.kind === 'ctx') {
      split.push({ left: { n: l.old!, text: l.text }, right: { n: l.new!, text: l.text }, change: false, unit: -1 });
      i++;
      continue;
    }
    let j = i;
    while (j < lines.length && lines[j].kind !== 'ctx') j++;
    const dels = lines.slice(i, j).filter((x) => x.kind === 'del');
    const adds = lines.slice(i, j).filter((x) => x.kind === 'add');
    for (let k = 0; k < Math.max(dels.length, adds.length); k++) {
      const d = dels[k], a = adds[k];
      split.push({
        left: d ? { n: d.old!, text: d.text } : null,
        right: a ? { n: a.new!, text: a.text } : null,
        change: true,
        unit: (d ?? a).unit,
      });
    }
    i = j;
  }

  // Unidades: a primeira linha do arquivo novo e a faixa de linhas em cada modo.
  const count = ranges ? ranges.length : runOf + 1;
  const units: Unit[] = [];
  // Primeira e última linha de uma unidade num modo; `[-1, -1]` quando a unidade não tem linha (não deveria acontecer).
  const span = <R extends { unit: number }>(rows: R[], u: number): [number, number] => {
    const first = rows.findIndex((r) => r.unit === u);
    if (first < 0) return [-1, -1];
    return [first, rows.length - 1 - [...rows].reverse().findIndex((r) => r.unit === u)];
  };
  for (let u = 0; u < count; u++) {
    let line: number | null = null, lastNew = 0;
    for (const r of unified) {
      if (r.new !== null) lastNew = r.new;
      if (r.unit === u && line === null) line = r.new ?? lastNew + 1;
    }
    const kinds = new Set(unified.filter((r) => r.unit === u).map((r) => r.kind));
    const kind = kinds.has("add") && kinds.has("del") ? "mod" : kinds.has("add") ? "add" : "del";
    units.push({ line: line ?? 1, split: span(split, u), unified: span(unified, u), kind });
  }
  return { split, unified, units };
}
