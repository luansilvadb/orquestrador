import { createRoot } from 'react-dom/client';
import { ConfigProvider, ThemeProvider } from '@lobehub/ui';
import { ModalHost, ToastHost } from '@lobehub/ui/base-ui';
import { motion } from 'motion/react';
import { App } from './App';
import './index.css';

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
    </ConfigProvider>
  </ThemeProvider>,
);
