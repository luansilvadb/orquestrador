// Aba Nova issue da área central, no estilo da página New issue do GitHub: título (obrigatório), a descrição com Escrever e Visualizar
// (Markdown do Lobe UI), o seletor de labels da faixa, e Cancelar e "Criar issue". O texto fica no editor (`draft`), não aqui: mover a aba
// de grupo remonta este componente sem perder nada. Ctrl+Enter cria; Ctrl+W fecha (com a confirmação do editor, se houver texto).
import { lazy, Suspense, useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { Flexbox } from '@lobehub/ui';
import { Button, Input, TabsIndicator, TabsList, TabsRoot, TabsTab, Text, TextArea } from '@lobehub/ui/base-ui';
import { GREEN_BUTTON, LabelChips } from './issue-ui';
import { useIssues, type NewIssue } from './issues-store';

// O Markdown do Lobe UI (com os plugins dele) dobra o pacote principal: só carrega na primeira vez que a aba Visualizar abre.
// O componente é o mesmo `Markdown` de `@lobehub/ui`, pelo caminho do módulo dele (`es/Markdown/index`), para o Vite separar o pedaço.
const Markdown = lazy(() => import('@lobehub/ui/es/Markdown/index'));

export type IssueDraft = NewIssue;
export const EMPTY_DRAFT: IssueDraft = { title: '', body: '', labels: [] };
/** Com título ou descrição escritos, fechar pede confirmação. */
export const draftFilled = (draft: IssueDraft) => draft.title.trim() !== '' || draft.body.trim() !== '';

export function NewIssueForm({ draft, onChange, focusKey, onCreated, onClose }: {
  draft: IssueDraft;
  onChange: (draft: IssueDraft) => void;
  /** Muda a cada abertura da aba: o título recebe o foco. */
  focusKey: number;
  /** A issue foi para o store: o editor fecha esta aba e ativa a aba Issues. */
  onCreated: () => void;
  /** Cancelar e Ctrl+W: o editor fecha a aba (pergunta antes, se houver texto). */
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
    store.create({ title: draft.title.trim(), body: draft.body.trim(), labels: draft.labels });
    onCreated();
  };
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      e.stopPropagation();
      create();
    } else if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'w') {
      // Nos campos, o Ctrl+W do editor não vale (ele ignora os campos de texto): esta aba fecha por aqui.
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
            placeholder="Resumo curto da issue" autoComplete="off" aria-required="true"
          />
        </Flexbox>

        <Flexbox gap={6}>
          <Text as="span" id="new-issue-body-label" weight={600} fontSize={14}>Descrição</Text>
          <div className="overflow-hidden rounded-md border">
            <div className="relative h-9 border-b bg-[var(--ant-color-fill-quaternary)]">
              <TabsRoot variant="square" value={mode} onValueChange={(value: string) => setMode(value as 'write' | 'preview')} className="h-full gap-0">
                <TabsList className="h-full flex-nowrap justify-start" style={{ gap: 0, boxShadow: 'none' }} aria-label="Modo da descrição">
                  <TabsIndicator />
                  <TabsTab value="write" className="flex-none px-3">Escrever</TabsTab>
                  <TabsTab value="preview" className="flex-none px-3">Visualizar</TabsTab>
                </TabsList>
              </TabsRoot>
            </div>
            {mode === 'write' ? (
              <TextArea
                variant="borderless" autoSize={{ minRows: 8, maxRows: 24 }} value={draft.body} onChange={(e) => onChange({ ...draft, body: e.target.value })}
                placeholder="Descreva o problema ou a ideia. Aceita Markdown." aria-labelledby="new-issue-body-label"
                className="rounded-none! font-mono! text-[12.5px]!"
              />
            ) : (
              <div className="min-h-[180px] px-3 py-2.5" aria-label="Visualização da descrição">
                {draft.body.trim()
                  ? (
                    <Suspense fallback={<Text as="p" type="secondary" fontSize={14} style={{ margin: 0 }}>Carregando a visualização…</Text>}>
                      <Markdown fontSize={14}>{draft.body}</Markdown>
                    </Suspense>
                  )
                  : <Text as="p" type="secondary" fontSize={14} style={{ margin: 0 }}>Nada para visualizar.</Text>}
              </div>
            )}
          </div>
        </Flexbox>

        <Flexbox gap={6}>
          <Text as="span" weight={600} fontSize={14}>Labels</Text>
          <LabelChips issues={store.issues} labels={store.labels} selected={draft.labels} onChange={(labels) => onChange({ ...draft, labels })} />
        </Flexbox>

        <Flexbox horizontal align="center" justify="flex-end" gap={8}>
          {blocked && <Text as="span" type="secondary" fontSize={12} className="mr-auto">{blocked}</Text>}
          <Button onClick={onClose}>Cancelar</Button>
          <Button className={GREEN_BUTTON} disabled={!canCreate} title="Criar a issue (Ctrl+Enter)" onClick={create}>Criar issue</Button>
        </Flexbox>
      </Flexbox>
    </div>
  );
}
