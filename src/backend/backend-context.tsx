import { createContext, useContext } from 'react';
import type { ReactNode } from 'react';
import { projectBackend } from './backend';

const BackendContext = createContext(projectBackend(null));

export function BackendProvider({ projectId, children }: { projectId: string | null; children: ReactNode }) {
  return <BackendContext.Provider value={projectBackend(projectId)}>{children}</BackendContext.Provider>;
}

export function useBackend() {
  return useContext(BackendContext);
}
