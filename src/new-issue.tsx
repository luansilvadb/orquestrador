// Aba do formulário da área das Issues (`issues-area.tsx`): a Nova issue ou, com `editing`, a edição de uma issue (Editar #N), com o visual
// do app (botão primário branco): título (obrigatório), a descrição com Escrever e Visualizar (Markdown do Lobe UI), o seletor de labels da
// faixa, e Cancelar e "Criar issue" (ou "Salvar"). O texto fica na área (`draft`), não aqui: remontar este componente não perde nada.
// Ctrl+Enter grava; Ctrl+W fecha (com a confirmação da área, se houver alteração). Gravada, a issue fica escolhida e um aviso confirma;
// se o backend recusar, o aviso diz o motivo e a aba continua aberta, com o texto.
import { Suspense, useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react';
import { Flexbox } from '@lobehub/ui';
import { Button, Input, TabsIndicator, TabsList, TabsRoot, TabsTab, Text, TextArea, toast } from '@lobehub/ui/base-ui';
import { message } from './backend';
import { FIELD_FOCUS, FOCUS_BLUE, FOCUS_RING, FOCUS_RING_INSET, IssueMarkdown, LabelChips, SECONDARY } from './issue-ui';
import { useIssues, type NewIssue } from './issues-store';

export type IssueDraft = NewIssue;
export const EMPTY_DRAFT: IssueDraft = { title: '', body: '', labels: [] };
/** Com título ou descrição escritos, fechar a Nova issue pede confirmação. */
export const draftFilled = (draft: IssueDraft) => draft.title.trim() !== '' || draft.body.trim() !== '';
/** O mesmo texto e as mesmas labels, na mesma ordem: a edição sem alteração. */
export const sameDraft = (a: IssueDraft, b: IssueDraft) =>
  a.title === b.title && a.body === b.body && a.labels.length === b.labels.length && a.labels.every((l, i) => l === b.labels[i]);

export function NewIssueForm({ draft, editing, onChange, focusKey, onSaved, onClose }: {
  draft: IssueDraft;
  /** O número da issue em edição; null na Nova issue. */
  editing: number | null;
  onChange: (draft: IssueDraft) => void;
  /** Muda a cada abertura da aba: o título recebe o foco. */
  focusKey: number;
  /** A issue foi gravada: a área fecha esta aba e mostra o detalhe. */
  onSaved: () => void;
  /** Cancelar e Ctrl+W: a área fecha a aba (pergunta antes, se houver alteração). */
  onClose: () => void;
}) {
  const store = useIssues();
  const [mode, setMode] = useState<'write' | 'preview'>('write');
  const [saving, setSaving] = useState(false);
  const titleRef = useRef<HTMLInputElement>(null);
  const canSave = draft.title.trim() !== '' && !saving;

  useEffect(() => {
    // Depois da pintura: a aba acabou de aparecer (estava com `display: none`).
    const frame = requestAnimationFrame(() => titleRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [focusKey]);

  const save = async () => {
    if (!canSave) return;
    setSaving(true);
    const input = { title: draft.title.trim(), body: draft.body.trim(), labels: draft.labels };
    try {
      const issue = editing === null ? await store.create(input) : await store.update(editing, input);
      store.select(issue.number);
      toast.success({ title: editing === null ? `Issue #${issue.number} criada` : `#${issue.number} atualizada`, duration: 3000 });
      onSaved();
    } catch (e) {
      toast.error({ title: message(e), duration: 7000 });
    } finally {
      setSaving(false);
    }
  };
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      e.stopPropagation();
      void save();
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
        <Text as="h1" fontSize={20} weight={600} style={{ margin: 0 }}>{editing === null ? 'Nova issue' : `Editar #${editing}`}</Text>

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
          <LabelChips issues={store.issues} selected={draft.labels} onChange={(labels) => onChange({ ...draft, labels })} />
        </Flexbox>

        <Flexbox horizontal align="center" justify="flex-end" gap={8}>
          <Button className={FOCUS_RING} onClick={onClose}>Cancelar</Button>
          <Button
            type="primary" className={FOCUS_RING} disabled={!canSave} loading={saving} onClick={() => void save()}
            title={editing === null ? 'Criar a issue (Ctrl+Enter)' : 'Salvar as alterações (Ctrl+Enter)'}
          >
            {editing === null ? 'Criar issue' : 'Salvar'}
          </Button>
        </Flexbox>
      </Flexbox>
    </div>
  );
}
