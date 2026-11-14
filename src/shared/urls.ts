// Regras puras de URL, sem React, para os checks rodarem direto no Node.

/** URL `http://` ou `https://` (sem diferença de caixa). */
export const isWebUrl = (url: string) => /^https?:\/\//i.test(url);
