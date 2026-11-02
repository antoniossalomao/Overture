# Planejamento v2: app de projeção para cultos em Rust

Status: planejamento. Nenhuma importação feita.

## 1. Decisões fechadas

| Tema | Decisão |
|---|---|
| Linguagem | Rust |
| Plataforma | Desktop nativo, Windows |
| Interface | Slint |
| Base de referência | OpenLP (só referência, sem portar código Python) |
| Licença | GPL-3.0 |
| Saída no projetor | Fundo de vídeo + texto |
| Mídias (vídeo/áudio) | Fase 4 |
| Idioma | PT-BR e EN, com troca nas configurações |
| Placar | Manual, controlado pelo operador (fase 4) |
| Monitor de palco | Slide atual, próximo slide e cronômetro |
| PowerPoint | Importação na fase 7 |
| API de controle | Token fixo na configuração, só rede local |
| Backup | SQLite + pasta de mídias |
| Fonte de importação de músicas | Arquivos TXT na pasta do usuário |

## 2. Escopo

**Objetivo:** app desktop nativo para montar e projetar letras, mídias e avisos em cultos.

**Inspiração de fluxo (sem copiar código, telas ou textos):** Holyrics e ProPresenter.

**Fora da v1:** app móvel nativo, sincronização em nuvem, multiusuário.

**Licenças:** antes de copiar qualquer trecho do OpenLP, verificar o `LICENSE` em `C:\Overture\OpenLP`. Com GPL-3.0 do projeto, o código copiado precisa ser compatível.

## 3. Requisitos funcionais

### Biblioteca
- Músicas: título, artista, autor, grupo, notas, histórico de execução.
- Bíblia: versões, busca, favoritos, histórico.
- Mídias: imagem, vídeo, áudio.
- Temas e fundos: cor, fonte, sombra, alinhamento, fundo imagem ou vídeo.

### Culto
- Playlist por culto ou evento: músicas, mídias, avisos, Bíblia, contagem regressiva.
- Playlist temporária, que some ao fechar.
- Escala de integrantes, com contagem de vezes escalado.
- Arquivar e restaurar itens.

### Apresentação
- Saída principal em monitor escolhido, fullscreen, com fundo de vídeo e texto.
- Tela de palco: slide atual, próximo slide e cronômetro.
- Monitor de retorno com tema próprio.
- Atalhos de teclado configuráveis.
- Modos "tela preta" e "logo".
- Contagem regressiva e placar manual (fase 4).
- Apresentação automática com duração por slide.

### Importação e exportação
- Importação de TXT (UTF-8 e ISO-8859-1), com as regras da seção 4.
- Importação de OpenSong e OpenLP XML (fase 2 em diante); PowerPoint na fase 7.
- Exportação para TXT, JSON próprio e backup completo.

### Controle remoto
- API HTTP local com token fixo.
- Interface web responsiva para celular, na mesma rede.
- Eventos em tempo real via WebSocket.

### Relatórios
- Músicas mais tocadas por período e dia da semana.
- Escala por integrante.

## 4. Importação de letras

### Fonte
- Arquivos TXT na pasta do usuário, no formato atual: linha 1 é o título, linha 2 é o crédito do site, depois uma linha em branco e a letra.
- O importador remove o cabeçalho e usa o título do nome do arquivo quando a linha 1 não estiver padronizada.
- O nome do arquivo segue o padrão `Artista - Título.txt`.

### Regras de slide
- Máximo de 2 linhas por slide.
- Estrofes separadas por linha em branco nunca são misturadas.
- A divisão preserva o sentido: quebra em pontuação (`,` `;` `.` `!` `?`) ou em limite de frase, não no meio de uma expressão.
- Nunca separar artigo ou preposição do substantivo que acompanha (ex.: "de", "da", "no", "pra" ficam junto da palavra seguinte).
- Quando não houver quebra natural, dividir no espaço mais próximo do meio da linha.
- Slides com quebra forçada ficam marcados `quebra_forcada` para revisão manual.

### Validação
- Cada música importada gera um relatório: total de slides, slides com quebra forçada, linhas acima do limite, letra curta (menos de 250 caracteres).
- Duplicatas exatas são removidas, e duplicatas de título e artista são marcadas.

### Licenças
- Arquivos do próprio usuário entram sem restrição.
- Hinos de domínio público entram sem restrição.
- Repositórios de terceiros, como `damarals/letras`, só entram após verificar a licença do repositório e de cada música.
- O app nunca busca nem inclui letras protegidas automaticamente.
- Cada música guarda a origem (`src`) para auditoria.

## 5. Arquitetura

Workspace Rust:

