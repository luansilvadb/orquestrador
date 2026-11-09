// O @xterm/headless decide entre requestIdleCallback e setTimeout com `"requestIdleCallback" in window`, e no worker não há `window`.
// Importado antes dele: `window` passa a ser o escopo do worker, que não tem requestIdleCallback, e ele usa setTimeout.
(globalThis as { window?: unknown }).window ??= globalThis;
