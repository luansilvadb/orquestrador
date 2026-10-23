import { showContextMenu, toast } from '@lobehub/ui/base-ui';
import { message } from '../backend/backend';
import { action, blocked, DIVIDER } from './file-tabs';

// Menu de edição dos campos de texto, no lugar do menu do WebView (que traz Emoji, Direção da escrita, Mais ferramentas e Inspecionar).
// Os comandos de edição do navegador preservam o desfazer do campo e disparam `input`, então o React vê a mudança.
export function showFieldMenu(field: HTMLInputElement | HTMLTextAreaElement) {
  const editable = !field.readOnly && !field.disabled;
  const selected = (field.selectionEnd ?? 0) > (field.selectionStart ?? 0);
  const run = (command: string, value?: string) => {
    field.focus();
    document.execCommand(command, false, value);
  };
  const paste = async () => {
    try {
      run('insertText', await navigator.clipboard.readText());
    } catch (e) {
      toast.error({ title: message(e), duration: 5000 });
    }
  };
  const editing = (key: string, label: string, fn: () => void, shortcut: string) => (editable ? action(key, label, fn, shortcut) : blocked(key, label, 'Campo somente leitura.'));
  showContextMenu([
    editing('undo', 'Desfazer', () => run('undo'), 'Ctrl+Z'),
    editing('redo', 'Refazer', () => run('redo'), 'Ctrl+Y'),
    DIVIDER,
    editable && selected ? action('cut', 'Recortar', () => run('cut'), 'Ctrl+X') : blocked('cut', 'Recortar', editable ? 'Selecione um trecho.' : 'Campo somente leitura.'),
    selected ? action('copy', 'Copiar', () => run('copy'), 'Ctrl+C') : blocked('copy', 'Copiar', 'Selecione um trecho.'),
    editing('paste', 'Colar', () => void paste(), 'Ctrl+V'),
    DIVIDER,
    action('select-all', 'Selecionar tudo', () => { field.focus(); field.select(); }, 'Ctrl+A'),
  ]);
}
