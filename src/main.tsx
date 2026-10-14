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

// Fonte da interface continua sendo a Inter; o padrão do Lobe UI é HarmonyOS Sans, que carregaria da CDN.
const antdTheme = { token: { fontFamily: "'Inter Variable', Inter, sans-serif" } };

// O ThemeProvider abre um ConfigProvider sem `motion`, e o contexto mais interno zera o valor. Por isso o ConfigProvider com motion fica dentro dele.
// Sem isso, os componentes de @lobehub/ui/base-ui (Button, Modal, Toast...) lançam erro ao renderizar.
createRoot(document.getElementById('root')!).render(
  <ThemeProvider themeMode="dark" enableCustomFonts={false} theme={antdTheme}>
    <ConfigProvider motion={motion}>
      <App />
      <ToastHost />
      <ModalHost />
      <ContextMenuHost />
      <ContextMenuDismiss />
    </ConfigProvider>
  </ThemeProvider>,
);
