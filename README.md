# Overture

Aplicativo desktop nativo **100% Rust puro** (zero JavaScript / WebView2) para **Windows**, inspirado no fluxo operacional de estúdio do **Holyrics** e modernizado com um design system próprio baseado em **`shadcn/ui` Zinc Dark** renderizado em GPU via **Slint**. Voltado à montagem e projeção de letras, Bíblia completa (66 livros), mídias, contagem regressiva, placar e avisos em cultos e eventos, com saída principal fullscreen acelerada por GPU (`wgpu` + `cosmic-text`), tela de palco independente, monitor de retorno com tema próprio, controle remoto via celular na rede local (`axum` HTTP + WebSocket), internacionalização em tempo real (**PT-BR** / **EN**) e instalador nativo `.msi` (`cargo-wix`).

---

## Principais Funcionalidades

- **Estúdio de Projeção em 5 Colunas & Design System `shadcn/ui` em Rust Puro (`crates/ui`)**
  - **Modo Louvor (5 Colunas Redimensionáveis):** barra superior de menus estilo desktop (`Menubar`), Biblioteca lateral unificada (`Letras | Texto | Áudio | Vídeo | Img`), lista/grade de slides com histórico de sessão, Monitor Ao Vivo 16:9 (`Principal | Palco | Retorno`), Ordem do Culto / Playlist Temporária + Anotações e tira vertical de miniaturas reais de Temas renderizadas pelo compositor GPU.
  - **Modo Bíblia (Grade Canônica Colorida de 66 Livros):** grade 11×6 codificada por categorias bíblicas (Pentateuco, Históricos, Poéticos, Profetas, Evangelhos, Atos, Epístolas e Apocalipse) + grades rápidas de Capítulos e Versículos com projeção em 1 clique.
  - **Modais de Estúdio & Diálogos Nativos (`rfd`):** Contagem Regressiva com prévia 16:9 ao vivo, Editor de Música, Estúdio de Temas, Multi-Monitor, Placar/Alerta de Palco, Escalas/Relatórios e seletores nativos de arquivos do Windows 11.

- **Biblioteca de Músicas & Importação Inteligente (`crates/import`, `crates/storage`)**
  - Importação em lote de arquivos `.txt` (`UTF-8` e `ISO-8859-1` / `Windows-1252`), OpenSong XML, OpenLP (OpenLyrics) XML, PowerPoint nativo (`.pptx`) e pacote JSON próprio.
  - Extração automática de `Artista - Título.txt` (ou fallback para a primeira linha), remoção de cabeçalhos de sites de cifra/letra, máximo de 2 linhas por slide sem misturar estrofes, respeito à pontuação e proteção gramatical contra separação de artigos/preposições (`de`, `da`, `do`, `no`, `na`, `pra`, `pro`, `com`, `em`, `por` etc.), além de marcador `quebra_forcada` para revisão.
  - Remoção de duplicatas exatas, sinalização de homônimos (`título + artista`), arquivamento reversível e busca indexada em **menos de 100 ms** mesmo em bibliotecas com mais de 500 músicas.
- **Montagem de Culto & Playlist Temporária (`crates/core`, `crates/storage`)**
  - Ordem de culto persistente e playlist rápida temporária suportando músicas, versículos bíblicos, vídeos, áudios, imagens, apresentações `.pptx`, avisos, contagem regressiva e placar manual.
  - Modo de apresentação automática com tempo configurável por slide.
- **Saídas Multi-Monitor em GPU (`crates/render`, `crates/output`)**
  - Enumeração nativa Win32 com suporte a **Per-Monitor DPI V2** e `AppUserModelID` (`Overture.WorshipProjection.App.1`).
  - **Saída Principal:** texto com sombra, contorno, alinhamento configurável, fundo sólido, gradiente, imagem ou vídeo/motion background em tempo real; modos Tela Preta, Limpar Texto e Logo.
  - **Tela de Palco:** slide atual, prévia do próximo slide, relógio/cronômetro e notas/avisos de palco.
  - **Monitor de Retorno:** tema exclusivo de alto contraste + alertas imediatos para a equipe.
- **Mídias, Vídeo de Fundo, Contagem Regressiva & Placar (`crates/media`)**
  - Integração dinâmica via C-API com `libmpv` (`libmpv-2.dll`) para decodificação de vídeo/áudio e composição RGBA em textura `wgpu`, com fallback automático para fundos animados procedurais (`motion://aurora`, `motion://ocean`, `motion://warm`).
  - Contagem regressiva sincronizada na saída e placar manual controlado em tempo real pelo operador.
