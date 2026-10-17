// O que vai ao terminal quando o clipboard traz arquivo ou imagem: a mesma regra no orquestrador e nos shells. Puro, para o check.

/** O CLI que roda no terminal; `null` é um shell comum. */
export type Cli = 'claude' | 'codex' | null;

/** `write` vai direto ao PTY; `paste` segue pelo paste do xterm; `error` é o que não dá para colar, dito ao usuário. */
export type Paste = { write: string } | { paste: string } | { error: string };

/**
 * O que o evento de colar traz quando há arquivo ou imagem: `null` com só texto, que segue pelo paste do xterm (e só por ele, sem
 * repetir). `image` é imagem solta ou arquivo de imagem; `text` vai junto para quando não há caminho nem CLI.
 */
export function clipboardFiles(data: Pick<DataTransfer, 'items' | 'getData'> | null): { image: boolean; text: string } | null {
  const files = Array.from(data?.items ?? []).filter((item) => item.kind === 'file');
  if (!files.length) return null;
  return { image: files.some((item) => item.type.startsWith('image/')), text: data?.getData('text') ?? '' };
}

const bracketed = (text: string) => `\x1b[200~${text}\x1b[201~`;
/** F12: o Codex não usa, então não mexe no rascunho nem no cursor; mas é uma tecla, e fecha o paste anterior antes do próximo. */
const F12 = '\x1b[24~';

/**
 * Medido no Claude Code 2.1.302 e no Codex 0.163.0, num ConPTY. Caminho de imagem colado vira anexo nativo (`[Image #N]`) nos dois:
 * o Claude anexa todas as de um paste só, um caminho por linha (`\n`; com `\r`, que é o que o paste do xterm manda, não), e põe o resto
 * como texto depois delas; o Codex só anexa quando o paste é um caminho só, e no Windows junta pastes seguidos num só, a não ser que
 * uma tecla os separe. Cada caminho leva um espaço no fim: os dois o aparam ao reconhecer a imagem, e no caminho que fica como texto
 * ele o separa do que já estava depois do cursor. Imagem solta (captura de tela): o gesto de colar imagem do CLI, que lê o clipboard
 * do Windows. Nada manda Enter.
 * `paths` são os arquivos copiados no Explorer, na ordem; `image` e `text` são o que o evento de colar trouxe.
 */
export function clipboardPaste(cli: Cli, paths: string[], image: boolean, text: string, bracketedMode: boolean): Paste {
  if (paths.length) {
    const quoted = paths.map((p) => (p.includes(' ') ? `"${p}"` : p));
    if (!cli || !bracketedMode) return { paste: quoted.join(' ') };
    if (cli === 'claude') return { write: bracketed(quoted.map((p) => `${p} `).join('\n')) };
    return { write: quoted.map((p) => bracketed(`${p} `)).join(F12) };
  }
  if (image && cli) return { write: cli === 'codex' ? '\x16' : '\x1bv' };
  if (text) return { paste: text };
  return { error: image ? 'A imagem não foi colada: este terminal só recebe texto. Abra o Claude Code ou o Codex nele para anexar imagens.' : 'Não há nada para colar no terminal.' };
}
