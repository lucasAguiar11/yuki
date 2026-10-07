# AGENTS.md — Yuki

Guia para agentes neste repositório. **Mantenha abaixo de 200 linhas**: ao adicionar regra, enxugue outra.

## Produto

Yuki é um **console de APIs nativo** (Rust + GPUI via `gpui-kit`), inspirado no Voleeo e no Zed — não uma cópia de nenhum dos dois. Direção visual "Console": documento por request (linha de prompt `GET ▸ url`), monoespaçado onde há dado, cor só onde carrega significado, keyboard-first. Cópia da UI em **pt-BR**, minúscula e curta (`sem resposta`, `▸ request ⌘E`).

Pré-lançamento: sem shims de migração para formatos internos. **Exceção:** dados em disco (YAML de workspace/request/env) são do usuário — mantenha legíveis (serde `default`/`rename`), nunca corrompa.

## Licença e modelo de negócio (decidido — não mudar sem pedido)

- **Código: MIT, 100% aberto.** Mesmo modelo do Yaak: o que se cobra é o *uso comercial dos binários oficiais* (termos em `docs/legal/TERMS.md`), nunca funcionalidades. Proibido: paywall de feature, módulo fechado, trocar a licença do código, bloquear o app quando o trial acabar.
- `LICENSE` traz o copyright do Voleeo (obrigatório: `crates/yuki-*` derivam dele) e o do Yuki; proveniência em `NOTICE`. Nunca remova a linha do Voleeo.
- Strings visíveis dizem **Yuki** (UI, User-Agent padrão, `serverInfo` do MCP, páginas OAuth, mensagens de import). "Voleeo" só para origem/import e o formato `VoleeoBundle`.
- Dependência nova: nada GPL/LGPL/AGPL (confira com `cargo metadata`). Binário distribuído precisa levar as licenças de terceiros (`cargo about generate`).

## Layout

