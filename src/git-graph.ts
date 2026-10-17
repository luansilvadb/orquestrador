// Modelo puro do gráfico de commits (faixas e linhas), sem React. Cada linha de saída descreve um commit:
// onde ele fica, a cor da faixa e os segmentos que ligam a borda de cima ao meio e o meio à borda de baixo.
export type GraphLine = { from: number; to: number; half: 'top' | 'bottom'; color: number };
export type GraphRow = { col: number; color: number; lines: GraphLine[]; width: number };

// Faixa ocupada: o hash do commit que ela espera e a cor que ela carrega desde que nasceu.
type Lane = { hash: string; color: number } | null;

// Commits do mais novo para o mais antigo, filhos antes dos pais. Pais fora da lista seguem até a borda de baixo.
export function layoutGraph(commits: readonly { hash: string; parents: readonly string[] }[]): GraphRow[] {
  const lanes: Lane[] = [];
  let nextColor = 0;
  const freeLane = () => {
    const i = lanes.indexOf(null);
    return i >= 0 ? i : lanes.length;
  };

  return commits.map((commit) => {
    const expecting = lanes.flatMap((l, k) => (l?.hash === commit.hash ? [k] : []));

    // Sem filho conhecido, o commit entra na primeira faixa livre com cor nova; caso contrário, na primeira que o espera.
    const col = expecting.length ? expecting[0] : freeLane();
    const color = expecting.length ? lanes[col]!.color : nextColor++;

    const lines: GraphLine[] = [];
    // Faixas que esperavam este commit convergem no ponto (a primeira é a própria faixa do commit).
    for (const k of expecting) lines.push({ from: k, to: col, half: 'top', color: lanes[k]!.color });
    // Faixas que só passam reto por esta linha.
    lanes.forEach((lane, k) => {
      if (!lane || lane.hash === commit.hash) return;
      lines.push({ from: k, to: k, half: 'top', color: lane.color }, { from: k, to: k, half: 'bottom', color: lane.color });
    });
    for (const k of expecting) lanes[k] = null;

    commit.parents.forEach((parent, idx) => {
      // Primeiro pai continua na faixa do commit, com a mesma cor; se outro filho também o espera, a convergência acontece na linha do pai.
      if (idx === 0) {
        lanes[col] = { hash: parent, color };
        lines.push({ from: col, to: col, half: 'bottom', color });
        return;
      }
      // Pai extra (merge) que já tem faixa: a linha converge para ela e termina ali.
      const existing = lanes.findIndex((lane) => lane?.hash === parent);
      if (existing >= 0) {
        lines.push({ from: col, to: existing, half: 'bottom', color });
        return;
      }
      // Pai extra novo ganha uma faixa livre com cor nova.
      const j = freeLane();
      const newColor = nextColor++;
      lanes[j] = { hash: parent, color: newColor };
      lines.push({ from: col, to: j, half: 'bottom', color: newColor });
    });

    while (lanes.length && lanes[lanes.length - 1] === null) lanes.pop();

    const width = 1 + Math.max(col, ...lines.flatMap((l) => [l.from, l.to]));
    return { col, color, lines, width };
  });
}
