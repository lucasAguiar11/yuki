<img src="app/assets/logo.svg" width="96" alt="Yuki">

# Yuki

[English](README.md) · **Português**

Um console de APIs nativo para macOS, feito para o teclado, escrito em Rust sobre GPUI. Um documento por request (`GET ▸ url`), monoespaçado onde há dado, cor só onde ela significa algo.

Baixe o `Yuki-macos-aarch64.dmg` (Apple Silicon) ou o `Yuki-macos-x86_64.dmg` (Intel) do [último release](https://github.com/lucasAguiar11/yuki/releases/latest) e arraste o Yuki para Aplicativos (depois ele se atualiza sozinho), ou rode a partir do código.

## Rodar

```bash
cargo run -p yuki                        # dev
YUKI_DATA_DIR=/tmp/yk cargo run -p yuki  # diretório de dados isolado
YUKI_LANG=pt-BR cargo run -p yuki        # força um idioma da interface (padrão: o do sistema)
cargo build --release -p yuki
```

## Atalhos

| Tecla | Ação |
|---|---|
| ⌘↵ | enviar request |
| ⌘P / ⌘⇧P / ⌘O | ir para request / comandos / trocar de workspace |
| ⌘N / ⌘⇧N | novo request / nova pasta |
| ⌘E | mostrar/ocultar o editor do request |
| ⌘⇧E | painel de variáveis |
| ⌘⌥E | ambientes e variáveis |
| ⌘⇧G | painel do git |
| ⌘, | configurações: nome do workspace, DNS overrides, variáveis do shell, tamanho das fontes |
| ⌘B | mostrar/ocultar a barra lateral |
| ⌘W / ⌃⇥ / ⌃⇧⇥ | fechar / próxima / aba anterior |
| ⌘S | salvar agora (as edições já salvam sozinhas) |

## Dados

Tudo fica em `~/Library/Application Support/dev.yuki` (troque com `YUKI_DATA_DIR`) como YAML legível: uma pasta por workspace, cada uma um repositório git próprio. Um workspace também pode morar numa pasta sua (um repositório, a pasta de um projeto): ⌘O → **+ abrir pasta…** cria um workspace novo numa pasta vazia ou abre o que já existe nela (um clone, por exemplo); o Yuki só guarda um link para ela. Configurações → workspace mostra onde ele está, e **mover…** leva tudo, `.git` incluso, para outra pasta vazia. Workspaces do Voleeo podem ser importados (copiados, nunca linkados) pela paleta de comandos.

## Variáveis e funções

`{{ NOME }}` na URL, params, headers, body ou auth é resolvido no envio a partir do ambiente ativo (e das variáveis da pasta). Funções começam com `$` e geram um valor novo a cada ocorrência: `{{ $fake.fullName() }}`, `firstName`, `lastName`, `email`, `phone`, `cpf`, `cnpj`, `uuid`, `lorem`, `number`, `password`, `date`, `username`, `company`, `street`, `city`, `zipCode`, `jobTitle`, `url` (todas `$fake.*`), além de `{{ $uuid.v4() }}` e `{{ $timestamp.iso() }}` / `unix` / `unixMs` / `date` / `time`. Digite `{{ $` para escolher uma; a timeline mostra os valores enviados.

## Importar / exportar

⌘⇧P → **Importar coleção…** (ou ⌘O → **+ importar coleção…**) lê um bundle do Voleeo, Yaak, Postman (v2.0/2.1), Insomnia (v4 JSON ou v5 YAML), OpenAPI/Swagger ou Bruno: escolha na árvore o que entra, num workspace novo ou numa pasta do workspace aberto. Auth e headers definidos na coleção ou numa pasta são copiados para cada request. **Exportar workspace…** grava o workspace aberto como Voleeo, Yaak, Postman (ambientes em arquivos ao lado) ou Insomnia v4; valores secretos saem vazios, a não ser que você marque **incluir segredos**.

## MCP

Com o Yuki aberto, agentes de IA podem listar, criar, editar e enviar seus requests (HTTP, WebSocket, gRPC), ambientes, cookies, histórico e snapshots por MCP. O cliente executa `yuki mcp`, uma ponte stdio para o app aberto — sem token nem porta para configurar.

**1. Ache o binário.** App instalado: `/Applications/Yuki.app/Contents/MacOS/yuki`. Do código-fonte: `target/release/yuki` (depois de `cargo build --release -p yuki`). Use o caminho absoluto.

**2. Adicione ao seu cliente** (compilou do código-fonte? troque pelo seu caminho):

| Cliente | Configuração |
|---|---|
| Claude Code | `claude mcp add -s user yuki -- /Applications/Yuki.app/Contents/MacOS/yuki mcp` |
| Codex | `codex mcp add yuki -- /Applications/Yuki.app/Contents/MacOS/yuki mcp` |
| Cursor | `~/.cursor/mcp.json` (ou `.cursor/mcp.json` num projeto), JSON abaixo |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json`, JSON abaixo; reinicie o app |

```json
{ "mcpServers": { "yuki": { "command": "/Applications/Yuki.app/Contents/MacOS/yuki", "args": ["mcp"] } } }
```

**3. Deixe o Yuki aberto.** `● mcp` na barra de status fica verde com o servidor no ar (com o número de agentes conectados) e vermelho se ele não subiu. Peça ao agente "liste meus workspaces do Yuki" para testar.

- `open Yuki once` — o app nunca rodou com esse diretório de dados: abra-o e tente de novo.
- `is Yuki running? no socket` — o app está fechado: abra-o; o cliente reconecta na próxima vez que iniciar.
- Roda o Yuki com `YUKI_DATA_DIR`? Passe o mesmo valor para a ponte: `"env": { "YUKI_DATA_DIR": "/tmp/yk" }` no JSON, `-e YUKI_DATA_DIR=…` (Claude Code) ou `--env YUKI_DATA_DIR=…` (Codex).

Edições do agente aparecem na barra lateral na hora; um request já aberto numa aba mantém o que tinha até você reabri-lo.

## Contribuindo

Issues e pull requests são bem-vindos: veja o [CONTRIBUTING.md](CONTRIBUTING.md) (em inglês). Problemas de segurança vão pelo [SECURITY.md](SECURITY.md), nunca numa issue pública.

## Licença

MIT — veja [LICENSE](LICENSE) e [NOTICE](NOTICE). Os crates do motor (`crates/yuki-*`) derivam do Voleeo. O código é totalmente aberto; o uso comercial dos binários oficiais segue os [termos](docs/legal/TERMS.pt-BR.md).
