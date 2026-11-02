// Limita o crescimento de src-tauri/target: acima do teto, roda `cargo clean`. Só usa módulos nativos do Node.
// Teto padrão de 30 GB; sobrescreva com ORQUESTRADOR_TARGET_MAX_GB. Nunca bloqueia o comando seguinte (sai sempre com 0).
import { spawnSync } from 'node:child_process';
import { lstat, readdir } from 'node:fs/promises';
import { join } from 'node:path';

const GB = 1024 ** 3;
const root = join(import.meta.dirname, '..');
const manifest = join(root, 'src-tauri', 'Cargo.toml');
const target = join(root, 'src-tauri', 'target');
const configured = Number(process.env.ORQUESTRADOR_TARGET_MAX_GB);
const limit = (Number.isFinite(configured) && configured >= 0 && process.env.ORQUESTRADOR_TARGET_MAX_GB?.trim() ? configured : 30) * GB;

// Soma o tamanho dos arquivos sem seguir symlinks nem junctions; erros de acesso valem 0.
async function sizeOf(dir) {
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return 0;
  }
  const sizes = await Promise.all(entries.map(async (entry) => {
    const path = join(dir, entry.name);
    try {
      if (entry.isSymbolicLink()) return 0;
      if (entry.isDirectory()) {
        // Junctions do Windows aparecem como diretório; lstat revela o reparse point.
        const info = await lstat(path);
        return info.isSymbolicLink() ? 0 : sizeOf(path);
      }
      return entry.isFile() ? (await lstat(path)).size : 0;
    } catch {
      return 0;
    }
  }));
  return sizes.reduce((total, size) => total + size, 0);
}

try {
  await lstat(target);
} catch {
  process.exit(0);
}

const size = await sizeOf(target);
if (size > limit) {
  const gb = (bytes) => +(bytes / GB).toFixed(3);
  console.log(`src-tauri/target tem ${gb(size)} GB, acima do teto de ${gb(limit)} GB: cargo clean`);
  const result = spawnSync('cargo', ['clean', '--manifest-path', manifest], { cwd: root, stdio: 'inherit' });
  if (result.status !== 0) {
    console.error(`aviso: cargo clean falhou (${result.error?.message ?? `código ${result.status}`}); seguindo sem limpar`);
  }
}
