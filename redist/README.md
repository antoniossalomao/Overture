# Overture Runtime Redistributables (`redist/`)

Este diretório armazena as dependências nativas empacotadas junto ao instalador `.msi` do **Overture**:

1. **`libmpv-2.dll`** — Motor nativo de decodificação de vídeo/áudio para fundos em movimento, vídeos sincronizados e trilhas de áudio (`crates/media`). Carregado dinamicamente em tempo de execução por `overture.exe` a partir do diretório do executável ou `redist/libmpv-2.dll`. Caso ausente em ambientes de desenvolvimento, o Overture ativa automaticamente o gerador procedural de fundos em movimento (`motion://aurora`, `motion://ocean`, `motion://warm`).
2. **`d3dcompiler_47.dll`** — Runtime auxiliar DirectX / HLSL utilizado pelo backend DirectX 12 do `wgpu` em máquinas Windows limpas.
3. **`mpv.conf`** — Perfil padrão de baixa latência e decodificação assistida por hardware (`hwdec=auto-safe`) usado pelo Overture.

> **Nota para Git:** Arquivos `.dll` binários grandes são ignorados no controle de versão (`.gitignore`) e baixados/preparados automaticamente antes da geração do MSI via `powershell -ExecutionPolicy Bypass -File scripts/build-msi.ps1`.
