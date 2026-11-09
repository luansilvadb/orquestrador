/** Chave de um terminal entre projetos: o mesmo ID pode existir em projetos diferentes. */
export const termKey = (id: string, project?: string | null) => `${project ?? ''}\0${id}`;
