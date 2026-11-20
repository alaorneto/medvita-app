# Desenvolvimento

## Preparar o Windows

| Ferramenta | Para quê | Instalação |
|---|---|---|
| Rustup | Toolchain fixada em `rust-toolchain.toml` (1.98.1, instalada sozinha no primeiro `cargo`) | `winget install Rustlang.Rustup` |
| Visual Studio 2022 Build Tools, "Desenvolvimento para desktop com C++" + Windows 11 SDK | Linker MSVC, `rc.exe` e `fxc.exe` (shaders do GPUI no build de release) | `winget install Microsoft.VisualStudio.2022.BuildTools` |
| CMake | Dependências nativas de algumas crates | `winget install Kitware.CMake` |
| .NET 8 SDK (opcional) | `vpk`, para empacotar e testar o instalador localmente | `winget install Microsoft.DotNet.SDK.8` |
| Python 3.11+ | Scripts do dev-workflow e o servidor simulado | `winget install Python.Python.3.13` |

Habilite caminhos longos no git: `git config --system core.longpaths true`.

O GPUI usa Direct3D 11 e DirectComposition: confira o app cedo em PCs modestos e por Área de Trabalho Remota.

## Comandos

```powershell
cargo run -p medvita                      # abre o app (build de debug, com o modo desenvolvedor)
cargo test --workspace                    # todos os testes
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all
cargo build --release --locked -p medvita # o mesmo build que o CI faz (compila os shaders)
cargo deny check                          # licenças e avisos (cargo install cargo-deny --locked)
```

O CI (`.github/workflows/ci.yml`) roda `fmt`, `clippy`, `test` e `build` no `windows-latest` e `cargo deny` no Linux. O job agregador `CI` só passa se todos passarem.

Variáveis úteis:

| Variável | Efeito |
|---|---|
| `MEDVITA_LOG` | Filtro de log (`info` por padrão; ex.: `medvita_api=debug,info`) |
| `MEDVITA_DEFAULT_SERVER_URL` | Servidor padrão compilado no binário (usado no build de release) |
| `GIT_HASH` | Commit mostrado em Configurações → Sobre |

## Rodar contra um servidor

Na primeira execução o app pede o endereço do servidor; ele fica em `%APPDATA%\Medvita\settings.json`. Para recomeçar do zero, apague esse arquivo e as entradas `Medvita Desktop` do Gerenciador de Credenciais (`cmdkey /list`).

### Servidor simulado

`tools/mock-server/mock_medvita.py` imita o que o shell usa do servidor: discovery OIDC, `authorize` (redireciona direto para o loopback), token com PKCE, revogação, `me/`, `organizations/` (duas organizações) e `apps/active/`. Assim dá para exercitar login, seleção de organização e workspace sem o Skeleton:

```powershell
py -3 tools/mock-server/mock_medvita.py   # http://127.0.0.1:8765
cargo run -p medvita                      # informe http://127.0.0.1:8765 e clique em "Entrar com o navegador"
```

### Skeleton local

Use o PostgreSQL de desenvolvimento de um worktree do skeleton (`tools/dev_postgres.py`), nunca o checkout principal. Enquanto o provedor OIDC não existir no servidor (alaorneto/skeleton#603), use o **modo desenvolvedor**, que só existe em build de debug:

1. Crie um token pessoal no sistema web (Conta → Tokens).
2. Cole o token na tela de login do app.

Com token, só `me/` e `organizations/` respondem até alaorneto/skeleton#604 e alaorneto/sk_med_core#61.

## Textos e idioma

Todo texto visível vem de `crates/medvita_ui/locales/pt-BR.yml` (formato `_version: 2` do `rust-i18n`), lido com `t!("chave")`. As chaves ficam em inglês. O GPUI Kit também é traduzido por `set_locale("pt-BR")`.

Atenção nos testes de `medvita_ui`: `use gpui_kit::*` traz a macro `test` do GPUI, que esconde o `#[test]` do Rust. Em módulos de teste, importe os itens pelo nome, não com `use super::*`.

## Fluxo de trabalho

Issues, branches, commits e PRs seguem o skill dev-workflow ([AGENTS.md](../AGENTS.md#fluxo-de-desenvolvimento)). Versões e changelog são escritos pela automação.
