// Estado das Issues do projeto aberto, para a faixa lateral e o detalhe da issue. O App instancia o store uma vez
// (`useIssuesStore`) e o passa pelo `IssuesProvider`; as telas leem com `useIssues()`.
// As issues ficam só neste computador, num arquivo por projeto que o backend lê e grava (`issues.rs`), junto com a issue em andamento
// (`active`, a que está com o orquestrador) e a próxima (`next`, colada sozinha quando o orquestrador termina a rodada). O rascunho do
// orquestrador (`draft`, as issues coladas nele e ainda não enviadas) fica só em memória: morre com o processo do orquestrador.
// O arquivo é lido ao abrir o projeto e relido quando a janela ganha o foco (outra janela do app pode ter mudado algo), no fim de cada
// rodada do orquestrador e depois de cada mudança. A em andamento e a próxima vêm do arquivo só na primeira leitura; depois quem decide é
// esta janela, que as grava a cada mudança. Cada mudança de issue grava na hora e devolve a issue como ficou.
// Na abertura, as criações que a versão com o GitHub guardou no localStorage do projeto, e nunca chegaram lá, viram issues abertas.
import { createContext, createElement, useContext, useEffect, useMemo, useSyncExternalStore, type ReactNode } from 'react';
import { backend, message } from './backend';
import {
  detailIssue, draftAdd, draftOnTurn, legacyIssues, markNext, markTurned, pruneTracked, startTracked, type Draft, type Tracked,
} from './issue-text';
import type { ActiveIssue, Issue } from './types';

export type IssuesState = {
  // Falso até a primeira leitura do arquivo dar certo.
  loaded: boolean;
  issues: Issue[];
  // Erro da última leitura ou gravação; a lista que já estava continua.
  error: string | null;
  // A issue escolhida na faixa, que o detalhe mostra (null: ninguém escolheu; vale `detailIssue`). Só em memória, por projeto.
  selected: number | null;
  // As issues coladas no rascunho do orquestrador desde o último envio, na ordem da colagem. Só em memória, por projeto.
  draft: Draft;
} & Tracked;
export type NewIssue = { title: string; body: string; labels: string[] };
export type IssuesStore = IssuesState & {
  // Relê o arquivo. Só a leitura pedida por último vale.
  refresh(): Promise<void>;
  // Grava a issue nova e a devolve, com o número. Rejeita com a mensagem se o backend recusar.
  create(input: NewIssue): Promise<Issue>;
  // Troca o título, a descrição e as labels.
  update(number: number, input: NewIssue): Promise<Issue>;
  // Apaga de vez: sai da escolha, e a em andamento e a próxima que eram ela perdem a marca.
  remove(number: number): Promise<void>;
  // Escolhe a issue que o detalhe mostra (null volta à regra sem escolha). Não persiste.
  select(number: number | null): void;
  // Fecha ou reabre; rejeita com a mensagem se o backend recusar.
  setState(number: number, open: boolean): Promise<void>;
  // A issue com o orquestrador (null: nenhuma). `since` recomeça só quando muda de issue; a anterior volta à fila, nada é fechado.
  setActive(number: number | null): void;
  // A próxima (null: nenhuma). A em andamento não pode ser a próxima.
  setNext(number: number | null): void;
  // Uma rodada do orquestrador começou: a em andamento deixa de ser só "colada, aguardando envio".
  markTurn(): void;
  // Colou `number` no rascunho do orquestrador: `join` (Colar junto) a põe depois das que estavam; senão fica só ela.
  pasted(number: number, join: boolean): void;
  // Uma rodada começou com `prompt` (`draftOnTurn`): o envio esvazia o rascunho; `null` (a leitura inicial) não.
  sent(prompt: string | null | undefined): void;
  // O orquestrador abriu ou terminou: o rascunho está vazio.
  clearDraft(): void;
  // Desfaz o Concluir: reabre `number` e a devolve a Em andamento como estava (`prev`, com o `since` e o `turned`). Rejeita com a mensagem se der errado.
  undoFinish(prev: ActiveIssue): Promise<void>;
  // Concluir a em andamento: sai do andamento na hora e é fechada; se der errado, volta ao andamento e rejeita com a mensagem.
  finish(number: number): Promise<void>;
  // O estado de agora, sem esperar a tela (depois de um `await refresh()`, por exemplo).
  latest(): IssuesState;
};

// A chave da versão com o GitHub, por projeto: as criações que não chegaram lá, a em andamento e a próxima.
const legacyKey = (project: string) => `orquestrador.issues:${project}`;

