// Área das Issues da coluna central: aparece no lugar do editor só com a rota Issues ativa (o App a mantém montada e escondida nas outras,
// como as faixas da esquerda). Tem a faixa de abas no visual da do editor: "Issues" (fixa, o detalhe da issue escolhida na faixa) e, quando
// aberta, "Nova issue" (fecha com a confirmação de descarte se tiver texto). O rascunho da Nova issue e a rolagem do detalhe moram aqui e
// sobrevivem a trocar de rota; a issue escolhida mora no store (`selected`). Nada disto vai para a recuperação do editor.
// O detalhe não tem estado do App além do store: os executores vivos, o foco no orquestrador e o "Rodar no Terminal" (a indisponibilidade
// do GitHub é resolvida no detalhe) chegam por props e descem direto para ele.
//
// API para o App (ref, `IssuesAreaHandle`):
//   openNewIssue(seed?)  Abre ou ativa a aba Nova issue. Com `seed` (vindo da faixa), só preenche se o formulário está vazio; senão avisa por toast.
//   showDetail(focus?)   Ativa a aba Issues (o detalhe). Com `focus`, o foco vai para o detalhe; sem ele, fica onde estava (a faixa).
//   dirty(): boolean     A Nova issue tem texto (o App conta isso ao fechar a janela).
//   discard(): void      Joga o rascunho fora e fecha a aba Nova issue.
import { forwardRef, useImperativeHandle, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { ActionIcon, Button, Modal, Text, toast } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { CircleDot, X } from 'lucide-react';
import { FOCUS_RING_INSET, SECONDARY, type StartIssue } from './issue-ui';
import { IssuesPage } from './issues-page';
import { draftFilled, EMPTY_DRAFT, NewIssueForm, type IssueDraft } from './new-issue';
import type { ExecutorNode } from './types';

export type IssuesAreaHandle = {
  openNewIssue(seed?: IssueDraft): void;
  showDetail(focus?: boolean): void;
  dirty(): boolean;
  discard(): void;
};

type View = 'detail' | 'new';

export type IssuesAreaProps = {
  start: StartIssue;
  /** Executores vivos do App: o painel "Com o orquestrador" do detalhe os resume. */
  executors: readonly ExecutorNode[];
  /** Leva o foco ao terminal do orquestrador (o "Ir ao orquestrador" do detalhe). */
  onFocusOrchestrator: () => void;
  /** Abre o painel Terminal, cola o comando no shell ativo sem Enter e leva o foco a ele (o "Rodar no Terminal" da indisponibilidade). */
  onRunInTerminal: (command: string) => void;
};

export const IssuesArea = forwardRef<IssuesAreaHandle, IssuesAreaProps>(function IssuesArea({ start, executors, onFocusOrchestrator, onRunInTerminal }, handle) {
  const [view, setView] = useState<View>('detail');
  const [newOpen, setNewOpen] = useState(false);
  // Texto da Nova issue: fica aqui, e não no formulário, para o formulário poder remontar sem perder nada.
  const [draft, setDraftState] = useState<IssueDraft>(EMPTY_DRAFT);
  const draftRef = useRef(draft);
  const newOpenRef = useRef(false);
  // Sobe a cada abertura da Nova issue: o título recebe o foco. E a cada pedido de foco no detalhe.
  const [issueFocus, setIssueFocus] = useState(0);
  const [detailFocus, setDetailFocus] = useState(0);
  const [confirm, setConfirm] = useState<((discard: boolean) => void) | null>(null);
  // Descartada pela pergunta: o foco vai ao detalhe só depois de a janela fechar (antes, ela o devolveria à aba que já saiu).
  const focusAfterConfirm = useRef(false);
  const tablist = useRef<HTMLDivElement>(null);

  const setDraft = (next: IssueDraft) => {
    draftRef.current = next;
    setDraftState(next);
  };
  const setOpen = (open: boolean) => {
    newOpenRef.current = open;
    setNewOpen(open);
  };

  const openNewIssue = (seed?: IssueDraft) => {
    if (seed) {
      if (newOpenRef.current && draftFilled(draftRef.current)) toast.info({ title: 'Já há uma issue em edição.', duration: 4000 });
      else setDraft({ title: seed.title, body: seed.body, labels: [...seed.labels] });
    } else if (!newOpenRef.current) {
      setDraft(EMPTY_DRAFT);
    }
    setOpen(true);
    setView('new');
    setIssueFocus((n) => n + 1);
  };
  const showDetail = (focus = false) => {
    setView('detail');
    if (focus) setDetailFocus((n) => n + 1);
  };
  // Criada: o formulário fecha sem perguntar (o texto já foi para o store) e o detalhe ganha o foco. A issue criada passa a ser a mostrada
  // quando o GitHub devolve o número (`NewIssueForm`); até lá, o detalhe segue com a de antes.
  const created = () => {
    setDraft(EMPTY_DRAFT);
    setOpen(false);
    showDetail(true);
  };
  const finishClose = () => {
    setDraft(EMPTY_DRAFT);
    setOpen(false);
    setView('detail');
  };
  // Fechar a Nova issue: com texto, pergunta antes (não há o que salvar: continuar editando ou descartar). Fechada, a aba Issues fica ativa
  // com o foco no detalhe (a aba e o campo que tinham o foco saem da tela).
  const closeNew = () => {
    if (!newOpenRef.current) return;
    if (!draftFilled(draftRef.current)) {
      finishClose();
      showDetail(true);
      return;
    }
    if (confirm) return;
    setConfirm(() => (discard: boolean) => {
      setConfirm(null);
      if (!discard) return;
      finishClose();
      focusAfterConfirm.current = true;
    });
  };

  useImperativeHandle(handle, () => ({
    openNewIssue,
    showDetail,
    dirty: () => newOpenRef.current && draftFilled(draftRef.current),
    discard: finishClose,
  }));

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'F4' && e.ctrlKey && view === 'new') {
      e.preventDefault();
      closeNew();
    }
  };
  // Setas, Home e End na faixa de abas: o foco anda entre as abas (← e → dão a volta, como nas abas do painel do Orquestrador) e a aba focada
  // fica ativa. A parada de Tab é uma só, na aba ativa.
  const onTabsKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || (e.target as HTMLElement).getAttribute('role') !== 'tab') return;
    const keys: View[] = newOpen ? ['detail', 'new'] : ['detail'];
    const at = keys.indexOf(view);
    const to = e.key === 'ArrowRight' ? (at + 1) % keys.length
      : e.key === 'ArrowLeft' ? (at - 1 + keys.length) % keys.length
        : e.key === 'Home' ? 0 : e.key === 'End' ? keys.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    if (keys[to] === 'detail') showDetail();
    else setView('new');
    tablist.current?.querySelector<HTMLElement>(`[data-tab="${keys[to]}"] [role="tab"]`)?.focus();
  };

  const tab = (key: View, title: string, closable: boolean): ReactNode => {
    const active = view === key;
    const dirty = key === 'new' && draftFilled(draft);
    return (
      <div
        data-tab={key}
        className={`group/tab relative z-[1] flex h-full max-w-[min(14rem,100cqw)] flex-none items-center border-r border-border ${active ? 'bg-[var(--ant-color-bg-container)]' : 'hover:bg-[var(--ant-color-bg-container)]'}`}
      >
        {active && <span aria-hidden className="pointer-events-none absolute inset-x-0 -bottom-px h-px bg-[var(--ant-color-bg-container)]" />}
        {active && <span aria-hidden className="pointer-events-none absolute inset-x-0 top-0 h-px" style={{ background: 'var(--ant-color-primary)' }} />}
        <button
          type="button" role="tab" aria-selected={active} tabIndex={active ? 0 : -1} title={title}
          onClick={() => (key === 'detail' ? showDetail() : setView('new'))}
          className={`flex h-full min-w-0 items-center gap-1.5 pl-2.5 text-left text-[13px] outline-none ${FOCUS_RING_INSET} ${closable ? 'pr-7' : 'pr-3'} ${active ? 'text-[var(--ant-color-text)]' : 'text-[var(--ant-color-text-secondary)]'}`}
        >
          <CircleDot className="size-4 shrink-0 text-[var(--ant-color-text-secondary)]" aria-hidden />
          <span className="min-w-0 truncate">{title}</span>
        </button>
        {closable && dirty && (
          <span
            aria-label="Alteração não salva" title="Alteração não salva"
            className="pointer-events-none absolute top-1/2 right-1 flex size-5 -translate-y-1/2 items-center justify-center text-[13px] group-hover/tab:opacity-0"
          >
            ●
          </span>
        )}
        {closable && (
          <ActionIcon
            variant="borderless" icon={X} size={{ size: 16, blockSize: 20, borderRadius: 5 }}
            aria-label={`Fechar ${title}`} title="Fechar (Ctrl+F4)"
            className={`absolute! top-1/2 right-1 z-10 -translate-y-1/2! focus-visible:opacity-100 group-hover/tab:opacity-100 ${active && !dirty ? 'opacity-100' : 'opacity-0'}`}
            onClick={closeNew}
          />
        )}
      </div>
    );
  };

  return (
    <Flexbox height="100%" className="min-h-0" onKeyDown={onKey}>
      <div className="relative flex h-[30px] flex-none bg-[var(--ant-color-bg-layout)]">
        <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-border" />
        <div
          ref={tablist} role="tablist" aria-label="Abas das Issues" onKeyDown={onTabsKey}
          className="@container relative flex min-w-0 flex-1 items-stretch overflow-x-auto [scrollbar-width:none]!"
        >
          {tab('detail', 'Issues', false)}
          {newOpen && tab('new', 'Nova issue', true)}
        </div>
      </div>
      <Flexbox flex={1} className="min-h-0 relative">
        <div className="absolute inset-0 flex flex-col min-h-0" style={{ display: view === 'detail' ? 'flex' : 'none' }} data-body="detail">
          <IssuesPage
            start={start} onNew={() => openNewIssue()} focusKey={detailFocus}
            executors={executors} onFocusOrchestrator={onFocusOrchestrator} onRunInTerminal={onRunInTerminal}
          />
        </div>
        {newOpen && (
          <div className="absolute inset-0 flex flex-col min-h-0" style={{ display: view === 'new' ? 'flex' : 'none' }} data-body="new">
            <NewIssueForm draft={draft} onChange={setDraft} focusKey={issueFocus} onCreated={created} onClose={closeNew} />
          </div>
        )}
      </Flexbox>
      <Modal
        open={confirm !== null}
        title="Descartar esta issue?"
        width={480}
        closable={false}
        keyboard
        onCancel={() => confirm?.(false)}
        afterClose={() => {
          if (!focusAfterConfirm.current) return;
          focusAfterConfirm.current = false;
          showDetail(true);
        }}
        footer={(
          <Flexbox horizontal wrap="wrap" justify="flex-end" gap={8} width="100%">
            <Button onClick={() => confirm?.(false)}>Continuar editando</Button>
            <Button danger onClick={() => confirm?.(true)}>Descartar</Button>
          </Flexbox>
        )}
      >
        <Text as="div" fontSize={14} lineHeight={1.6} style={{ ...SECONDARY, margin: 0 }}>O título e a descrição escritos nesta aba serão perdidos.</Text>
      </Modal>
    </Flexbox>
  );
});
