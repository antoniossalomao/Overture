# Plano: deixar a tela principal do Overture igual à do Holyrics

Status: planejamento. Nada implementado nesta etapa.

## 1. Objetivo e escopo

Replicar a estrutura e o fluxo da tela principal do Holyrics 2.30 (estúdio de louvor) no Overture:
mesmas regiões, mesma ordem, mesmas proporções e os mesmos blocos de controle.

Fora de escopo:
- Busca de letras na internet. A tela de pesquisa do Holyrics mostra um aviso de direitos autorais. No Overture a
  tela pode existir só com a busca local, sem busca automática de letras protegidas (regra do planejamento v2).
- Cópia de ícones, textos ou marcas do Holyrics. Os ícones usados são da biblioteca Lucide (já no projeto).
- Paleta idêntica. O tema continua Grafite (Orion); o que muda é a estrutura.

## 2. Medidas de referência (print do Holyrics, 1917 px de largura)

| Região | Posição x (px) | Largura (px) | Altura (px) |
|---|---|---|---|
| Barra de menus | 0–1917 | 1917 | 36 |
| Barra de ações (Ir para Bíblia, título, Temas) | 0–1917 | 1917 | 34 |
| Biblioteca (abas, busca, lista) | 0–318 | 318 | até a base |
| Lista de slides (toolbar, histórico, cartões) | 326–997 | 671 | até a base |
| Coluna de prévia (player + saída 16:9 + rodapé) | 1004–1553 | 549 | até a base |
| Playlist (Mídia/Anotações, "Letras (Culto)", Todos/Selecionadas) | 1560–1815 | 255 | até a base |
| Faixa de temas (miniaturas verticais) | 1822–1917 | 95 | até a base |

Proporção aproximada das colunas: biblioteca 17 %, slides 35 %, prévia 29 %, playlist 13 %, temas 5 %.

## 3. Situação atual do Overture (tela principal)

Medidas atuais (proporções): biblioteca 330 px, slides 520 px, centro (prévia) ~730 px, playlist 240 px, temas ~80 px.

Diferenças:
1. **Prévia no lugar errado:** a saída 16:9 fica no centro, com a lista de slides separada à esquerda. No Holyrics ela fica
   na coluna da direita, abaixo do player.
2. **Sem painel de player:** o Holyrics tem um painel "VLC Player" com faixa atual, transporte e volume. O Overture não tem.
3. **Barra da lista de slides:** o Overture usa botões com texto ("Repetir") e não tem a linha de tom/BPM nem os ícones
   de ação do Holyrics (reproduzir, próximo, adicionar, marcar, editar, grade, música, lixeira).
4. **Histórico:** no Overture é um cabeçalho "Histórico (0)" ocupando espaço fixo. No Holyrics é um cabeçalho
   recolhível, centralizado sobre a lista.
5. **Rodapé da prévia:** "Salvar Tema para Música", miniatura do tema e "Configurações de exibição" estão no centro. No
   Holyrics ficam embaixo da coluna da direita.
6. **Biblioteca:** no Holyrics a busca tem seta de opções ao lado e as linhas mostram título + artista em cinza na mesma linha.
   O rodapé tem checkbox, ícone de confirmar, ícone de lista e menu. O Overture tem "0 itens / Arquivados".
7. **Abas:** o Holyrics tem abas com texto e dois ícones extras à direita (informação e arquivo). O Overture tem um "+" no lugar.
8. **Playlist:** no Holyrics cada item tem título em branco e artista em cinza, ícones de remover à direita e a faixa
   inferior "Todos / Selecionadas" com seta. O Overture tem "+ Cont. / + Mídia" em cima e a faixa embaixo com "Todos / Ret.".
9. **Faixa de temas:** no Holyrics é uma coluna estreita com miniaturas reais de tamanho uniforme. No Overture a coluna
   existe, mas fica vazia quando não há temas carregados e o botão "Novo" está nela.
