import { useEffect } from 'react';
import { createRoot } from 'react-dom/client';
import { ConfigProvider, ThemeProvider } from '@lobehub/ui';
import { closeContextMenu, ContextMenuHost, ModalHost, ToastHost } from '@lobehub/ui/base-ui';
import { motion } from 'motion/react';
import { App } from './App';
import './index.css';

// Clique fora fecha o menu de contexto aberto. O overlay inerte da biblioteca não fecha o menu no pointerdown externo; a escuta em captura chama a API pública.
// Só age com um menu na tela e um alvo fora de todos os `role="menu"` (submenus incluídos). Não cancela o evento: o clique segue para o alvo.
function ContextMenuDismiss() {
  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Element) || !document.querySelector('[role="menu"]')) return;
      if (event.target.closest('[role="menu"]')) return;
      closeContextMenu();
    };
    window.addEventListener('pointerdown', onPointerDown, true);
    return () => window.removeEventListener('pointerdown', onPointerDown, true);
  }, []);
  return null;
}

// Avisos em português. A região dos avisos da Base UI se chama "Notifications" e o fechar do Toast do Lobe UI, "Close", fixos no código das
// bibliotecas, sem prop para trocar: o observador troca os rótulos assim que as regiões e os avisos aparecem. Cada aviso (`dialog`) já tem o
// título como nome (`aria-labelledby`); um sem título ganharia o texto como nome. Até o ToastHost montar as regiões, o observador olha a
// árvore toda; depois, só as regiões, que ficam montadas enquanto o app roda.
function ToastLabels() {
  useEffect(() => {
    const fix = () => {
      for (const close of document.querySelectorAll('[aria-label="Notificações"] button[aria-label="Close"]')) close.setAttribute('aria-label', 'Fechar');
      for (const item of document.querySelectorAll('[aria-label="Notificações"] [role="dialog"]:not([aria-labelledby]):not([aria-label])')) {
        item.setAttribute('aria-label', item.textContent?.trim() ?? '');
      }
    };
    const regions = new MutationObserver(fix);
    const findRegions = () => {
      const found = document.querySelectorAll('[role="region"][aria-label="Notifications"]');
      for (const region of found) {
        region.setAttribute('aria-label', 'Notificações');
        regions.observe(region, { childList: true, subtree: true });
      }
      if (!found.length) return;
      host.disconnect();
      fix();
    };
    const host = new MutationObserver(findRegions);
    host.observe(document.body, { childList: true, subtree: true });
    findRegions();
    return () => {
      host.disconnect();
      regions.disconnect();
    };
  }, []);
  return null;
}

// Fonte da interface continua sendo a Inter; o padrão do Lobe UI é HarmonyOS Sans, que carregaria da CDN.
// colorTextPlaceholder: o padrão do tema (#555555) dá 2,6:1 sobre preto; o texto secundário (#aaaaaa) passa de 4,5:1.
const antdTheme = { token: { fontFamily: "'Inter Variable', Inter, sans-serif", colorTextPlaceholder: '#aaaaaa' } };

// O ThemeProvider abre um ConfigProvider sem `motion`, e o contexto mais interno zera o valor. Por isso o ConfigProvider com motion fica dentro dele.
// Sem isso, os componentes de @lobehub/ui/base-ui (Button, Modal, Toast...) lançam erro ao renderizar.
createRoot(document.getElementById('root')!).render(
  <ThemeProvider themeMode="dark" enableCustomFonts={false} theme={antdTheme}>
    <ConfigProvider motion={motion}>
      <App />
      <ToastHost />
      <ToastLabels />
      <ModalHost />
      <ContextMenuHost />
      <ContextMenuDismiss />
    </ConfigProvider>
  </ThemeProvider>,
);
