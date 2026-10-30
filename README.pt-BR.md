<img src="app/assets/logo.svg" width="96" alt="Yuki">

# Yuki

[English](README.md) · **Português**

Um console de APIs nativo para macOS, feito para o teclado, escrito em Rust sobre GPUI: uma alternativa open source ao Postman. Um documento por request (`GET ▸ url`), monoespaçado onde há dado, cor só onde ela significa algo. Site: [lucasaguiar11.github.io/yuki](https://lucasaguiar11.github.io/yuki/).

Baixe o `Yuki-macos-aarch64.dmg` (Apple Silicon) ou o `Yuki-macos-x86_64.dmg` (Intel) do [último release](https://github.com/lucasAguiar11/yuki/releases/latest) e arraste o Yuki para Aplicativos (depois ele se atualiza sozinho), ou rode a partir do código.

**Primeira abertura.** O Yuki ainda não é assinado com um certificado Apple Developer nem notarizado, então o macOS se recusa a abri-lo da primeira vez ("A Apple não pôde verificar…"). Libere uma vez, na mão: tente abri-lo e depois vá em Ajustes do Sistema → Privacidade e Segurança → **Abrir Mesmo Assim**. Ou pelo terminal:

```bash
xattr -dr com.apple.quarantine /Applications/Yuki.app
```

As atualizações instaladas pelo próprio Yuki não precisam desse passo.

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
| ⌘⇧O | abrir um workspace em nova janela (ou ⌘-clique / botão direito num workspace da tela inicial); um workspace já aberto em outra janela vem para a frente |
| ⌘N / ⌘⇧N | novo request / nova pasta |
| ⌘E | mostrar/ocultar o editor do request |
| ⌘⌥L | resposta ao lado / abaixo do request |
| ⌘⇧E | painel de variáveis |
| ⌘⌥E | ambientes e variáveis |
| ⌘⇧G | painel do git |
| ⌘, | configurações: geral (todos os workspaces), workspace, aparência |
| ⌘B | mostrar/ocultar a barra lateral |
| ⌘W / ⌃⇥ / ⌃⇧⇥ | fechar / próxima / aba anterior |
| ⌘S | salvar agora (as edições já salvam sozinhas) |

## Dados

Tudo fica em `~/Library/Application Support/dev.yuki` (troque com `YUKI_DATA_DIR`) como YAML legível: uma pasta por workspace, cada uma um repositório git próprio. Um workspace também pode morar numa pasta sua (um repositório, a pasta de um projeto): ⌘O → **+ abrir pasta…** cria um workspace novo numa pasta vazia ou abre o que já existe nela (um clone, por exemplo); o Yuki só guarda um link para ela. Configurações → workspace mostra onde ele está, e **mover…** leva tudo, `.git` incluso, para outra pasta vazia. Configurações → geral → **pasta dos workspaces** coloca todos numa pasta sua (uma subpasta para cada, com o nome do workspace); imports novos também caem lá. Workspaces do Voleeo podem ser importados (copiados, nunca linkados) pela paleta de comandos.

## Configurações

**Geral** vale para todos os workspaces nesta máquina (nunca vai para o git); **workspace** vale para um. Quando os dois definem algo, eles se combinam assim:

| Configuração | Geral + workspace |
|---|---|
| user-agent | o do workspace (o header `User-Agent` dele) vence; vazio usa o do geral; vazio lá envia `Yuki`. Um header no request ou na pasta vence os dois. |
| certificados | cada workspace escolhe **como o geral**, **aceitar inválidos** ou **verificar**; só **como o geral** segue a opção do geral |
| dns | as linhas se somam; para o mesmo host, a linha do workspace vence |
| env do shell | os nomes se somam |

Geral também define como os workspaces abrem: **nesta janela** ou **numa nova janela** (⌘O e a lista da tela inicial; ⌘⇧O e ⌘-clique fazem o outro). Agentes enviando via MCP seguem as mesmas regras.

## Variáveis e funções

`{{ NOME }}` na URL, params, headers, body ou auth é resolvido no envio a partir do ambiente ativo (e das variáveis da pasta). Funções começam com `$` e geram um valor novo a cada ocorrência: `{{ $fake.fullName() }}`, `firstName`, `lastName`, `email`, `phone`, `cpf`, `cnpj`, `uuid`, `lorem`, `number`, `password`, `date`, `username`, `company`, `street`, `city`, `zipCode`, `jobTitle`, `url` (todas `$fake.*`), além de `{{ $uuid.v4() }}` e `{{ $timestamp.iso() }}` / `unix` / `unixMs` / `date` / `time`. Digite `$` para escolher uma (a lista mostra um exemplo de cada); a aba **log** da resposta mostra os valores enviados.

**Valores de uma resposta.** Clique num valor de uma resposta JSON (o `access_token` de um login, por exemplo) e use `→ variável` (⌥↵): dê um nome e o ambiente ativo ganha `TOKEN = {{ $response.body(requestId="…", selector="$.data.access_token") }}`. Funciona como ponteiro: enviar qualquer request que use `{{ TOKEN }}` envia o login antes e pega o campo dessa resposta nova, então o token está sempre atual (uma origem que voltaria em ciclo é lida da última resposta). A barra abaixo da resposta lista as variáveis que a leem (✕ remove).

Sem variável: em qualquer campo de template — inclusive o auth de uma pasta ou do workspace — digite `$response.` e o autocomplete lista os campos da última resposta JSON de cada request (`login › $.accessToken`); continue digitando para filtrar, `$response.login.token` (nome do request, depois o campo). Só `$` oferece `$response.…` no topo, e `$token` sozinho também acha os campos. Escolher um insere o mesmo ponteiro `$response.body(…)`.

**Pastas e o workspace.** Clique numa pasta para abri-la numa aba: auth, variáveis, headers e notas valem para todos os requests dela e das subpastas (a seta só expande/recolhe). Um request com auth `herdar` envia a auth da pasta mais próxima, senão a do workspace; headers se somam workspace → pastas → request, e o mais próximo vence quando o nome se repete. Botão direito na área vazia da barra lateral → **auth e headers do workspace…** edita o nível do workspace.

**Variações.** Um request, vários jeitos de enviar — tokens de `admin` e de `user`, por exemplo — em vez de duplicar. O chip `base ▾` à esquerda das abas do request lista as variações: **nova variação…** copia o que está na tela (URL, params, headers, body, auth) para uma variação nova e a deixa ativa. Cada variação é independente: editar uma nunca muda a base nem as outras, e editar a base nunca muda as variações. Escolha uma variação para enviá-la. A variação ativa fica salva no arquivo do request, então o time compartilha a escolha. Via MCP, `request.send` aceita `variation` (nome ou id). Exportar para Postman, Insomnia ou Yaak transforma cada variação num request próprio logo depois da base (`me · admin`); um bundle do Voleeo as mantém como variações.

**Comentários no body JSON.** `// …` e `/* … */` num body JSON ficam salvos no request mas saem no envio (junto com uma vírgula que sobrar antes de `}` ou `]`), então dá para comentar um campo.

**Configurações do request e carga.** A aba `config` de um request define o **timeout** (ms, da conexão ao último byte do body; vazio = sem limite) e os **redirects** (seguir ou não — desligado, o 3xx é a resposta — e o máximo a seguir, 10 por padrão); ficam salvos no arquivo do request. **Carga** envia o request N vezes, P de cada vez, e mostra a contagem por status, os erros, tempo mín / médio / p50 / p95 / máx e requests por segundo. O request é resolvido uma vez, então todo envio leva os mesmos valores (`$fake` inclusive). Uma resposta vai para o histórico (o primeiro 2xx); a aba **log** dela lista cada envio na ordem em que terminou (número, status ou erro, tempo) — ▸ abre o request e a resposta daquele envio (headers, bodies) — e depois o resumo da carga. Fica guardada com a resposta, então o histórico a reabre.

## Importar / exportar

⌘⇧P → **Importar coleção…** (ou ⌘O → **+ importar coleção…**) lê um bundle do Voleeo, Yaak, Postman (v2.0/2.1), Insomnia (v4 JSON ou v5 YAML), OpenAPI/Swagger ou Bruno: escolha na árvore o que entra, num workspace novo ou numa pasta do workspace aberto. Auth e headers definidos na coleção ou numa pasta são copiados para cada request. **Exportar workspace…** grava o workspace aberto como Voleeo, Yaak, Postman (ambientes em arquivos ao lado) ou Insomnia v4; valores secretos saem vazios, a não ser que você marque **incluir segredos**.

Um comando `curl …` colado na barra de URL (o **Copy as cURL** do DevTools, a documentação de uma API) vira o request aberto: método, URL e query, headers, body e basic auth. Copie um em qualquer lugar e volte ao Yuki: **▸ importar cURL** aparece na barra de status e o transforma num request novo com um clique (✕ dispensa).

## MCP

Com o Yuki aberto, agentes de IA podem listar, criar, editar e enviar seus requests (HTTP, WebSocket, gRPC), ambientes, cookies, histórico e snapshots por MCP. O cliente executa `yuki mcp`, uma ponte stdio para o app aberto — sem token nem porta para configurar.

**1. Ache o binário.** App instalado: `/Applications/Yuki.app/Contents/MacOS/yuki`. Do código-fonte: `target/release/yuki` (depois de `cargo build --release -p yuki`). Use o caminho absoluto.

**2. Adicione ao seu cliente** — o mais fácil é pelo app: clique em `● mcp` na barra de status (ou configurações → agentes (mcp)), escolha o cliente e copie o trecho, já com o caminho deste binário. À mão (compilou do código-fonte? troque pelo seu caminho):

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
