import { createRoot } from 'react-dom/client';
import { ConfigProvider, ThemeProvider } from '@lobehub/ui';
import { closeContextMenu, ContextMenuHost, ModalHost, ToastHost } from '@lobehub/ui/base-ui';
import { motion } from 'motion/react';
import { App } from './App';
import { DiffWorkerPool } from './editor/diff-worker';
import { showFieldMenu } from './shared/field-menu';
import './index.css';

// Clique fora fecha o menu de contexto aberto. O overlay inerte da biblioteca não fecha o menu no pointerdown externo; a escuta em captura chama a API pública.
// Só age com um menu na tela e um alvo fora de todos os `role="menu"` (submenus incluídos). Não cancela o evento: o clique segue para o alvo.
window.addEventListener(
  'pointerdown',
  (event) => {
    if (!(event.target instanceof Element) || !document.querySelector('[role="menu"]')) return;
    if (event.target.closest('[role="menu"]')) return;
    closeContextMenu();
  },
  true,
);

// Sem o menu de contexto do WebView (Voltar, Atualizar, Salvar como, Imprimir, Inspecionar): o app se comporta como nativo. Campos de texto
// ganham o menu de edição do app; o textarea oculto do xterm não conta. Os menus do app (`ContextMenuTrigger`) abrem antes e marcam o evento.
window.addEventListener('contextmenu', (event) => {
  const wasHandled = event.defaultPrevented;
  event.preventDefault();
  const field = event.target instanceof Element ? event.target.closest('input, textarea') : null;
  if (wasHandled || !(field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement)) return;
  if (field.classList.contains('xterm-helper-textarea') || field.selectionStart === null) return;
  showFieldMenu(field);
});

// Avisos em português. A região dos avisos da Base UI se chama "Notifications", fixo no código da biblioteca, sem prop para trocar: o
// observador troca o rótulo assim que as regiões aparecem. Cada aviso (`dialog`) já tem o título como nome (`aria-labelledby`); um sem
// título ganharia o texto como nome. Até o ToastHost montar as regiões, o observador olha a árvore toda; depois, só as regiões, que ficam
// montadas enquanto o app roda. O fechar do Lobe UI fica escondido pelo CSS (`.app-toasts` em index.css).
const fixToasts = () => {
  for (const item of document.querySelectorAll('[aria-label="Notificações"] [role="dialog"]:not([aria-labelledby]):not([aria-label])')) {
    item.setAttribute('aria-label', item.textContent?.trim() ?? '');
  }
};
const toastRegions = new MutationObserver(fixToasts);
const toastHost = new MutationObserver(() => {
  const found = document.querySelectorAll('[role="region"][aria-label="Notifications"]');
  for (const region of found) {
    region.setAttribute('aria-label', 'Notificações');
    toastRegions.observe(region, { childList: true, subtree: true });
  }
  if (!found.length) return;
  toastHost.disconnect();
  fixToasts();
});
toastHost.observe(document.body, { childList: true, subtree: true });

// Fonte da interface continua sendo a Inter; o padrão do Lobe UI é HarmonyOS Sans, que carregaria da CDN.
// colorTextPlaceholder: o padrão do tema (#555555) dá 2,6:1 sobre preto; o texto secundário (#aaaaaa) passa de 4,5:1.
const antdTheme = { token: { fontFamily: "'Inter Variable', Inter, sans-serif", colorTextPlaceholder: '#aaaaaa' } };

// O ThemeProvider abre um ConfigProvider sem `motion`, e o contexto mais interno zera o valor. Por isso o ConfigProvider com motion fica dentro dele.
// Sem isso, os componentes de @lobehub/ui/base-ui (Button, Modal, Toast...) lançam erro ao renderizar.
// appearance="dark": sem ele, o antd-style começa em light e só troca para dark depois do primeiro paint, que sai branco.
createRoot(document.getElementById('root')!).render(
  <ThemeProvider themeMode="dark" appearance="dark" enableCustomFonts={false} theme={antdTheme}>
    <ConfigProvider motion={motion}>
      <DiffWorkerPool>
        <App />
      </DiffWorkerPool>
      <ToastHost className="app-toasts" position="top" swipeDirection="up" />
      <ModalHost />
      <ContextMenuHost />
    </ConfigProvider>
  </ThemeProvider>,
);