// Uma sessão por projeto: trocar de projeto descarta a sessão e começa outra do zero.
function createSession(project: string) {
  let state: IssuesState = { loaded: false, issues: [], active: null, next: null, error: null, selected: null, draft: [] };
  const listeners = new Set<() => void>();
  // `loads` conta as leituras pedidas: a resposta de uma que não é a última chega velha e é ignorada.
  let loads = 0, started = false;
  // As gravações da em andamento e da próxima, em fila: cada uma grava o estado de quando chega a vez dela, e a última vence.
  let tracking: Promise<void> = Promise.resolve();

  const set = (patch: Partial<IssuesState>) => {
    state = { ...state, ...patch };
    listeners.forEach((l) => l());
  };
  const setDraft = (draft: Draft) => {
    if (draft !== state.draft) set({ draft });
  };
  const setTracked = (tracked: Tracked) => {
    if (tracked.active === state.active && tracked.next === state.next) return;
    set({ active: tracked.active, next: tracked.next });
    // Depois do `stop` o backend já pode estar em outro projeto: o que ainda estava na fila não grava.
    tracking = tracking.then(() => (started ? backend.issuesSetTracked(state.active, state.next) : undefined)).catch((e) => set({ error: message(e) }));
  };
  // A issue devolvida pelo backend entra na hora: no lugar da que já estava, ou no fim.
  const upsert = (issue: Issue) => {
    const at = state.issues.findIndex((i) => i.number === issue.number);
    set({ issues: at < 0 ? [...state.issues, issue] : state.issues.map((i, k) => (k === at ? issue : i)), error: null });
    setTracked(pruneTracked(state, state.issues));
  };

  async function refresh() {
    const ticket = ++loads;
    try {
      const data = await backend.issuesLoad();
      if (ticket !== loads) return;
      // A em andamento e a próxima vêm do arquivo só na primeira leitura; depois, só perdem a marca se a issue não está mais aberta.
      const first = !state.loaded;
      set({ loaded: true, issues: data.issues, error: null, ...(first ? { active: data.active, next: data.next } : {}) });
      setTracked(pruneTracked(state, state.issues));
    } catch (e) {
      if (ticket === loads) set({ error: message(e) });
    }
  }

  // As criações da versão com o GitHub viram issues, uma a uma, e saem da chave a cada uma gravada: se o app fechar no meio, só a que
  // estava sendo gravada pode voltar repetida. Sem localStorage, ou com o backend recusando, a chave fica para a próxima abertura.
  async function migrate() {
    try {
      const key = legacyKey(project);
      let rest = legacyIssues(localStorage.getItem(key));
      while (rest.length && started) {
        await backend.issuesCreate(rest[0].title, rest[0].body, rest[0].labels);
        rest = rest.slice(1);
        if (rest.length) localStorage.setItem(key, JSON.stringify({ v: 1, pending: rest }));
      }
      if (!rest.length) localStorage.removeItem(key);
    } catch (e) {
      set({ error: message(e) });
    }
  }

  async function setIssueState(number: number, open: boolean) {
    upsert(await backend.issuesSetState(number, open));
    if (started) void refresh();
  }

  async function finish(number: number) {
    const before = state.active;
    if (before?.number === number) setTracked({ active: null, next: state.next });
    try {
      await setIssueState(number, false);
    } catch (e) {
      // Volta com o `since` de antes, se nada tomou o lugar dela enquanto o pedido ia.
      if (before?.number === number && !state.active) setTracked({ active: before, next: state.next === number ? null : state.next });
      throw e;
    }
  }

  const onFocus = () => void refresh();

  return {
    get: () => state,
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    start() {
      started = true;
      window.addEventListener('focus', onFocus);
      void migrate().then(refresh);
    },
    stop() {
      started = false;
      window.removeEventListener('focus', onFocus);
    },
    actions: {
      refresh,
      create: async (input: NewIssue) => {
        const issue = await backend.issuesCreate(input.title, input.body, input.labels);
        upsert(issue);
        if (started) void refresh();
        return issue;
      },
      update: async (number: number, input: NewIssue) => {
        const issue = await backend.issuesUpdate(number, input.title, input.body, input.labels);
        upsert(issue);
        if (started) void refresh();
        return issue;
      },
      remove: async (number: number) => {
        await backend.issuesDelete(number);
        set({ issues: state.issues.filter((i) => i.number !== number), selected: state.selected === number ? null : state.selected, error: null });
        setTracked(pruneTracked(state, state.issues));
        if (started) void refresh();
      },
      select: (number: number | null) => {
        if (number !== state.selected) set({ selected: number });
      },
      setState: setIssueState,
      setActive: (number: number | null) => setTracked(number === null ? { active: null, next: state.next } : startTracked(state, number, Date.now())),
      setNext: (number: number | null) => setTracked(markNext(state, number)),
      markTurn: () => setTracked(markTurned(state)),
      pasted: (number: number, join: boolean) => setDraft(draftAdd(state.draft, number, join)),
      sent: (prompt: string | null | undefined) => setDraft(draftOnTurn(state.draft, prompt)),
      clearDraft: () => setDraft(state.draft.length ? [] : state.draft),
      undoFinish: async (prev: ActiveIssue) => {
        await setIssueState(prev.number, true);
        // Se nada tomou o lugar dela enquanto o pedido ia.
        if (!state.active) setTracked({ active: prev, next: state.next === prev.number ? null : state.next });
      },
      finish,
      latest: () => state,
    },
  };
}

/** O store das Issues de `project`. Trocar de projeto zera tudo e recomeça; com `project` vazio não lê nada. */
export function useIssuesStore(project: string): IssuesStore {
  const session = useMemo(() => createSession(project), [project]);
  useEffect(() => {
    if (!project) return;
    session.start();
    return session.stop;
  }, [session, project]);
  const state = useSyncExternalStore(session.subscribe, session.get);
  return useMemo(() => ({ ...state, ...session.actions }), [state, session]);
}

const IssuesContext = createContext<IssuesStore | null>(null);

/** Entrega o store criado pelo App à faixa lateral e à área das Issues. */
export function IssuesProvider({ value, children }: { value: IssuesStore; children?: ReactNode }) {
  return createElement(IssuesContext.Provider, { value }, children);
}

export function useIssues(): IssuesStore {
  const store = useContext(IssuesContext);
  if (!store) throw new Error('useIssues precisa do IssuesProvider.');
  return store;
}

/** A issue que o detalhe mostra, e a faixa marca como selecionada: a escolhida, ou a em andamento, a próxima, a primeira aberta (`detailIssue`). */
export function useDetailIssue(): Issue | null {
  const { issues, selected, active, next } = useIssues();
  return detailIssue(issues, selected, active?.number ?? null, next);
}
