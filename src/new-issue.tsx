// Aba Nova issue da área das Issues (`issues-area.tsx`), com o visual do app (botão primário branco): título (obrigatório), a descrição com Escrever e Visualizar
// (Markdown do Lobe UI), o seletor de labels da faixa, e Cancelar e "Criar issue". O texto fica na área (`draft`), não aqui: remontar
// este componente não perde nada. Ctrl+Enter cria; Ctrl+W fecha (com a confirmação da área, se houver texto).
import { Suspense, useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react';
import { Flexbox } from '@lobehub/ui';
import { Button, Input, TabsIndicator, TabsList, TabsRoot, TabsTab, Text, TextArea, toast } from '@lobehub/ui/base-ui';
import { backend, message } from './backend';
import { FIELD_FOCUS, FOCUS_BLUE, FOCUS_RING, FOCUS_RING_INSET,IssueMarkdown, LabelChips, SECONDARY } from './issue-ui';
import { useIssues, type NewIssue } from './issues-store';

export type IssueDraft = NewIssue;
export const EMPTY_DRAFT: IssueDraft = { title: '', body: '', labels: [] };
/** Com título ou descrição escritos, fechar pede confirmação. */
export const draftFilled = (draft: IssueDraft) => draft.title.trim() !== '' || draft.body.trim() !== '';

export function NewIssueForm({ draft, onChange, focusKey, onCreated, onClose }: {
  draft: IssueDraft;
  onChange: (draft: IssueDraft) => void;
  /** Muda a cada abertura da aba: o título recebe o foco. */
  focusKey: number;
  /** A issue foi para o store: a área fecha esta aba e mostra o detalhe. */
  onCreated: () => void;
  /** Cancelar e Ctrl+W: a área fecha a aba (pergunta antes, se houver texto). */
  onClose: () => void;
}) {
  const store = useIssues();
  const [mode, setMode] = useState<'write' | 'preview'>('write');
  const titleRef = useRef<HTMLInputElement>(null);
  const ready = store.status?.kind === 'ready';
  const blocked = !ready ? (store.status?.kind === 'unavailable' ? store.status.message : 'Conectando ao GitHub…') : '';
  const canCreate = draft.title.trim() !== '' && ready;

  useEffect(() => {
    // Depois da pintura: a aba acabou de aparecer (estava com `display: none`).
    const frame = requestAnimationFrame(() => titleRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [focusKey]);

  const create = () => {
    if (!canCreate) return;
    // O número só existe quando o GitHub responde: o aviso vem então, com a ação de abrir, e o detalhe passa a mostrá-la, a não ser que
    // a seleção tenha mudado enquanto isso (quem escolheu outra issue nesse meio tempo não é interrompido).
    const selected = store.latest().selected;
    store.create({ title: draft.title.trim(), body: draft.body.trim(), labels: draft.labels }, (issue) => {
      if (store.latest().selected === selected) store.select(issue.number);
      toast.success({
        title: `Issue #${issue.number} criada`, duration: 6000,
        actions: [{ label: 'Abrir', variant: 'text', onClick: () => void backend.githubOpen(issue.url).catch((e) => toast.error({ title: message(e), duration: 7000 })) }],
      });
    });
    onCreated();
  };
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      e.stopPropagation();
      create();
    } else if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'w') {
      // Nos campos, o Ctrl+W global não vale (ignora os campos de texto): esta aba fecha por aqui.
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  };

  return (
    <div className="h-full min-h-0 overflow-y-auto" onKeyDown={onKey}>
      <Flexbox gap={16} padding={24} className="mx-auto w-full max-w-[900px]">
        <Text as="h1" fontSize={20} weight={600} style={{ margin: 0 }}>Nova issue</Text>

        <Flexbox gap={6}>
          <label htmlFor="new-issue-title">
            <Text as="span" weight={600} fontSize={14}>Título</Text>
            <Text as="span" type="danger" weight={600} fontSize={14} aria-hidden> *</Text>
          </label>
          <Input
            id="new-issue-title" ref={titleRef} value={draft.title} onChange={(e) => onChange({ ...draft, title: e.target.value })}
            placeholder="Resumo curto da issue" autoComplete="off" aria-required="true" className={FIELD_FOCUS}
          />
        </Flexbox>

        <Flexbox gap={6}>
          <Text as="span" id="new-issue-body-label" weight={600} fontSize={14}>Descrição</Text>
          <div className="overflow-hidden rounded-md border focus-within:border-[var(--focus-blue)]" style={{ '--focus-blue': FOCUS_BLUE } as CSSProperties}>
            <div className="relative h-9 border-b bg-[var(--ant-color-fill-quaternary)]">
              <TabsRoot variant="square" value={mode} onValueChange={(value: string) => setMode(value as 'write' | 'preview')} className="h-full gap-0">
                <TabsList className="h-full flex-nowrap justify-start" style={{ gap: 0, boxShadow: 'none' }} aria-label="Modo da descrição">
                  <TabsIndicator />
                  <TabsTab value="write" className={`flex-none px-3 ${FOCUS_RING_INSET}`}>Escrever</TabsTab>
                  <TabsTab value="preview" className={`flex-none px-3 ${FOCUS_RING_INSET}`}>Visualizar</TabsTab>
                </TabsList>
              </TabsRoot>
            </div>
            {mode === 'write' ? (
              <TextArea
                variant="borderless" autoSize={{ minRows: 8, maxRows: 24 }} value={draft.body} onChange={(e) => onChange({ ...draft, body: e.target.value })}
                placeholder="Descreva o problema ou a ideia. Aceita Markdown." aria-labelledby="new-issue-body-label"
                className="rounded-none! text-[14px]!"
              />
            ) : (
              <div className="min-h-[180px] px-3 py-2.5" aria-label="Visualização da descrição">
                {draft.body.trim()
                  ? (
                    <Suspense fallback={<Text as="p" fontSize={14} style={{ ...SECONDARY, margin: 0 }}>Carregando a visualização…</Text>}>
                      <IssueMarkdown fontSize={14}>{draft.body}</IssueMarkdown>
                    </Suspense>
                  )
                  : <Text as="p" fontSize={14} style={{ ...SECONDARY, margin: 0 }}>Nada para visualizar.</Text>}
              </div>
            )}
          </div>
        </Flexbox>

        <Flexbox gap={6}>
          <Text as="span" weight={600} fontSize={14}>Labels</Text>
          <LabelChips issues={store.issues} labels={store.labels} selected={draft.labels} onChange={(labels) => onChange({ ...draft, labels })} />
        </Flexbox>

        <Flexbox horizontal align="center" justify="flex-end" gap={8}>
          {blocked && <Text style={SECONDARY} as="span" fontSize={12} className="mr-auto">{blocked}</Text>}
          <Button className={FOCUS_RING} onClick={onClose}>Cancelar</Button>
          <Button type="primary" className={FOCUS_RING} disabled={!canCreate} title="Criar a issue (Ctrl+Enter)" onClick={create}>Criar issue</Button>
        </Flexbox>
      </Flexbox>
    </div>
  );
}
