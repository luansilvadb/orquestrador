// O seletor de ferramenta, modelo e esforço: o mesmo para o orquestrador e para os executores.
import { message } from './backend';
import type { Choice, Options } from './types';

type Config = {
  title: string;
  hint: string;
  options: Options;
  initial: Choice;
  confirmLabel: string;
  onConfirm: (choice: Choice) => Promise<unknown>;
  onCancel?: () => void;
};

const DEFAULT = 'padrão do CLI';

function chips(box: HTMLElement, values: string[], current: string, pick: (value: string) => void) {
  box.replaceChildren(...values.map((value) => {
    const chip = document.createElement('button');
    chip.type = 'button';
    chip.className = 'chip';
    chip.textContent = value || DEFAULT;
    chip.setAttribute('aria-pressed', String(value === current));
    chip.onclick = () => pick(value);
    return chip;
  }));
}

export function createSelector(config: Config): HTMLElement {
  const { options } = config;
  const choice = { ...config.initial };
  if (!options.tools.some((t) => t.name === choice.tool)) Object.assign(choice, { tool: options.tools[0].name, model: '', effort: '' });

  const form = document.createElement('form');
  form.className = 'selector';
  form.innerHTML = `
    <div class="selector-title"></div>
    <p class="selector-hint"></p>
    <div class="field"><span>Ferramenta</span><div class="chips" data-row="tool"></div></div>
    <div class="field"><span>Modelo</span><div class="chips" data-row="model"></div>
      <input name="model" placeholder="ou digite o nome de outro modelo" autocomplete="off" spellcheck="false"></div>
    <div class="field"><span>Esforço</span><div class="chips" data-row="effort"></div></div>
    <p class="error" role="alert" hidden></p>
    <div class="selector-actions"><button type="submit" class="primary"></button><button type="button" class="ghost" hidden>Cancelar</button></div>`;
  const row = (name: string) => form.querySelector<HTMLElement>(`[data-row="${name}"]`)!;
  const typed = form.querySelector('input')!;
  const error = form.querySelector<HTMLElement>('.error')!;
  const [submit, cancel] = form.querySelectorAll<HTMLButtonElement>('.selector-actions button');
  form.querySelector('.selector-title')!.textContent = config.title;
  form.querySelector('.selector-hint')!.textContent = config.hint;
  submit.textContent = config.confirmLabel;

  const tool = () => options.tools.find((t) => t.name === choice.tool)!;
  // Só os esforços que o modelo escolhido aceita; modelo fora da lista recebe todos os da ferramenta.
  const showEfforts = () => {
    const known = tool().models.find((m) => m.name === choice.model)?.efforts;
    const list = tool().efforts.filter((e) => !known || known.includes(e));
    if (!list.includes(choice.effort)) choice.effort = '';
    chips(row('effort'), ['', ...list], choice.effort, (value) => { choice.effort = value; showEfforts(); });
  };
  const showModels = () => {
    const names = tool().models.map((m) => m.name);
    if (document.activeElement !== typed) typed.value = names.includes(choice.model) ? '' : choice.model;
    chips(row('model'), ['', ...names], choice.model, (value) => { choice.model = value; typed.value = ''; showModels(); });
    showEfforts();
  };
  // Outra ferramenta limpa modelo e esforço: os valores de uma não servem para a outra.
  const showTools = () => {
    chips(row('tool'), options.tools.map((t) => t.name), choice.tool, (value) => {
      Object.assign(choice, { tool: value, model: '', effort: '' });
      typed.value = '';
      showTools();
    });
    showModels();
  };
  typed.oninput = () => { choice.model = typed.value.trim(); showModels(); };
  showTools();

  if (config.onCancel) {
    cancel.hidden = false;
    cancel.onclick = config.onCancel;
  }
  form.onsubmit = async (event) => {
    event.preventDefault();
    submit.disabled = true;
    error.hidden = true;
    try {
      await config.onConfirm({ ...choice });
    } catch (e) {
      error.textContent = message(e);
      error.hidden = false;
    }
    submit.disabled = false;
  };
  return form;
}