```
app/              binário `yuki` (GPUI) — única camada que conhece gpui
  themes/yuki-console.json   paleta + syntax (única fonte de cor)
  src/main.rs     actions!, keybindings, tema, janela
  src/workspace.rs  root view `Yuki`: workspaces, abas, painéis, paletas, ações
  src/editor.rs   `RequestEditor` (estado/lógica de uma aba)
  src/document.rs Render do RequestEditor: prompt, meta line, editor de request
  src/body.rs     painel body: select de tipo + input por tipo (raw, form/multipart, binary, graphql)
  src/table.rs    tabela nome/valor (headers e campos form/multipart); última linha sempre em branco
  src/complete.rs autocomplete de `{{ VAR }}` (ao digitar `{{` ou ⌃space) em URL, células e editores de código
  src/code_vars.rs "badge" de `{{ VAR }}` nos editores de código (decorations; `EditorState` não aceita tokens)
  src/response.rs faixa de status, body/headers, histórico (●●●)
  src/var_view.rs chips `{{ VAR }}` + tooltip; src/vars.rs modelo/tokenização (puro)
  src/vars_panel.rs painel ⌘⇧E;  src/sidebar.rs, tabs.rs, chrome.rs, palette.rs
  src/items.rs    CRUD da árvore (criar/duplicar/excluir); moving.rs drag & drop + "mover para…"; rename.rs renomear inline
  src/backend.rs  ponte para o motor: tokio runtime, load/save/send, import Voleeo
  src/draft.rs, tree.rs, ui.rs  helpers puros
crates/yuki-*     motor herdado do Voleeo (MIT), sem gpui/tauri
  yuki-core tipos+erros · yuki-storage YAML · yuki-http executor · yuki-mcp ApiBackend+resolve
  yuki-crypto AES-GCM/keychain · yuki-auth/oauth/cookies/ws/grpc/import/export/git
```

**Invariantes:** `crates/` nunca depende de gpui. Toda a pipeline de envio passa por `ApiBackend::send_http` (`crates/yuki-mcp/src/api/request/send.rs`) — app e MCP usam a mesma; nunca reimplemente env/pasta/auth/cookies/histórico na UI. Resolução de variáveis: `yuki_mcp::resolve` (`load_env_vars_from`, `apply_folder_vars`, `resolve_str`, `is_identifier`).

## Dados

- Diretório: `~/Library/Application Support/dev.yuki` (override `YUKI_DATA_DIR`). Mesmo layout do Voleeo (`workspaces/{id}/req_*.yaml`, `envs-local/{id}/`, `keys/{id}.key`, `responses-local/`).
- Import: `backend::import_voleeo` **copia** (nunca link) workspaces, envs locais e key files; pula `.git` e ids existentes.
- Salvar só escreve se mudou (`save_request_if_changed`); ids de headers são preservados por posição (`draft::text_to_headers`) — fresh ids = diff fantasma no git do usuário.
- Keychain service: `yuki`. Chave de arquivo é o fallback (e o que o import traz).

## Comandos

```bash
cargo run -p yuki                       # dev (deps em opt-level 3, app em 0)
YUKI_DATA_DIR=/tmp/yk cargo run -p yuki # dados isolados p/ testes
cargo build --release -p yuki
cargo test -p yuki -p yuki-mcp          # unitários (app: draft, vars)
cargo clippy -p yuki && cargo fmt --all
```

Não há `.app` empacotado nem CI ainda.

## Código

- KISS/YAGNI; menor diff que resolve a causa. Comentário só para o não-óbvio (o *porquê*).
- `.rs` em `app/src`: alvo ≤ 250 linhas; dividir por responsabilidade (`workspace.rs` já passou — divida ao tocar).
- Cores **só** via `cx.theme()` (tokens do JSON) e `ui::{status_color, method_color, faint}`. Nunca hex no código. Dado (URL, headers, métodos, JSON, rodapé) usa `theme.mono_font_family`.
- Testes: só lógica pura com risco real (`draft.rs`, `vars.rs`). UI se verifica rodando (skill `yuki-visual-smoke`).

## GPUI / gpui-kit — armadilhas já pagas

Detalhes e padrões na skill `yuki-gpui`. Resumo do que custou caro:

1. **Borrow de `cx`:** `let theme = cx.theme()` empresta `cx`; copie as cores (`Hsla` é `Copy`) antes de qualquer `cx.listener`/`&mut cx`, ou construa os filhos mutáveis antes de pegar `theme`.
2. **I/O fora da UI:** YAML via `cx.background_spawn`; rede via `Backend::save_and_send` (runtime tokio próprio). A task tokio nunca captura o `Backend` (dropar runtime dentro dele panica) — só `Arc<ApiBackend>`.
3. **Foco:** sem elemento focado nenhum keybinding dispara. Depois de trocar workspace/fechar aba, chame `focus_active`.
4. **⌘↵ em inputs:** o `Input` consome `secondary-enter`; existe binding extra `cmd-enter` com contexto `"Input"` registrado depois (vence). Mantenha.
5. **Resizable:** nunca `.visible(false)` em painel de `h_resizable` — painel oculto deixa tamanho velho e a borda "pula" ao arrastar. Omita o painel e use id de grupo por combinação (`main-{sidebar}-{vars}`). Painéis com tamanho: `.flex_none()` + `size_range`.
6. **Hover em tokens de input:** não guarde hover em estado da view (o re-render reseta o tracking e o hover morre). Use `.tooltip(...)` no elemento do chip.
7. **Paleta (`Command`):** a lista ocupa `max_h` inteiro; a altura é calculada por `matched_count` (`palette::list_height`) e a view só re-renderiza quando a contagem muda (observer com guarda — notify incondicional = loop).
8. **Template chips:** `vars::content` ao carregar e `chip_templates!` em cada `InputEvent::Change`; o texto salvo continua literal (`{{ BASE }}`). `EditorState` (código) recusa tokens: lá o badge é `RangeDecoration` + `TextDecoration` (`code_vars.rs`); dentro de string JSON a cor da sintaxe vence a do nome.
9. **Teclas de popup sobre `Input`:** o `Input` consome ↑↓↵⇥esc. Para vencer, binding com contexto `"VarComplete > Input"` (mesma profundidade, registrado depois) e `key_context("VarComplete")` no ancestral só enquanto o popup está aberto. ⌘space é do Spotlight — nunca chega ao app.
10. **Células single-line:** `InputState` remove quebras de linha; `draft::cell_value` devolve o valor salvo se a célula não foi tocada (evita corromper valor multi-linha).

## Fluxo de entrega

1. Ler a área + skill relevante. 2. Implementar no menor diff. 3. `cargo build -p yuki`, `clippy`, testes. 4. **Rodar o app** e verificar a superfície alterada (skill `yuki-visual-smoke`) — compilar não é prova. 5. Limpar fixtures/scripts temporários.

Commits: Conventional Commits, mensagens em português, sem co-autoria de IA. Nunca commitar/pushar sem pedido.
