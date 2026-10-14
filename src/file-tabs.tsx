// Helpers de menu e de nome compartilhados pela árvore (src/explorer.tsx) e pelo editor central (src/editor-workspace.tsx).
// A faixa de abas antiga saiu: as abas agora são do editor central, com grupos e prévias.
import { type ContextMenuItem, Text } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';

export const fileName = (path: string) => path.slice(path.lastIndexOf('/') + 1);

const hint = (text: string) => <Text as="span" type="secondary" fontSize={12}>{text}</Text>;
export const action = (key: string, label: string, run: () => void, shortcut?: string): ContextMenuItem => ({
  key, label, extra: shortcut ? hint(shortcut) : undefined, onClick: run,
});
// Ação sem alvo: desabilitada, com o motivo embaixo do nome (texto comum, lido por leitor de tela).
export const blocked = (key: string, label: string, reason: string): ContextMenuItem => ({
  key, disabled: true, label: <Flexbox gap={0}><span>{label}</span>{hint(reason)}</Flexbox>,
});
export const DIVIDER: ContextMenuItem = { type: 'divider' };
