<img src="app/assets/logo.svg" width="96" alt="Yuki">

# Yuki

[English](README.md) · **Português**

Um console de APIs nativo para macOS, feito para o teclado, escrito em Rust sobre GPUI. Um documento por request (`GET ▸ url`), monoespaçado onde há dado, cor só onde ela significa algo.

Pré-lançamento: ainda não há `.app` empacotado; rode a partir do código.

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

## MCP

Com o Yuki aberto, agentes de IA podem listar, criar, editar e enviar seus requests por MCP. Aponte o cliente para o binário `yuki` com o argumento `mcp` — ele liga o stdio ao app aberto (sem token para copiar):

```json
{ "mcpServers": { "yuki": { "command": "/path/to/target/release/yuki", "args": ["mcp"] } } }
```

`● mcp` na barra de status fica verde com o servidor no ar (com o número de agentes conectados) e vermelho se ele não subiu. Edições do agente aparecem na barra lateral na hora; um request já aberto numa aba mantém o que tinha até você reabri-lo.

## Contribuindo

Issues e pull requests são bem-vindos: veja o [CONTRIBUTING.md](CONTRIBUTING.md) (em inglês). Problemas de segurança vão pelo [SECURITY.md](SECURITY.md), nunca numa issue pública.

## Licença

MIT — veja [LICENSE](LICENSE) e [NOTICE](NOTICE). Os crates do motor (`crates/yuki-*`) derivam do Voleeo. O código é totalmente aberto; o uso comercial dos binários oficiais segue os [termos](docs/legal/TERMS.pt-BR.md).