10. **Barra superior:** no Holyrics o botão "Ir para Bíblia" tem ícone de livro e fica à esquerda, com o título centralizado.
    No Overture o ícone do menu de mensagens e o botão de Bíblia ficam juntos, e o título fica no meio. Falta o
    indicador de "Temporário / Culto" com seta, igual ao do Holyrics.

## 4. Mudanças por fase

Cada fase tem arquivos, critério de aceite e verificação.

### Fase 1 — Proporções e estrutura das colunas (sem mudar comportamento)
- Arquivos: `crates/ui/ui/main.slint` (propriedades `col1-width`, `col2-width`, `col4-width` e o contêiner central).
- Mudanças:
  - Biblioteca 318 px, slides o restante até 35 %, prévia ~29 %, playlist 13 %, temas 5 %.
  - Mover a prévia/saída 16:9 para a coluna da direita (acima da playlist, como no Holyrics: prévia no topo da coluna
    central-direita, rodapé embaixo).
- Aceite: a prévia fica na coluna da direita; as colunas mantêm larguras mínimas; nada corta em 1280×800.
- Verificação: render headless em 1280×800, 1600×900 e 1920×1080 (harness `snap`).

### Fase 2 — Barra de ações e cabeçalho
- Arquivos: `main.slint` (linha de ações acima das colunas).
- Mudanças:
  - Botão "Ir para Bíblia" com ícone de livro à esquerda (já existe; alinhar altura e raio).
  - Título centralizado com o item ativo (já existe).
  - Seletor "Temporário / Culto" com seta à direita, e botão "Temas".
  - Remover o ícone de mensagens solto e colocá-lo dentro do grupo de alertas.
- Aceite: barra com 34 px de altura; título nunca sobrepõe os botões.

### Fase 3 — Lista de slides (coluna central)
- Arquivos: `main.slint`, `components.slint` (`UiSlideCard`), `crates/app/src/main.rs` (estado da seleção).
- Mudanças:
  - Toolbar de ícones, sem texto: reproduzir, próximo, adicionar, marcar, editar, grade, música, lixeira.
    Substituir o botão "Repetir" por um ícone com tooltip; o texto "Repetir / Parar no fim" vai para o menu do ícone.
  - Linha de tom e BPM ("Fm | BPM: 87") entre a toolbar e a lista. Os campos vêm dos metadados da música; se não houver,
    mostrar só o título.
  - Cartões de slide: número + texto em 2–4 linhas, fundo azul no selecionado, check à direita no slide ativo.
  - Histórico recolhível centralizado sobre a lista (cabeçalho com seta).
- Aceite: seleção azul no slide ativo; histórico recolhe e expande; toolbar só com ícones.

### Fase 4 — Coluna da direita: player e prévia
- Arquivos: `main.slint`, `crates/app/src/main.rs`, `crates/media` (estado de áudio).
- Mudanças:
  - Painel "Player" no topo da coluna: nome da faixa atual, tempo, transporte (anterior, play/pausa, próximo, parar),
    volume e ícone de saída. Liga na reprodução de áudio do item "Áudio" da biblioteca.
  - Abaixo, a prévia 16:9 da saída principal (com rótulo "Principal | Palco | Retorno" embaixo, como as abas atuais).
  - Rodapé da coluna: "Salvar Tema para Música" (switch + miniatura do tema) e "Configurações de exibição".
- Aceite: a prévia mostra o slide ativo; o player toca e para um áudio da biblioteca; os controles do rodapé
  mantêm o comportamento atual.
- Dependência: o player de áudio precisa de comandos novos no `MediaEngine` (play/pause/stop/volume em `motion`/`media`).
  Hoje o engine só toca fundo e vídeo.

### Fase 5 — Biblioteca (coluna esquerda)
- Arquivos: `main.slint`, `crates/ui/ui/components.slint`.
- Mudanças:
  - Abas com texto e dois ícones à direita (informação e arquivo). O "+" vai para a barra de busca.
  - Busca com seta de opções ao lado (filtros: título, artista, letra — as checkboxes já existem no backend).
  - Linhas: título em branco e artista em cinza na mesma linha, com ícone de estrela/favorito à direita.
  - Rodapé: checkbox "Arquivados", ícone de confirmar, ícone de lista e menu de ações.
