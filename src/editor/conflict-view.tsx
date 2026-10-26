// Aba de conflito de merge do editor central: escolha por trecho (marcadores normais e diff3), restauração de um lado, aceite de
// remoção e o texto final editável num CodeMirror.
import { useEffect, useRef, useState } from 'react';
import { Button, Text, toast } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { message } from '../backend/backend';
import { useBackend } from '../backend/backend-context';
import { CodeEditor } from './file-view';
import { Note } from './media-view';
import { blockCount, type Choice, type ConflictDraft, hasMarkers, isConflictDirty, parseConflict, renderConflict } from './workspace-store';
import type { Conflict } from '../backend/types';
import type { Common } from './diff-view';

// ---- Conflito: blocos por trecho (marcadores normais e diff3), texto final editável ----
// O rascunho é a fonte: os trechos são lidos do texto dele a cada render. O que já foi escolhido sai da lista, o que ainda tem
// marcador continua com ação, e edição à mão não precisa ser descartada. Gravar (`save`) fica com o editor central.

export function ConflictTab({
  path, common, shown, draft, putConflict, dropConflict, save, closeTab }: {
  path: string; common: Common; shown: boolean; draft: ConflictDraft | undefined;
  putConflict: (draft: ConflictDraft) => void; dropConflict: (path: string) => void;
  save: (markResolved: boolean) => Promise<boolean>; closeTab: () => void;
}) {
  const backend = useBackend();
  const [conflict, setConflict] = useState<Conflict | null>(null);
  const [error, setError] = useState('');
  const [stale, setStale] = useState(false);
  const busyRef = useRef(false);
  const [busy, setBusy] = useState(false);
  const latestPathRef = useRef(path);
  latestPathRef.current = path;
  // `reset` troca o rascunho pelo texto atual da pasta; só é chamado depois da confirmação. Sem ele, respeita o rascunho.
  const load = async (reset = false) => {
    const requestedPath = path;
    const fresh = await backend.gitConflict(requestedPath).catch((e: unknown) => {
      if (latestPathRef.current === requestedPath) setError(message(e));
      return null;
    });
    if (latestPathRef.current !== requestedPath) return;
    if (!fresh) return;
    setConflict(fresh);
    setError('');
    if (!draft || reset) {
      const text = fresh.worktree ?? '';
      putConflict({ path, origin: text, draft: text, revision: fresh.revision });
      setStale(false);
    } else {
      setStale(draft.revision !== fresh.revision);
    }
  };
  // Aba escondida não relê: ao voltar a ser a atual, relê a pasta (a revisão do rascunho pode ter ficado velha).
  useEffect(() => {
    if (!shown) return;
    void load();
  }, [shown, path]);

  const text = draft?.draft ?? '';
  // Trechos lidos do texto atual do rascunho (inclusive o que já foi resolvido à mão ou por trecho).
  const parsed = conflict ? parseConflict(text) : null;
  const pieces = parsed?.pieces ?? [];
  const count = blockCount(pieces);
  const resolvable = !!parsed && !parsed.malformed;
  const markersLeft = hasMarkers(text);
  const deletion = conflict !== null && (conflict.ours === null || conflict.theirs === null);
  const absent = conflict !== null && conflict.worktree === null;
  const fileOpen = common.fileDirty(path);
  const canMark = resolvable && !markersLeft && !common.locked && !busy && !fileOpen;

  const run = async (work: () => Promise<unknown>) => {
    if (busyRef.current || common.locked) return;
    busyRef.current = true;
    setBusy(true);
    try {
      await work();
    } catch (e) {
      toast.error({ title: message(e), duration: 7000 });
    }
    busyRef.current = false;
    setBusy(false);
  };

  // Escolhe o lado de um trecho do rascunho atual; os demais trechos ficam com marcadores.
  const choose = (index: number, value: Choice) => {
    if (!draft || !resolvable) return;
    const picks: Choice[] = Array.from({ length: count }, (_, i) => (i === index ? value : 'pending'));
    putConflict({ ...draft, draft: renderConflict(pieces, picks) });
  };
  // Restaura a versão de um lado no rascunho. Não grava: quem grava é o botão Salvar ou Marcar.
  const restore = (value: string) => {
    if (draft) putConflict({ ...draft, draft: value });
  };

  if (error && !conflict) return <Note text={error} danger />;
  if (!conflict || !draft) return <Note text="Lendo o conflito…" />;
  if (conflict.binary) return <Note text="Conflito em arquivo binário: escolha um lado pelo Git; não há texto para resolver aqui." />;

  return (
    <Flexbox flex={1} className="min-h-0">
      <Flexbox horizontal align="center" wrap="wrap" gap={6} paddingInline={12} paddingBlock={4} flex="none" className="border-b">
        <Text as="code" code type="secondary" fontSize={12} ellipsis title={path} className="min-w-0">{path}</Text>
        <Text as="span" type="secondary" fontSize={12} className="shrink-0">conflito</Text>
        {stale && <Text as="span" role="alert" type="danger" fontSize={12} className="min-w-0">A pasta mudou desde o seu rascunho.</Text>}
        {error && <Text as="span" role="status" type="danger" fontSize={12} className="min-w-0">{error}</Text>}
        <span className="ml-auto flex flex-wrap items-center gap-1.5">
          <Button
            size="small"
            onClick={async () => {
              if (stale) {
                const ok = await common.ask({
                  title: 'Recarregar do Git?',
                  body: 'O rascunho deste conflito será trocado pelo texto atual da pasta. As escolhas feitas aqui se perdem.',
                  actions: [{ key: 'ok', label: 'Recarregar' }],
                });
                if (ok !== 'ok') return;
                await load(true);
                return;
              }
              await load();
            }}
          >
            {stale ? 'Recarregar do Git' : 'Atualizar'}
          </Button>
          <Button size="small" disabled={!isConflictDirty(draft) || common.locked || busy || fileOpen} onClick={() => run(() => save(false))}>Salvar</Button>
          <Button size="small" disabled={!canMark} onClick={() => run(() => save(true))}>Marcar como resolvido</Button>
        </span>
      </Flexbox>
      {fileOpen && (
        <Flexbox horizontal align="center" paddingInline={12} height={28} flex="none" className="border-b">
          <Text as="span" role="status" type="danger" fontSize={12}>Este arquivo também está aberto no editor com alterações. Salve ou descarte antes de gravar a resolução.</Text>
        </Flexbox>
      )}
      {absent && (
        <Flexbox gap={6} paddingInline={12} paddingBlock={6} flex="none" className="border-b">
          <Text as="span" role="status" type="secondary" fontSize={12}>
            O arquivo não existe na pasta. {deletion ? 'Um lado apagou o arquivo: aceite a remoção ou restaure a versão de um lado.' : 'Restaure a versão de um lado para editar e salvar.'}
          </Text>
          <Flexbox horizontal align="center" wrap="wrap" gap={6}>
            {conflict.ours !== null && (
              <Button size="small" disabled={common.locked || busy} onClick={() => restore(conflict.ours ?? '')}>Restaurar versão atual</Button>
            )}
            {conflict.theirs !== null && (
              <Button size="small" disabled={common.locked || busy} onClick={() => restore(conflict.theirs ?? '')}>Restaurar versão recebida</Button>
            )}
          </Flexbox>
        </Flexbox>
      )}
      {deletion && !absent && (
        <Flexbox horizontal align="center" wrap="wrap" gap={8} paddingInline={12} paddingBlock={4} flex="none" className="border-b">
          <Text as="span" type="secondary" fontSize={12}>Um lado apagou o arquivo. Marcar como resolvido mantém o arquivo como está.</Text>
        </Flexbox>
      )}
      {deletion && (
        <Flexbox horizontal align="center" wrap="wrap" gap={8} paddingInline={12} paddingBlock={4} flex="none" className="border-b">
          <Button
            size="small" danger disabled={common.locked || busy}
            onClick={async () => {
              const ok = await common.ask({
                title: 'Aceitar a remoção do arquivo?',
                body: `O arquivo ${path} sai do projeto, conforme o lado que o apagou.`,
                actions: [{ key: 'ok', label: 'Aceitar remoção', danger: true }],
              });
              if (ok !== 'ok') return;
              // Revisão de quando a tela carregou o conflito; se a pasta mudou, o Git recusa e nada é removido.
              const revision = conflict.revision;
              await run(async () => {
                await backend.gitAcceptDeletion(path, revision);
                dropConflict(path);
                common.refreshGit();
                closeTab();
              });
            }}
          >
            Aceitar a remoção
          </Button>
        </Flexbox>
      )}
      {!absent && !resolvable && (
        <Flexbox paddingInline={12} paddingBlock={6} flex="none" className="border-b">
          <Text as="span" type="secondary" fontSize={12}>{parsed?.malformed ? 'Marcadores de conflito incompletos: edite o texto final à mão.' : 'Edite o texto final à mão.'}</Text>
        </Flexbox>
      )}
      {!absent && resolvable && count > 0 && (
        <Flexbox gap={6} padding={8} className="max-h-40 overflow-y-auto border-b">
          <Text as="span" type="secondary" fontSize={12}>{count === 1 ? '1 trecho com conflito' : `${count} trechos com conflito`} no texto atual:</Text>
          {Array.from({ length: count }, (_, i) => (
            <Flexbox key={i} horizontal align="center" wrap="wrap" gap={6}>
              <Text as="span" fontSize={12} className="min-w-0 flex-1">Trecho {i + 1}</Text>
              {(['ours', 'theirs', 'both'] as const).map((value) => (
                <Button key={value} size="small" disabled={common.locked || busy} onClick={() => choose(i, value)}>
                  {CHOICE_LABEL[value]}
                </Button>
              ))}
            </Flexbox>
          ))}
        </Flexbox>
      )}
      <Flexbox flex={1} className="min-h-0">
        <Flexbox flex={1} className="min-h-0 overflow-hidden">
          <CodeEditor
            text={text}
            readOnly={common.locked || busy}
            path={path}
            reveal={null}
            onChange={(value) => putConflict({ ...draft, draft: value })}
            onSave={() => void run(() => save(false))}
            onCursor={() => undefined}
          />
        </Flexbox>
      </Flexbox>
    </Flexbox>
  );
}

const CHOICE_LABEL: Record<Choice, string> = { ours: 'Atual', theirs: 'Recebida', both: 'Ambas', pending: 'Pendente' };