| Crate | Responsabilidade |
|---|---|
| `core` | Modelos de domínio: Song, Slide, Service, Theme, Bible, Media. Sem UI. |
| `storage` | SQLite via `sqlx`. Migrações. Backup. |
| `import` | Parsers TXT, OpenSong, OpenLP XML, PowerPoint. Divisão de slides. |
| `render` | Composição: texto, sombra, fundo. Layout com `cosmic-text`. Saída com `wgpu`. |
| `media` | Vídeo de fundo e mídias com `libmpv`. |
| `output` | Janelas por monitor. Fullscreen via Win32. |
| `control` | API HTTP/WebSocket com `axum`. Token fixo. |
| `i18n` | Textos em PT-BR e EN, com troca em tempo real. |
| `app` | Binário principal. |
| `ui` | Interface em Slint (`.slint`). |

Fluxo: UI envia comandos ao `core`; o `core` atualiza o estado; `output` e `control` assinam os eventos de estado.

## 6. Modelo de dados

- `songs(id, title, artist, author, group_id, notes, source, created_at)`
- `song_slides(id, song_id, order, text, forced_break)`
- `groups(id, name)`
- `themes(id, name, json)`
- `services(id, date, name)`
- `service_items(id, service_id, order, kind, ref_id, params)`
- `bible_versions(id, abbr, name, lang, license)`
- `bible_verses(version_id, book, chapter, verse, text)`
- `media(id, path, kind, duration)`
- `execution_log(id, service_item_id, executed_at)`
- `team_members(id, name)`
- `schedules(id, service_id, member_id, role)`

## 7. Interface

Telas:

1. **Controle:** culto à esquerda, biblioteca no centro, pré-visualização e slide atual à direita.
2. **Biblioteca:** músicas, Bíblia, mídias, temas, com busca.
3. **Editor de música:** texto com divisão automática, pré-visualização e marcação de quebras forçadas.
4. **Temas:** editor com pré-visualização ao vivo, inclusive fundo de vídeo.
5. **Monitores:** saída principal, palco e retorno.
6. **Escala e relatórios.**
7. **Configurações:** idioma, token da API, backup.

## 8. Fases

| Fase | Entrega | Critério de aceite |
|---|---|---|
| 0 | Spike de saída: janela fullscreen num monitor escolhido, com texto | Abre no monitor certo, em fullscreen, sem atraso perceptível |
| 1 | Banco SQLite, biblioteca, importação TXT com regras da seção 4 | Importa as músicas da pasta sem erro; busca em menos de 100 ms |
| 2 | Serviço, playlist, editor de slides, temas | Montar culto de 10 itens e navegar entre slides |
| 3 | Tela de palco e monitor de retorno | Dois monitores com conteúdos diferentes |
| 4 | Mídias: vídeo de fundo, vídeo, áudio, contagem regressiva, placar | Vídeo de fundo sem travar; contagem sincronizada na saída |
| 5 | Bíblia | Busca e exibição de versículos com versão escolhida |
| 6 | API de controle e interface web | Celular controla o próximo slide na rede local |
| 7 | Exportação, backup, instalador MSI, i18n completo | Instalação limpa no Windows; desinstalação pelo Configurações; restauração de backup |

## 8.1 Instalador Windows

**Requisitos:**
- Instalador MSI gerado com `cargo-wix` (WiX Toolset).
- Aparece em Configurações > Aplicativos > Aplicativos instalados, com opção de desinstalar.
- Atalho no menu Iniciar e opção de fixar na barra de tarefas.
- Ícone próprio em `.ico` (várias resoluções: 16, 32, 48, 256 px).
- Identificador de aplicativo (AppUserModelID) definido, para o Windows agrupar a janela corretamente na barra de tarefas.
- Versão e fabricante preenchidos nos metadados do MSI.
- Upgrade in-place: nova versão substitui a anterior, sem desinstalar manualmente (`UpgradeCode` fixo).
- Dependências empacotadas: `libmpv` e runtime necessário para `wgpu`.

**Fora da v1:** assinatura de código (sem ela, o Windows SmartScreen mostra aviso). Pode ser adicionada depois com certificado.

**Testes de instalação:** máquina limpa, instalação, abertura pela barra de tarefas, atualização de versão, desinstalação pelas Configurações, verificação de que nenhum arquivo ficou.

## 9. Testes

- Unitários: parsers, divisão de slides, quebra de linha, modelos.
- Integração: banco e importação com arquivos de exemplo.
- Regressão: conjunto fixo de 20 músicas com slides esperados.
- Manual: saída em monitor, vídeo de fundo, controle pelo celular.
- Latência: medir atraso entre comando e exibição na saída.

## 10. Riscos

- Latência do vídeo de fundo na saída.
- Múltiplos monitores e DPI no Windows.
- Quebra de linha em português, com fonte escolhida pelo usuário.
- Divisão de slides que altere o sentido em casos atípicos (mitigado pelo marcador de revisão).
- Licenças de Bíblia e de conteúdo importado.
- Escopo: priorizar fases 0 a 3 antes de expandir.

## 11. Pendências para completar

1. Bíblias incluídas e licença de cada uma.
2. Layout padrão dos temas (fontes, tamanhos, posições).
3. Nome final do app.

## 12. Próximo passo

Fase 0: criar o workspace Rust e o spike de saída em monitor.
