# orquestrador-midia

Aba de mídia do editor central: imagem, áudio e vídeo, só leitura.

## API

- `VisaoMidia::new(caminho, window, cx)`: View (`Entity<VisaoMidia>` com `Render` e `Focusable`) para o conteúdo da aba. `caminho`
  é absoluto; o tipo vem de `midia_de`: imagem pelos formatos do crate `image`; áudio e vídeo pelas extensões que o Media
  Foundation abre (`ByteStreamHandlers`) ou que o Windows percebe como áudio/vídeo (`PerceivedType`), e pelos fluxos do arquivo
  quando a extensão falta ou não bate. Outro arquivo deixa a view vazia.
- `trocar_caminho(caminho, cx)`: renomeação com a aba aberta; recarrega.
- `definir_visivel(bool, cx)`: a bancada chama ao trocar de aba ou esconder o grupo; `false` pausa, `true` não retoma. Minimizar a
  janela pausa sozinho. Soltar a entidade (fechar a aba) para a thread do player e o motor.
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
do disco pelo Media Foundation, sem o protocolo `orq-media`. Ícones `icons/midia-*.svg` estão em `orquestrador-ui`.

## Media Foundation

Áudio e vídeo tocam no Media Engine do Windows (`IMFMediaEngine`), sem bibliotecas externas nem preparação. Cada quadro vai
do Direct3D 11 (placa de vídeo, ou WARP) para BGRA. O arquivo abre com FILE_SHARE_DELETE: renomear ou apagar com a aba aberta
funciona.

Os codecs são os do Windows. MP4/MOV/M4V/MKV com H.264, AAC, MP3, WAV, FLAC, AVI e WMV vêm nele. HEVC, VP8/VP9, AV1, MPEG-2,
Ogg, Vorbis e Opus dependem das extensões da Microsoft Store; quando falta uma, o aviso da aba diz qual instalar. No Windows
Server, o Media Foundation é o recurso `Server-Media-Foundation`.

## Testes

```powershell
cargo test -p orquestrador-midia
```

As fixtures de `tests/fixtures` têm 2 s, 64×48 e tom de 440 Hz. Os testes de formatos que dependem de extensão da Store pulam,
com aviso, onde ela falta.
