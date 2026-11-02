# Overture

Aplicativo desktop nativo em **Rust** para Windows voltado à montagem e projeção de letras, Bíblia, mídias, contagem regressiva, placar e avisos em cultos, com saída principal fullscreen acelerada por GPU (`wgpu` + `cosmic-text`), tela de palco, monitor de retorno e controle remoto web via rede local.

## Arquitetura do Workspace

| Crate | Caminho | Responsabilidade |
|---|---|---|
| `overture-core` | `crates/core` | Modelos de domínio (`Song`, `Slide`, `Service`, `Theme`, `Bible`, `Media`), máquina de estado e eventos |
| `overture-storage` | `crates/storage` | Persistência SQLite (`sqlx`), migrações, busca rápida, relatórios e backup/restauração |
| `overture-import` | `crates/import` | Parsers TXT (PT-BR/EN com divisão semântica), OpenSong, OpenLP XML, PowerPoint (`.pptx`) e exportadores |
| `overture-render` | `crates/render` | Composição visual (`cosmic-text` + `wgpu`): fundos, vídeo RGBA, sombra, palco, retorno, placar e cronômetro |
| `overture-media` | `crates/media` | Reprodução de vídeo de fundo, vídeo e áudio via `libmpv` + fundos animados procedurais |
| `overture-output` | `crates/output` | Enumeração de monitores Windows (`Win32`), janelas fullscreen independentes e telemetria de latência |
| `overture-control` | `crates/control` | Servidor HTTP/WebSocket local (`axum`) autenticado por token fixo + Web App responsivo para celular |
| `overture-i18n` | `crates/i18n` | Internacionalização PT-BR e EN com troca dinâmica em tempo real |
| `overture-ui` | `crates/ui` | Interface desktop nativa em Slint (`.slint`) com as 7 telas operacionais |
| `overture` | `crates/app` | Binário principal integrando estado, banco, janelas de saída, servidor remoto e UI |

## Compilação e Testes

```powershell
cargo build --workspace
cargo test --workspace
cargo run -p overture
```

## Instalador Windows (MSI)

```powershell
cargo wix --package overture --nocapture
```

## Licença

Distribuído sob a licença **GNU General Public License v3.0** (`GPL-3.0-or-later`). Veja [`LICENSE`](LICENSE).