- Aceite: linha com título + artista; rodapé com os 4 controles; filtros abrem pela seta.

### Fase 6 — Playlist (coluna da direita, lista de culto)
- Arquivos: `main.slint`.
- Mudanças:
  - Abas "Mídia | Anotações" no topo, com ícones.
  - Cabeçalho "Letras (Culto)" com ícone de lista e botão "+".
  - Itens: título branco, artista cinza, ícone de remover à direita; arraste já existe (ordem do culto).
  - Rodapé com "Todos / Selecionadas" e seta, filtrando a lista.
- Aceite: itens com dois níveis de texto; filtro "Selecionadas" mostra só itens marcados.

### Fase 7 — Faixa de temas (coluna mais à direita)
- Arquivos: `main.slint`, `crates/app/src/main.rs` (miniaturas dos temas).
- Mudanças:
  - Miniaturas verticais de tamanho uniforme, com o tema ativo destacado.
  - Botão "Novo" sai da faixa e vai para o cabeçalho da playlist.
  - Quando não há temas, mostrar um texto "Nenhum tema" em vez de uma coluna vazia.
- Aceite: faixa sempre preenchida (ou com o texto de vazio); clique numa miniatura aplica o tema.

### Fase 8 — Bíblia (mesmo padrão)
- Arquivos: `crates/ui/ui/bible_view.slint`.
- Mudanças:
  - Pills de versão como dropdowns (ACF, ACF, NVT no Holyrics) em vez de botões fixos; hoje são pills, então é
    só ajustar o visual.
  - Grade de livros e capítulos: manter as cores por categoria (já correspondem ao Holyrics).
- Aceite: troca de versão pelo dropdown; grade com as mesmas proporções.

## 5. Ordem de execução

1. Fase 1 (estrutura) — base para todas as outras; sem ela as medidas não batem.
2. Fase 2 (barra) e Fase 5 (biblioteca) — são visíveis e independentes.
3. Fase 3 (lista de slides) — depende da fase 1.
4. Fase 4 (player e prévia) — depende da fase 1 e de comandos novos de áudio.
5. Fases 6 e 7 (playlist e temas) — independentes entre si.
6. Fase 8 (Bíblia) — por último.

## 6. Riscos

- **Player de áudio (fase 4):** é a parte mais grande. Pode virar uma fase própria, antes da prévia.
- **Largura mínima:** em 1280 px, 35 % para slides e 29 % para prévia podem ficar apertados. Definir larguras
  mínimas e recolher a faixa de temas abaixo de 1366 px.
- **Medidas do Holyrics:** foram tiradas de um print em escala 100 %. Em outra escala de tela, a proporção se mantém,
  mas os pixels mudam. Conferir com a referência em 1920×1080.

## 7. Verificação

- Render headless (harness `snap`) em 1280×800, 1600×900 e 1920×1080 para cada fase, comparado com o print.
- `cargo test --workspace` a cada fase (não deve quebrar a lógica de estado).
- Checklist no Windows, a cargo do usuário: prévia atualiza ao trocar de slide, tema aplica pela faixa, player toca
  e para, histórico recolhe, filtros da biblioteca funcionam.

## 8. Decisões tomadas

1. **Prévia 16:9 na coluna da direita**, como no Holyrics (acima do rodapé de temas e configurações).
2. **Player de áudio nesta rodada** (fase 4 completa, com os comandos de áudio no engine).
3. **Textos em português e o nome "Overture" nas abas**, mantendo a estrutura do Holyrics.

Com essas decisões, a fase 4 passa a ser a maior do plano. Ela pode ser dividida em duas entregas
(prévia na direita primeiro, player depois) se o cronograma apertar.
