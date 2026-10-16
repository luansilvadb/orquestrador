// Área das Issues da coluna central: aparece no lugar do editor só com a rota Issues ativa (o App a mantém montada e escondida nas outras,
// como as faixas da esquerda). Tem a faixa de abas no visual da do editor: "Issues" (fixa, o detalhe da issue escolhida na faixa) e, quando
// aberta, a aba do formulário, uma só no app: "Nova issue" ou "Editar #N" (fecha com a confirmação de descarte se tiver alteração). O texto
// do formulário e a rolagem do detalhe moram aqui e sobrevivem a trocar de rota; a issue escolhida mora no store (`selected`). Nada disto
// vai para a recuperação do editor. Excluída a issue em edição, a aba fecha sozinha.
// O detalhe não tem estado do App além do store: os executores vivos e o foco no orquestrador chegam por props e descem direto para ele.
//
// API para o App (ref, `IssuesAreaHandle`):
//   openNewIssue()       Abre ou ativa a aba Nova issue (o Título ganha o foco). Com uma edição alterada, só ativa a aba e avisa.
//   openEdit(issue)      Abre a issue no formulário (a aba Editar #N). Com outra edição ou uma Nova issue com texto, só ativa a aba e avisa.
//   showDetail(focus?)   Ativa a aba Issues (o detalhe). Com `focus`, o foco vai para o detalhe; sem ele, fica onde estava (a faixa).
//   dirty()              O nome da aba do formulário, se ela tem alteração (o App conta isso ao fechar a janela); senão null.
//   discard(): void      Joga a alteração fora e fecha a aba do formulário.
import { forwardRef, useEffect, useImperativeHandle, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { ActionIcon, Button, Modal, Text, toast } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { CircleDot, X } from 'lucide-react';
import { FOCUS_RING_INSET, SECONDARY, type StartIssue } from './issue-ui';
import { useIssues } from './issues-store';
import { IssuesPage } from './issues-page';
import { draftFilled, EMPTY_DRAFT, NewIssueForm, sameDraft, type IssueDraft } from './new-issue';
import type { ExecutorNode, Issue } from './types';

export type IssuesAreaHandle = {
  openNewIssue(): void;
  openEdit(issue: Issue): void;
  showDetail(focus?: boolean): void;
  dirty(): string | null;
  discard(): void;
};

type View = 'detail' | 'form';
// O formulário: `editing` é o número da issue em edição (null na Nova issue) e `base` o texto dela quando a aba abriu.
type Form = { editing: number | null; draft: IssueDraft; base: IssueDraft };
const EMPTY_FORM: Form = { editing: null, draft: EMPTY_DRAFT, base: EMPTY_DRAFT };

export type IssuesAreaProps = {
  start: StartIssue;
  /** Executores vivos do App: o painel "Com o orquestrador" do detalhe os resume. */
  executors: readonly ExecutorNode[];
  /** Leva o foco ao terminal do orquestrador (o "Ir ao orquestrador" do detalhe). */
  onFocusOrchestrator: () => void;
};

const formTitle = (form: Form) => (form.editing === null ? 'Nova issue' : `Editar #${form.editing}`);
// A Nova issue tem alteração quando tem texto; a edição, quando o texto mudou desde que a aba abriu.
const formDirty = (form: Form) => (form.editing === null ? draftFilled(form.draft) : !sameDraft(form.draft, form.base));

export const IssuesArea = forwardRef<IssuesAreaHandle, IssuesAreaProps>(function IssuesArea({ start, executors, onFocusOrchestrator }, handle) {
  const store = useIssues();
  const [view, setView] = useState<View>('detail');
  const [formOpen, setFormOpen] = useState(false);
  // Texto do formulário: fica aqui, e não no formulário, para o formulário poder remontar sem perder nada.
  const [form, setFormState] = useState<Form>(EMPTY_FORM);
  const formRef = useRef(form);
  const formOpenRef = useRef(false);
  // Sobe a cada abertura do formulário: o título recebe o foco. E a cada pedido de foco no detalhe.
  const [formFocus, setFormFocus] = useState(0);
  const [detailFocus, setDetailFocus] = useState(0);
  const [confirm, setConfirm] = useState<((discard: boolean) => void) | null>(null);
  // Descartada pela pergunta: o foco vai ao detalhe só depois de a janela fechar (antes, ela o devolveria à aba que já saiu).
  const focusAfterConfirm = useRef(false);
  // A issue da pergunta (null na Nova issue): o texto da janela não muda enquanto ela fecha, com o formulário já vazio.
  const asked = useRef<number | null>(null);
  const tablist = useRef<HTMLDivElement>(null);

  const setForm = (next: Form) => {
    formRef.current = next;
    setFormState(next);
  };
  const setOpen = (open: boolean) => {
    formOpenRef.current = open;
    setFormOpen(open);
  };
  const activateForm = () => {
    setOpen(true);
    setView('form');
    setFormFocus((n) => n + 1);
  };
  const busy = () => formOpenRef.current && formDirty(formRef.current);

  const openNewIssue = () => {
    const { editing } = formRef.current;
    // Com alteração na edição, nada é trocado: só a aba aparece. Uma Nova issue já aberta fica com o texto dela.
    if (editing !== null && busy()) toast.info({ title: 'Já há uma issue em edição.', duration: 4000 });
    else if (editing !== null || !formOpenRef.current) setForm(EMPTY_FORM);
    activateForm();
  };
  const openEdit = (issue: Issue) => {
    if (!(formOpenRef.current && formRef.current.editing === issue.number)) {
      if (busy()) {
        toast.info({ title: 'Já há uma issue em edição.', duration: 4000 });
      } else {
        const draft = { title: issue.title, body: issue.body, labels: [...issue.labels] };
        setForm({ editing: issue.number, draft, base: draft });
      }
    }
    activateForm();
  };
  const showDetail = (focus = false) => {
    setView('detail');
    if (focus) setDetailFocus((n) => n + 1);
  };
  // Gravada: o formulário fecha sem perguntar e o detalhe ganha o foco, com a issue escolhida (o formulário já a escolheu).
  const saved = () => {
    setForm(EMPTY_FORM);
    setOpen(false);
    showDetail(true);
  };
  const finishClose = () => {
    setForm(EMPTY_FORM);
    setOpen(false);
    setView('detail');
  };
  // Fechar o formulário: com alteração, pergunta antes (não há o que salvar: continuar editando ou descartar). Fechado, a aba Issues fica
  // ativa com o foco no detalhe (a aba e o campo que tinham o foco saem da tela).
  const closeForm = () => {
    if (!formOpenRef.current) return;
    if (!busy()) {
      finishClose();
      showDetail(true);
      return;
    }
    if (confirm) return;
    asked.current = formRef.current.editing;
    setConfirm(() => (discard: boolean) => {
      setConfirm(null);
      if (!discard) return;
      finishClose();
      focusAfterConfirm.current = true;
    });
  };

  // A issue em edição foi excluída (aqui ou em outra janela): a aba não tem mais o que salvar.
  const gone = store.loaded && formOpen && form.editing !== null && !store.issues.some((i) => i.number === form.editing);
  useEffect(() => {
    if (gone) finishClose();
  }, [gone]);

  useImperativeHandle(handle, () => ({
    openNewIssue,
    openEdit,
    showDetail,
    dirty: () => (busy() ? formTitle(formRef.current) : null),
    discard: finishClose,
  }));

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'F4' && e.ctrlKey && view === 'form') {
      e.preventDefault();
      closeForm();
    }
  };
  // Setas, Home e End na faixa de abas: o foco anda entre as abas (← e → dão a volta, como nas abas do painel do Orquestrador) e a aba focada
  // fica ativa. A parada de Tab é uma só, na aba ativa.
  const onTabsKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || (e.target as HTMLElement).getAttribute('role') !== 'tab') return;
    const keys: View[] = formOpen ? ['detail', 'form'] : ['detail'];
    const at = keys.indexOf(view);
    const to = e.key === 'ArrowRight' ? (at + 1) % keys.length
      : e.key === 'ArrowLeft' ? (at - 1 + keys.length) % keys.length
        : e.key === 'Home' ? 0 : e.key === 'End' ? keys.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    if (keys[to] === 'detail') showDetail();
    else setView('form');
    tablist.current?.querySelector<HTMLElement>(`[data-tab="${keys[to]}"] [role="tab"]`)?.focus();
  };

  const tab = (key: View, title: string, closable: boolean): ReactNode => {
    const active = view === key;
    const dirty = key === 'form' && formDirty(form);
    return (
      <div
        data-tab={key}
        className={`group/tab relative z-[1] flex h-full max-w-[min(14rem,100cqw)] flex-none items-center border-r border-border ${active ? 'bg-[var(--ant-color-bg-container)]' : 'hover:bg-[var(--ant-color-bg-container)]'}`}
      >
        {active && <span aria-hidden className="pointer-events-none absolute inset-x-0 -bottom-px h-px bg-[var(--ant-color-bg-container)]" />}
        {active && <span aria-hidden className="pointer-events-none absolute inset-x-0 top-0 h-px" style={{ background: 'var(--ant-color-primary)' }} />}
        <button
          type="button" role="tab" aria-selected={active} tabIndex={active ? 0 : -1} title={title}
          onClick={() => (key === 'detail' ? showDetail() : setView('form'))}
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
            onClick={closeForm}
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
          {formOpen && tab('form', formTitle(form), true)}
        </div>
      </div>
      <Flexbox flex={1} className="min-h-0 relative">
        <div className="absolute inset-0 flex flex-col min-h-0" style={{ display: view === 'detail' ? 'flex' : 'none' }} data-body="detail">
          <IssuesPage
            start={start} onNew={() => openNewIssue()} onEdit={openEdit} focusKey={detailFocus}
            executors={executors} onFocusOrchestrator={onFocusOrchestrator}
          />
        </div>
        {formOpen && (
          <div className="absolute inset-0 flex flex-col min-h-0" style={{ display: view === 'form' ? 'flex' : 'none' }} data-body="form">
            <NewIssueForm
              draft={form.draft} editing={form.editing} onChange={(draft) => setForm({ ...formRef.current, draft })} focusKey={formFocus}
              onSaved={saved} onClose={closeForm}
            />
          </div>
        )}
      </Flexbox>
      <Modal
        open={confirm !== null}
        title={asked.current === null ? 'Descartar esta issue?' : `Descartar as alterações na #${asked.current}?`}
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
        <Text as="div" fontSize={14} lineHeight={1.6} style={{ ...SECONDARY, margin: 0 }}>
          {asked.current === null ? 'O título e a descrição escritos nesta aba serão perdidos.' : `A #${asked.current} continua como estava antes desta aba.`}
        </Text>
      </Modal>
    </Flexbox>
  );
});
