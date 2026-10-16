// Estado das Issues do GitHub do projeto aberto, para a faixa lateral e a página de Issues. O App instancia o store uma vez
// (`useIssuesStore`) e o passa pelo `IssuesProvider`; as telas leem com `useIssues()`.
// O GitHub é a única fonte das issues. Localmente só ficam, no localStorage, as criações que ainda não chegaram lá (`pending`), a issue em
// andamento (`active`, a que está com o orquestrador) e a próxima (`next`, colada sozinha quando o orquestrador termina a rodada).
// Depois de cada atualização boa, a em andamento e a próxima que não estão mais abertas saem.
// Atualiza a cada 15 s com a janela em foco e a cada 60 s sem foco, e na hora quando a janela ganha o foco; uma atualização por vez.
import { createContext, createElement, useContext, useEffect, useMemo, useSyncExternalStore, type ReactNode } from 'react';
import { backend, message } from './backend';
import {
  markNext, pruneTracked, readStoredIssues, settleStored, startTracked, storedEmpty, type PendingIssue, type StoredIssues, type Tracked,
} from './issue-text';
import type { GhStatus, Issue, Label } from './types';

export type { ActiveIssue, PendingIssue } from './issue-text';

export type IssuesState = {
  // null = carregando a primeira vez.
  status: GhStatus | null;
  issues: Issue[];
  labels: Label[];
  pending: PendingIssue[];
  // Date.now() da última resposta boa do GitHub (inclusive "nada mudou").
  syncedAt: number | null;
  refreshing: boolean;
  // Erro da última atualização; a lista que já estava continua.
  error: string | null;
} & Tracked;
export type NewIssue = { title: string; body: string; labels: string[] };
export type IssuesStore = IssuesState & {
  refresh(): Promise<void>;
  // Otimista: entra em `pending` na hora e sai quando o GitHub devolve a issue.
  create(input: NewIssue): void;
  retry(localId: string): void;
  discard(localId: string): void;
  // Otimista: o estado muda na hora; volta e rejeita com a mensagem se o GitHub recusar.
  setState(number: number, open: boolean): Promise<void>;
  // A issue com o orquestrador (null: nenhuma). `since` recomeça só quando muda de issue; a anterior volta à fila, nada é fechado.
  setActive(number: number | null): void;
  // A próxima (null: nenhuma). A em andamento não pode ser a próxima.
  setNext(number: number | null): void;
  // Concluir a em andamento: sai do andamento na hora e fecha no GitHub; se o GitHub recusar, volta ao andamento e rejeita com a mensagem.
  finish(number: number): Promise<void>;
  // O estado de agora, sem esperar a tela (depois de um `await refresh()`, por exemplo).
  latest(): IssuesState;
};

const FOCUSED_MS = 15_000, BLURRED_MS = 60_000;
const storageKey = (project: string) => `orquestrador.issues:${project}`;

// Criações com o pedido a caminho nesta execução do app (por `localId`), de qualquer projeto. Uma sessão nova do mesmo projeto
// (trocou e voltou no meio do envio) as lê como `sending`, não como interrompidas, e não as reenvia.
const inFlight = new Set<string>();
// Sessões ligadas, para a resposta de uma criação chegar à sessão do projeto em que ela começou, mesmo que seja outra.
type Settled = { issue: Issue } | { error: string };
const liveSessions = new Set<{ project: string; settled: (localId: string, outcome: Settled) => void }>();

function loadStored(project: string): StoredIssues {
  try {
    return readStoredIssues(localStorage.getItem(storageKey(project)), inFlight);
  } catch {
    return { v: 1, pending: [], active: null, next: null };
  }
}

function writeStored(key: string, stored: StoredIssues) {
  if (storedEmpty(stored)) localStorage.removeItem(key);
  else localStorage.setItem(key, JSON.stringify(stored));
}

