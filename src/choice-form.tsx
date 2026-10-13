// O formulário de ferramenta, modelo e esforço numa janela (Modal): o mesmo para o orquestrador e para os executores.
import { useId, useState, type FormEvent } from 'react';
import { Alert, Button, Input, Modal, Segmented, Select } from '@lobehub/ui/base-ui';
import { message } from './backend';
import type { Choice, Options, Tool, ToolOption } from './types';

// Valores sentinela: as opções do Select precisam de um valor não vazio. '' no Choice = padrão do CLI.
const DEFAULT = '__default__';
const CUSTOM = '__custom__';

// Só os esforços que o modelo aceita; modelo fora da lista (ou padrão) recebe todos os da ferramenta.
const effortsFor = (tool: ToolOption, model: string) => {
  const known = tool.models.find((m) => m.name === model)?.efforts;
  return tool.efforts.filter((e) => !known || known.includes(e));
};
const validEffort = (options: Options, choice: Choice) => {
  const tool = options.tools.find((t) => t.name === choice.tool)!;
  return effortsFor(tool, choice.model).includes(choice.effort) ? choice.effort : '';
};

type Props = {
  open: boolean;
  title: string;
  hint: string;
  options: Options;
  initial: Choice;
  confirmLabel: string;
  onConfirm: (choice: Choice) => Promise<unknown>;
  /** Esc, clique fora e o X fazem o mesmo que este. Sem ele a janela só fecha ao confirmar. */
  onCancel?: () => void;
  /** Chamado depois de fechar. O foco não volta ao botão que abriu a janela: quem chama decide para onde ele vai. */
  onClosed?: () => void;
};

export function ChoiceForm({ open, title, onCancel, onClosed, ...form }: Props) {
  // Sem onCancel: Esc, clique fora e o X não fecham; a janela só fecha ao confirmar.
  const closable = !!onCancel;
  // afterClose roda depois da animação de saída: quem chama usa para devolver o foco ao terminal do orquestrador.
  return (
    <Modal
      open={open}
      title={title}
      width={460}
      classNames={{ title: 'text-base font-semibold' }}
      footer={false}
      closable={closable}
      keyboard={closable}
      maskClosable={closable}
      onCancel={onCancel}
      afterClose={onClosed}
    >
      {/* Desmontado ao fechar (o Modal só renderiza o conteúdo enquanto a janela está montada): a abertura seguinte começa do estado inicial. */}
      <ChoiceFields {...form} onCancel={onCancel} />
    </Modal>
  );
}

function ChoiceFields({ hint, options, initial, confirmLabel, onConfirm, onCancel }: Omit<Props, 'open' | 'title' | 'onClosed'>) {
  const ids = { model: useId(), tool: useId(), effort: useId() };
  const [choice, setChoice] = useState<Choice>(() => {
    const picked = options.tools.some((t) => t.name === initial.tool) ? initial : { tool: options.tools[0].name, model: '', effort: '' };
    return { ...picked, effort: validEffort(options, picked) };
  });
  // Modelo digitado à mão (fora da lista): o Select mostra "Outro" e o campo mostra o nome.
  const [custom, setCustom] = useState(() => {
    const models = options.tools.find((t) => t.name === choice.tool)?.models ?? [];
    return !!choice.model && !models.some((m) => m.name === choice.model);
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const tool = options.tools.find((t) => t.name === choice.tool)!;
  // Só os esforços que o modelo escolhido aceita; modelo fora da lista recebe todos os da ferramenta.
  const efforts = effortsFor(tool, choice.model);

  // O esforço é corrigido junto com a troca de modelo: o que o novo modelo não aceita vira padrão do CLI.
  // Se o modelo seguinte aceitar de novo, o padrão continua valendo; o esforço antigo não volta sozinho.
  const withModel = (model: string, isCustom: boolean) => {
    setCustom(isCustom);
    setChoice((c) => {
      const next = { ...c, model };
      return { ...next, effort: validEffort(options, next) };
    });
  };
  const pickTool = (name: Tool) => {
    setChoice({ tool: name, model: '', effort: '' });
    setCustom(false);
  };
  const pickModel = (value: string) => {
    if (value === CUSTOM) withModel('', true);
    else withModel(value === DEFAULT ? '' : value, false);
  };
  const typeModel = (value: string) => withModel(value.trim(), true);

  const modelValue = custom && !tool.models.some((m) => m.name === choice.model) ? CUSTOM : choice.model || DEFAULT;

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError('');
    try {
      await onConfirm(choice);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} className="grid gap-5">
      <p className="text-sm text-muted-foreground">{hint}</p>

      <div className="grid gap-2">
        <label id={ids.tool} className="text-sm font-medium">Ferramenta</label>
        <div role="group" aria-labelledby={ids.tool}>
          <Segmented
            value={choice.tool}
            onChange={(v) => pickTool(v as Tool)}
            options={options.tools.map((t) => ({ value: t.name, label: t.name, className: 'font-mono' }))}
          />
        </div>
      </div>

      <div className="grid gap-2">
        <label htmlFor={ids.model} className="text-sm font-medium">Modelo</label>
        <Select
          id={ids.model}
          className="w-full font-mono"
          value={modelValue}
          onChange={(v) => pickModel(v as string)}
          options={[
            { value: DEFAULT, label: 'padrão do CLI' },
            ...tool.models.map((m) => ({ value: m.name, label: m.name, className: 'font-mono' })),
            { value: CUSTOM, label: 'Outro (digitado abaixo)' },
          ]}
        />
        <Input
          aria-label="Nome de outro modelo"
          value={custom ? choice.model : ''}
          onChange={(e) => typeModel(e.target.value)}
          placeholder="ou digite o nome de outro modelo"
          autoComplete="off"
          spellCheck={false}
          className="font-mono"
        />
      </div>

      <div className="grid gap-2">
        <label id={ids.effort} className="text-sm font-medium">Esforço</label>
        <div role="group" aria-labelledby={ids.effort} className="flex flex-wrap">
          <Segmented
            size="small"
            value={choice.effort || DEFAULT}
            onChange={(v) => setChoice((c) => ({ ...c, effort: v === DEFAULT ? '' : v }))}
            options={[DEFAULT, ...efforts].map((e) => ({ value: e, label: e === DEFAULT ? 'padrão' : e, className: 'font-mono' }))}
          />
        </div>
      </div>

      {error && <Alert type="error" description={error} />}

      <div className="flex justify-end gap-2">
        {onCancel && (
          <Button htmlType="button" onClick={onCancel}>Cancelar</Button>
        )}
        <Button type="primary" htmlType="submit" loading={busy} disabled={busy}>
          {confirmLabel}
        </Button>
      </div>
    </form>
  );
}
