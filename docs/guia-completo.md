# Guia Completo de Arquitetura, Operação e Empacotamento — Overture

## 1. Decisões Consolidadas (Seção 11 do Planejamento)

1. **Nome Final e Identidade Windows**
   - Nome do produto: **Overture** (`overture.exe`)
   - `AppUserModelID`: `Overture.WorshipProjection.App.1` (aplicado tanto via Win32 `SetCurrentProcessExplicitAppUserModelID` na inicialização quanto na tabela `MsiShortcutProperty` do atalho no Menu Iniciar).
   - Ícone próprio multi-resolução: `assets/overture.ico` contendo camadas PNG de `16x16`, `32x32`, `48x48`, `64x64`, `128x128` e `256x256` pixels, embutido nos recursos PE do executável via `winresource`.

2. **Bíblias Incluídas e Licenciamento**
   - **TB1917 (`pt-BR`)**: *Tradução Brasileira (1917)* — **Domínio Público** (`Public Domain`).
   - **WEB (`en`)**: *World English Bible* — **Domínio Público** (`Public Domain`).
   - Novas versões podem ser importadas pelo operador nos formatos **Zefania XML** ou **JSON**, exigindo declaração explícita de licença antes da gravação em `bible_versions`.

3. **Temas Padrão Pré-Configurados**
   - **Culto Principal**: Fonte `Segoe UI` 64 pt negrito, centralizada, gradiente azul-profundo (`#0B132B` -> `#1C2541`) ou motion background `motion://aurora`, sombra suave ativada.
   - **Escrituras**: Fonte `Georgia` 54 pt, centralizada, fundo escuro sóbrio voltado à leitura prolongada.
   - **Retorno Palco (Alto Contraste)**: Fonte `Segoe UI` 72 pt amarelo-vivo (`#FFE600`) sobre fundo preto absoluto (`#000000`), sem sombra, exclusivo para monitores de retorno da equipe.

---

## 2. As 7 Telas Operacionais (`crates/ui/ui/main.slint`)

1. **Controle (Ao Vivo)**: Ordem do culto e playlist temporária à esquerda; grade de slides do item ativo e controles rápidos de contagem regressiva, placar manual e alerta de palco no centro; pré-visualização renderizada pelo compositor real à direita com botões de transição (Anterior, Próximo, Tela Preta, Limpar Texto, Logo e Auto-Play).
2. **Biblioteca**: Abas dedicadas para Músicas, Bíblia, Mídias e Apresentações, com busca instantânea (`< 100 ms`), importação em lote (`.txt`, OpenSong, OpenLP, `.pptx`, Zefania XML), filtro de arquivados e remoção de duplicatas exatas.
3. **Editor de Música**: Edição de título, artista, autor, observações e letra com redivisão automática respeitando as regras semânticas em português, prévia dos cartões de slides e indicação visual de `quebra_forcada`.
4. **Temas**: Editor completo de tipografia, cores hexadecimal, alinhamento, sombra e vídeo/motion de fundo (`motion://aurora`, `motion://ocean`, `motion://warm` ou arquivo `.mp4`/`.mkv`), com prévia renderizada ao vivo.
5. **Monitores**: Associação independente de monitores físicos Windows para **Saída Principal**, **Tela de Palco** e **Monitor de Retorno**, escolha do tema de retorno e abertura/fechamento instantâneo das janelas fullscreen.
6. **Escala e Relatórios**: Cadastro de integrantes da equipe, montagem da escala do culto com contador histórico de participações, músicas mais executadas no período e distribuição por dia da semana.
7. **Configurações**: Troca imediata de idioma (**Português (Brasil)** / **English**), porta e token fixo da API de controle remoto, exportação TXT/JSON e criação/restauração de backup completo `.zip`.

---

## 3. Atalhos de Teclado Padrão na Operação

| Tecla | Ação |
|---|---|
| `Seta Direita` / `Espaço` / `PageDown` | Próximo slide |
| `Seta Esquerda` / `PageUp` | Slide anterior |
| `B` | Alternar Tela Preta (*Blackout*) |
| `C` | Alternar Limpar Texto (mantém o fundo/vídeo ativo) |
| `L` | Alternar Modo Logo |

---

## 4. API Local de Controle Remoto (`crates/control`)

O servidor escuta na rede local (ex.: `http://<IP_DA_MAQUINA>:4875`) e aceita conexões apenas de endereços privados/loopback (`127.0.0.0/8`, `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`).

- **Interface Web Mobile:** `GET /` (insira o token fixo exibido na aba Configurações).
- **Autenticação:** Cabeçalho `Authorization: Bearer <TOKEN>` ou query `?token=<TOKEN>`.
- **Endpoints REST:**
  - `GET /api/state` — Estado completo da projeção, item atual, índice do slide e itens do culto.
  - `POST /api/next` e `POST /api/prev` — Avançar ou retroceder slide.
  - `POST /api/slide/{index}` — Ir diretamente para um slide específico.
  - `POST /api/service-item/{index}` — Ativar item da ordem de culto.
  - `POST /api/mode` — Definir modo de exibição (`{"mode": "normal" | "black" | "clear_text" | "logo"}`).
  - `POST /api/stage-alert` — Enviar ou limpar mensagem de alerta nos monitores de palco/retorno.
- **Tempo Real (WebSocket):** `GET /ws?token=<TOKEN>` — Transmite atualizações instantâneas de `OutputState` a todos os celulares conectados.

---

## 5. Instalador MSI, Atualização In-Place e Desinstalação

### Geração Reprodutível
```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-msi.ps1
```

### Estrutura Instalada (`C:\Program Files\Overture`)
- `overture.exe` — Aplicativo nativo compilado em `--release`
- `d3dcompiler_47.dll` — Runtime auxiliar DirectX / `wgpu`
- `redist\libmpv-2.dll` e `redist\mpv.conf` — Motor de vídeo e áudio `libmpv`
- `assets\overture.ico` — Ícone multi-resolução
- `LICENSE.rtf` — Licença GNU GPL v3.0

### Ciclo de Vida no Windows
- **Instalação:** Cria a pasta em `Program Files\Overture`, registra o aplicativo em *Configurações > Aplicativos > Aplicativos instalados* e adiciona o atalho **Overture** no Menu Iniciar pronto para ser fixado na barra de tarefas.
- **Upgrade In-Place:** Ao incrementar a versão em `Cargo.toml` mantendo `UpgradeCode = 9F4E8A71-3D2C-4B85-A619-7C80E5D12F01`, o `MajorUpgrade` substitui automaticamente os binários antigos preservando o banco SQLite e mídias do usuário em `%APPDATA%\Overture`.
- **Desinstalação:** Remove integralmente os binários, bibliotecas `redist/` e o atalho do Menu Iniciar diretamente por *Configurações > Aplicativos instalados*.