- **Bíblia Completa de 66 Livros em Domínio Público (`crates/storage`, `crates/import`)**
  - Embute integralmente os **66 livros, 1.189 capítulos e 31.102 versículos** da **Tradução Brasileira 1917 (`TB1917` — Domínio Público)** em português, além de semente em inglês (`WEB` — World English Bible) e importador Zefania XML com verificação explícita de licença.
  - Navegação visual instantânea por grade Livro → Capítulo → Versículo, busca por referência (`João 3:16`, `Sl 23:1-6`) e por palavras-chave, favoritos e histórico de leitura.
- **Controle Remoto Mobile na Rede Local (`crates/control`)**
  - Servidor `axum` HTTP + WebSocket restrito à LAN e protegido por token fixo configurável, acompanhado de Web App responsivo para celular (próximo/anterior, tela preta/logo, itens do culto e alerta de palco).
- **Escala, Relatórios, Exportação e Backup Completo**
  - Cadastro de equipe, escalas por culto com contagem de participações e relatórios de músicas mais tocadas por período e por dia da semana.
  - Exportação em `.txt` e `.json`, além de backup completo (`.zip` contendo snapshot consistente `VACUUM INTO` do SQLite + pasta de mídias) e restauração íntegra.

---

## Arquitetura do Workspace (10 Crates)

| Crate | Diretório | Responsabilidade |
|---|---|---|
| `overture-core` | `crates/core` | Modelos de domínio, máquina de estado reativa (`StateEngine`), atalhos e eventos |
| `overture-storage` | `crates/storage` | SQLite (`sqlx`), migrações, busca rápida, Bíblia, escalas, relatórios e backup `.zip` |
| `overture-import` | `crates/import` | Parsers TXT (PT-BR/EN), OpenSong, OpenLP, PowerPoint (`.pptx`), Zefania XML e exportadores |
| `overture-render` | `crates/render` | Compositor `cosmic-text` + `wgpu` (principal, palco, retorno, vídeo RGBA, placar e timer) |
| `overture-media` | `crates/media` | Motor `libmpv` dinâmico (`libloading`) + gerador procedural de motion backgrounds |
| `overture-output` | `crates/output` | Monitores Win32, DPI V2, janelas fullscreen independentes e telemetria de latência |
| `overture-control` | `crates/control` | API REST + WebSocket (`axum`) autenticada por token fixo + interface Web responsiva |
| `overture-i18n` | `crates/i18n` | Dicionários completos `pt-BR` e `en` com alternância instantânea em tempo de execução |
| `overture-ui` | `crates/ui` | Design system `shadcn/ui` puro em Slint (`theme`, `icons`, `components`, `bible_view`, `modals`, `main`) |
| `overture` | `crates/app` | Executável desktop Windows integrando banco, saídas GPU, mídia, API remota e UI |

---

## Compilação, Execução e Testes

### Pré-requisitos
- **Windows 10 / 11 (x64)**
- **Rust Toolchain (`stable-x86_64-pc-windows-msvc`)** + Visual Studio C++ Build Tools

### Comandos Principais

```powershell
# Executar toda a suíte de testes (unitários, integração, regressão de 20 músicas, API e multi-monitor)
cargo test --workspace

# Executar o spike de saída fullscreen em monitor (Fase 0)
cargo run -p overture-output --example monitor_spike

# Executar smoke test automatizado da aplicação completa
cargo run -p overture -- --smoke-test

# Iniciar o aplicativo desktop Overture
cargo run --release -p overture
```

---

## Instalador Windows (`.msi`)

O instalador nativo `.msi` é construído via `cargo-wix` + **WiX Toolset v3.14** utilizando [`crates/app/wix/main.wxs`](crates/app/wix/main.wxs) e empacota automaticamente `overture.exe`, `libmpv-2.dll`, `d3dcompiler_47.dll`, `mpv.conf`, `overture.ico` (16/32/48/64/128/256 px) e `LICENSE.rtf`:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-msi.ps1
```

O script gera `target\wix\overture-0.1.0-x86_64.msi` e executa [`scripts/verify-msi.ps1`](scripts/verify-msi.ps1) para validar:
- Registro em **Configurações > Aplicativos > Aplicativos instalados** com ícone e desinstalador;
- Atalho no **Menu Iniciar** com `System.AppUserModel.ID = Overture.WorshipProjection.App.1` (agrupamento e fixação na barra de tarefas);
- `UpgradeCode` fixo (`{9F4E8A71-3D2C-4B85-A619-7C80E5D12F01}`) para atualização *in-place* sem desinstalação manual.

Consulte [`docs/guia-completo.md`](docs/guia-completo.md) para detalhes operacionais, endpoints da API remota e decisões de projeto.

---

## Licença

Distribuído sob a licença **GNU General Public License v3.0 or later** (`GPL-3.0-or-later`). Consulte [`LICENSE`](LICENSE).
