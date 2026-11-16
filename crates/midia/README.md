# orquestrador-midia

Aba de mídia do editor central (porte de `src/editor/media-view.tsx`): imagem, áudio e vídeo, só leitura.

## API

- `VisaoMidia::new(caminho, window, cx)`: View (`Entity<VisaoMidia>` com `Render` e `Focusable`) para o conteúdo da aba. `caminho`
  é absoluto; o tipo vem da extensão (`midia_de`): imagem `png jpg jpeg gif webp bmp`, áudio `mp3 wav ogg oga flac m4a`, vídeo
  `mp4 m4v webm mov`. Outra extensão deixa a view vazia.
- `trocar_caminho(caminho, cx)`: renomeação com a aba aberta; recarrega.
- `definir_visivel(bool, cx)`: a bancada chama ao trocar de aba ou esconder o grupo; `false` pausa, `true` não retoma. Minimizar a
  janela pausa sozinho. Soltar a entidade (fechar a aba) para a thread do decodificador e a saída de áudio.
- Consultas: `tipo`, `caminho`, `tocando`, `estado_reproducao`, `tamanho`, `status`, `aviso`, `tamanho_real`, `tela_cheia`, `foco`.
- Ações: `alternar_tamanho`, `tocar`, `pausar`, `alternar_reproducao`, `buscar`, `definir_volume`, `alternar_mudo`,
  `definir_velocidade` (`VELOCIDADES`), `alternar_tela_cheia`, `baixar`.
- Picture-in-picture (item do menu ⋮ do vídeo): `abrir_pip`, `alternar_pip`, `voltar_da_pip` ("voltar para a guia": fecha
  sem pausar, ativa a janela e emite `VoltarParaAba`), `fechar_pip` (o "×": fecha e pausa), `pip_aberta`, `janela_pip`. A
  janela (`pip::JanelaPip`) é pequena, sem moldura, sempre no topo, redimensionável e arrastável, e desenha o quadro do
  mesmo player. Com ela aberta, `definir_visivel(false)` e minimizar não pausam; fechar a aba fecha a janela.
- `registrar_atalhos(cx)`: uma vez no início do app; contexto `VisaoMidia` (espaço/K, ←/→ ±5 s, ↑/↓ volume, M, F, Esc).
- Eventos (`EventEmitter<EventoMidia>`): `Falhou(String)`, `Reproducao { tocando }` e `VoltarParaAba` (a bancada ativa a aba).
- `player::Player` e `imagem::decodificar` ficam públicos para testes e reuso.

Imagens leem por `files::read_media_at` e áudio/vídeo passam por `files::stream_path_at` (as mesmas regras da versão 0.1.48); o arquivo é lido
do disco pelo FFmpeg, sem o protocolo `orq-media`. Ícones `icons/midia-*.svg` estão em `orquestrador-ui`.

## FFmpeg compartilhado (LGPL)

`ffmpeg-next` ligado dinamicamente ao FFmpeg 9.0.2 pré-compilado do BtbN/FFmpeg-Builds (`win64-lgpl-shared`, URL e SHA-256
fixos no script). Sem `--enable-gpl` nem `--enable-nonfree`; todos os decodificadores e demuxers nativos vêm junto (mp3, pcm,
vorbis, opus, flac, aac, h264, hevc, mpeg4, vp8, vp9, prores; mov/mp4, matroska/webm, ogg, wav, flac, mp3). O teste
`decodificadores_nativos_e_licenca` confere licença, flags e decodificadores.

Preparação (uma vez por clone e na CI), em `ferramentas/` na raiz, ignorada pelo git:

```powershell
pwsh crates/midia/preparar-ffmpeg.ps1
```

O script baixa e confere o zip em `ferramentas/ffmpeg` (recusa build com GPL ou nonfree) e o `libclang` (pip
`libclang==18.1.1`) para o bindgen. O `[env]` do `.cargo/config.toml` define `FFMPEG_DIR` e `LIBCLANG_PATH`. O `Cargo.toml`
da raiz compila o bindgen otimizado (em opt-level 0 o build script estoura a pilha). Requer Python e o Visual Studio com C++.
O `build.rs` copia as DLLs para `target/<perfil>`, `deps` e `examples`; o `empacotar.ps1` as instala ao lado do exe com
`FFmpeg-LICENSE.txt`.

## Testes e vitrine

```powershell
cargo test -p orquestrador-midia
cargo run -p orquestrador-midia --example vitrine-midia -- imagem|audio|video|tocar|pip|erro [arquivo]
```