// A resposta de uma criação vale para o projeto em que ela começou: grava no localStorage dele (tira a pendência quando deu certo)
// e avisa a sessão ligada desse projeto, se houver. Assim trocar de projeto no meio do envio não faz a criação voltar como falha.
function settle(project: string, localId: string, outcome: Settled) {
  inFlight.delete(localId);
  try {
    const key = storageKey(project);
    writeStored(key, settleStored(localStorage.getItem(key), localId, 'error' in outcome ? outcome.error : undefined));
  } catch {
    // Sem localStorage: a sessão ligada ainda recebe a resposta abaixo.
  }
  for (const session of liveSessions) if (session.project === project) session.settled(localId, outcome);
}

// Guarda as criações que não chegaram ao GitHub (`sending` também: se o app fechar antes da resposta, voltam como `failed`), a em andamento e a próxima.
function save(project: string, { pending, active, next }: IssuesState) {
  try {
    writeStored(storageKey(project), { v: 1, pending, active, next });
  } catch {
    // Sem localStorage (cheio ou bloqueado): só não sobrevive a um recarregamento.
  }
}

// Uma sessão por projeto: trocar de projeto descarta a sessão e começa outra do zero.
function createSession(project: string) {
  const stored = loadStored(project);
  let state: IssuesState = {
    status: null, issues: [], labels: [], pending: stored.pending, active: stored.active, next: stored.next, syncedAt: null, refreshing: false, error: null,
  };
  const listeners = new Set<() => void>();
  // `version` é a última recebida (0 = nenhuma). `recheck` pede o status de novo antes das issues (depois de um erro).
  let version = 0, recheck = true, started = false, focused = document.hasFocus();
  // Conta as mudanças concluídas (criação, abrir/fechar): uma lista pedida antes delas chega velha e é ignorada; a atualização seguinte traz tudo.
  let mutations = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let running: Promise<void> | null = null, queued: Promise<void> | null = null;
  // Abrir/fechar a caminho: a lista que chegar no meio mantém o estado novo.
  const toggling = new Map<number, Issue['state']>();

  const set = (patch: Partial<IssuesState>) => {
    state = { ...state, ...patch };
    listeners.forEach((l) => l());
  };
  const setPending = (pending: PendingIssue[]) => {
    set({ pending });
    save(project, state);
  };
  const setTracked = (tracked: Tracked) => {
    if (tracked.active === state.active && tracked.next === state.next) return;
    set({ active: tracked.active, next: tracked.next });
    save(project, state);
  };
  // A issue devolvida pelo GitHub entra na hora: no lugar da que já estava, ou no começo.
  const upsert = (issue: Issue) => {
    const at = state.issues.findIndex((i) => i.number === issue.number);
    set({ issues: at < 0 ? [issue, ...state.issues] : state.issues.map((i, k) => (k === at ? issue : i)) });
  };
  const withState = (number: number, s: Issue['state']) => state.issues.map((i) => (i.number === number ? { ...i, state: s } : i));

  // Sem status, pronto ou offline, segue no ritmo; os outros indisponíveis só no `refresh()` manual e no foco da janela.
  const automatic = () => !state.status || state.status.kind === 'ready' || state.status.reason === 'offline';
  const schedule = () => {
    clearTimeout(timer);
    if (started && automatic()) timer = setTimeout(() => void refresh(), focused ? FOCUSED_MS : BLURRED_MS);
  };

  async function update() {
    set({ refreshing: true });
    const seen = mutations;
    try {
      if (recheck || state.status?.kind !== 'ready') {
        const status = await backend.githubStatus();
        set({ status });
        if (status.kind !== 'ready') {
          set({ error: null });
          return;
        }
        recheck = false;
      }
      const snapshot = await backend.githubIssues(version);
      if (mutations !== seen) return;
      if (snapshot.data) {
        const issues = snapshot.data.issues.map((i) => (toggling.has(i.number) ? { ...i, state: toggling.get(i.number)! } : i));
        set({ issues, labels: snapshot.data.labels });
      }
      setTracked(pruneTracked(state, state.issues));
      version = snapshot.version;
      set({ syncedAt: Date.now(), error: null });
      // Reenvio automático das que falharam: uma vez por atualização boa.
      if (started) for (const p of state.pending) if (p.status === 'failed') send(p.localId);
    } catch (e) {
      recheck = true;
      set({ error: message(e) });
    } finally {
      set({ refreshing: false });
      schedule();
    }
  }

  // Nunca duas ao mesmo tempo: a pedida durante outra roda logo depois, uma vez só (quem pediu espera essa).
  function refresh(): Promise<void> {
    if (!running) {
      running = update().finally(() => {
        running = null;
      });
      return running;
    }
    queued ??= running.then(() => {
      queued = null;
      return refresh();
    });
    return queued;
  }

  function send(localId: string) {
    const p = state.pending.find((x) => x.localId === localId);
    if (!p) return;
    const { error: _, ...rest } = p;
    setPending(state.pending.map((x) => (x.localId === localId ? { ...rest, status: 'sending' as const } : x)));
    inFlight.add(localId);
    backend.githubCreateIssue(p.title, p.body, p.labels).then(
      (issue) => settle(project, localId, { issue }),
      (e) => settle(project, localId, { error: message(e) }),
    );
  }

  // Resposta de uma criação deste projeto (desta sessão ou de uma anterior do mesmo projeto). O localStorage já foi gravado por `settle`.
  const hooks = {
    project,
    settled(localId: string, outcome: Settled) {
      if ('issue' in outcome) {
        mutations++;
        setPending(state.pending.filter((x) => x.localId !== localId));
        upsert(outcome.issue);
        void refresh();
      } else {
        // Descartada no meio do envio: não volta.
        setPending(state.pending.map((x) => (x.localId === localId ? { ...x, status: 'failed' as const, error: outcome.error } : x)));
      }
    },
  };

  async function setIssueState(number: number, open: boolean) {
    const before = state.issues.find((i) => i.number === number)?.state;
    const next = open ? 'open' : 'closed';
    toggling.set(number, next);
    set({ issues: withState(number, next) });
    try {
      const issue = await backend.githubSetIssueState(number, open);
      toggling.delete(number);
      mutations++;
      upsert(issue);
      if (started) void refresh();
    } catch (e) {
      toggling.delete(number);
      if (before) set({ issues: withState(number, before) });
      throw e;
    }
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

  const onFocus = () => {
    focused = true;
    void refresh();
  };
  const onBlur = () => {
    focused = false;
    if (!running) schedule();
  };

  return {
    get: () => state,
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    start() {
      started = true;
      liveSessions.add(hooks);
      focused = document.hasFocus();
      window.addEventListener('focus', onFocus);
      window.addEventListener('blur', onBlur);
      void refresh();
    },
    stop() {
      started = false;
      liveSessions.delete(hooks);
      clearTimeout(timer);
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('blur', onBlur);
    },
    actions: {
      refresh,
      create: (input: NewIssue) => {
        const p: PendingIssue = {
          localId: crypto.randomUUID(), title: input.title, body: input.body, labels: [...input.labels], createdAt: Date.now(), status: 'sending',
        };
        setPending([...state.pending, p]);
        send(p.localId);
      },
      retry: (localId: string) => {
        if (state.pending.some((x) => x.localId === localId && x.status === 'failed')) send(localId);
      },
      discard: (localId: string) => setPending(state.pending.filter((x) => x.localId !== localId)),
      setState: setIssueState,
      setActive: (number: number | null) => setTracked(number === null ? { active: null, next: state.next } : startTracked(state, number, Date.now())),
      setNext: (number: number | null) => setTracked(markNext(state, number)),
      finish,
      latest: () => state,
    },
  };
}

/** O store das Issues de `project`. Trocar de projeto zera tudo e recomeça; com `project` vazio não atualiza. */
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

/** Entrega o store criado pelo App à faixa lateral e à página de Issues. */
export function IssuesProvider({ value, children }: { value: IssuesStore; children?: ReactNode }) {
  return createElement(IssuesContext.Provider, { value }, children);
}

export function useIssues(): IssuesStore {
  const store = useContext(IssuesContext);
  if (!store) throw new Error('useIssues precisa do IssuesProvider.');
  return store;
}
