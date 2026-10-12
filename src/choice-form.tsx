// O formulário de ferramenta, modelo e esforço: o mesmo para o orquestrador e para os executores.
import { useId, useState, type FormEvent } from 'react';
import { LoaderCircle } from 'lucide-react';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { message } from './backend';
import type { Choice, Options, Tool, ToolOption } from './types';

// Valores sentinela: o Select e o ToggleGroup não aceitam '' como valor de item. '' no Choice = padrão do CLI.
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
  title: string;
  hint: string;
  options: Options;
  initial: Choice;
  confirmLabel: string;
  onConfirm: (choice: Choice) => Promise<unknown>;
  onCancel?: () => void;
};

export function ChoiceForm({ title, hint, options, initial, confirmLabel, onConfirm, onCancel }: Props) {
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
    <form onSubmit={submit} className="grid w-full max-w-[460px] gap-5">
      <div className="grid gap-1">
        <h2 className="text-base font-semibold">{title}</h2>
        <p className="text-sm text-muted-foreground">{hint}</p>
      </div>

      <div className="grid gap-2">
        <Label id={ids.tool}>Ferramenta</Label>
        <ToggleGroup aria-labelledby={ids.tool} type="single" variant="outline" value={choice.tool} spacing={0}
          onValueChange={(v) => v && pickTool(v as Tool)} className="justify-start">
          {options.tools.map((t) => (
            <ToggleGroupItem key={t.name} value={t.name} className="px-4 font-mono">{t.name}</ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>

      <div className="grid gap-2">
        <Label htmlFor={ids.model}>Modelo</Label>
        <Select value={modelValue} onValueChange={pickModel}>
          <SelectTrigger id={ids.model} className="w-full font-mono">
            <SelectValue />
          </SelectTrigger>
          <SelectContent position="popper" align="start">
            <SelectItem value={DEFAULT}>padrão do CLI</SelectItem>
            {tool.models.map((m) => (
              <SelectItem key={m.name} value={m.name} className="font-mono">{m.name}</SelectItem>
            ))}
            <SelectItem value={CUSTOM}>Outro (digitado abaixo)</SelectItem>
          </SelectContent>
        </Select>
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
        <Label id={ids.effort}>Esforço</Label>
        <ToggleGroup aria-labelledby={ids.effort} type="single" variant="outline" size="sm" spacing={0}
          value={choice.effort || DEFAULT} className="flex-wrap justify-start"
          onValueChange={(v) => v && setChoice((c) => ({ ...c, effort: v === DEFAULT ? '' : v }))}>
          {[DEFAULT, ...efforts].map((e) => (
            <ToggleGroupItem key={e} value={e} className="font-mono">{e === DEFAULT ? 'padrão' : e}</ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>

      {error && (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      <div className="flex flex-wrap justify-end gap-2">
        {onCancel && (
          <Button type="button" variant="outline" onClick={onCancel}>Cancelar</Button>
        )}
        <Button type="submit" disabled={busy}>
          {busy && <LoaderCircle className="animate-spin" />}
          {confirmLabel}
        </Button>
      </div>
    </form>
  );
}
