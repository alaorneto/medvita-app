# Instruções para agentes: Medvita Desktop (sk_med_app)

## O que é este repositório

O aplicativo de desktop do Medvita, o sistema de gestão médica construído sobre o Skeleton (`alaorneto/skeleton`) e os apps do `alaorneto/sk_med_core` (`med_patients`, `med_scheduling`, `med_records`, `med_finance`). É um app Rust com GPUI + GPUI Kit (`gpui-kit`, versão fixa). Ele:

- fala com o sistema web **só pela API HTTP** (`/api/v1/`);
- autentica a pessoa pelo **login do próprio sistema web**: OIDC com PKCE no navegador do sistema e retorno por loopback;
- se **atualiza sozinho** (Velopack), a partir do servidor do Medvita.

Plataforma suportada hoje: Windows. O código fica portável.

## Contrato do servidor

A API é do servidor; este app não a redefine. Autenticação, organização no caminho (`/api/v1/o/<org-uuid>/…`), formato de erro, paginação, ETag/`If-Match`, `Idempotency-Key`, limites e manutenção estão na documentação do Skeleton, que vale aqui integralmente:

- <https://github.com/alaorneto/skeleton/blob/v0.1.0/docs/api.md>
- <https://github.com/alaorneto/skeleton/blob/v0.1.0/docs/access-control.md>

As APIs de cada app (pacientes, agenda, prontuário, financeiro) estão no `docs/` do `sk_med_core`. Em divergência entre este repositório e o servidor, vale o servidor. A referência sobe junto com a mudança que passa a depender de uma versão nova.

## Ordem de leitura

1. `README.md` e este arquivo.
2. `docs/architecture.md`, `docs/authentication.md` e `docs/updates.md`.
3. `docs/development.md` antes de rodar qualquer comando.

## Fronteiras do repositório

| Crate | Papel | Pode depender de |
|---|---|---|
| `crates/medvita` | Binário: hook do Velopack, logging, configurações, runtime e janela | todos |
| `crates/medvita_ui` | Telas GPUI Kit, ponte tokio ↔ GPUI, assets, tema, módulos | `medvita_api`, `medvita_auth`, `medvita_updater`, `medvita_platform` |
| `crates/medvita_api` | Cliente HTTP tipado da API | `medvita_platform` |
| `crates/medvita_auth` | OIDC, PKCE, loopback, renovação, revogação, cofre de credenciais | `medvita_api` (tipos), `medvita_platform` |
| `crates/medvita_updater` | Velopack, fonte do servidor, envelope Ed25519 | `medvita_platform` |
| `crates/medvita_platform` | Configurações, caminhos, logging, `build_info`, trait `SecretStore` | nenhum crate do workspace |
| `xtask` | Tarefas de build (chaves, assinatura do feed) | `medvita_updater` |

- Só `medvita_ui` e o binário dependem de `gpui-kit`. Os demais crates são testáveis sem GPU.
- Só `medvita_api` faz requisições à API. Só `medvita_auth` lê ou grava tokens e o cofre. Só `medvita_updater` baixa e aplica atualizações.
- Nenhuma tela chama `reqwest` ou tokio direto: a rede passa pela ponte de `medvita_ui`, e a thread principal do GPUI nunca bloqueia.

## Regras do app

- **Dados de saúde e pessoais:** nada de paciente em disco (sem cache offline, sem arquivo temporário) até uma decisão registrada em ADR.
  - Logs levam só método, rota sem query string, status, duração e `X-Request-ID`. Nunca corpo, cabeçalho `Authorization`, token, nome, CPF ou conteúdo clínico.
  - Mensagens de erro mostram o `request_id` para suporte, não detalhes internos.
- **Tokens:**
  - O refresh token só fica no Gerenciador de Credenciais do Windows, com persistência local, nunca roaming. O access token só fica em memória.
  - Tokens em `secrecy::SecretString`.
  - A renovação é uma de cada vez (o servidor rotaciona o refresh token).
- **Login:** só pelo navegador do sistema, com PKCE S256, `state` e `nonce`. **Proibido webview** para login. `http` só em loopback (`127.0.0.1`, `::1`, `localhost`); fora disso, `https`.
  - O login por token pessoal `skt_` existe só em build de debug.
- **Atualização:** um feed só é usado depois de verificado o envelope Ed25519 com a chave pública commitada. A chave privada nunca entra no repositório, nem em log nem em artefato.
- **Textos de interface** em pt-BR, sempre por `t!()` com chave em inglês (`crates/medvita_ui/locales/pt-BR.yml`). Código, identificadores, comentários e commits em inglês; documentação, issues e PRs em pt-BR.
- **Dependências:**
  - `gpui-kit` e `velopack` com versão exata (`=x.y.z`); o `vpk` do CI acompanha o `velopack`.
  - Biblioteca de terceiros nova só com issue própria.
  - `cargo deny check` tem de passar.
- **Dinheiro e datas** seguem o servidor: centavos inteiros, ISO 8601 com fuso, exibição no fuso local.

## Fluxo de desenvolvimento

O fluxo deste repositório é o skill `dev-workflow` (`.claude/skills/dev-workflow/`, configurado em
`.github/dev-workflow.toml`). Carregue o skill e siga-o (no Claude Code, pela ferramenta Skill; em
outro agente ou à mão, lendo `.claude/skills/dev-workflow/SKILL.md`) sempre que:

- entrar no modo de planejamento ou escrever um plano: ele termina com a seção "Itens do GitHub";
- um plano for aceito: as issues e o epic são criados antes de qualquer edição;
- for começar, sem plano, qualquer mudança em arquivo versionado (código, testes, documentação, CI,
  dependências);
- for criar ou atualizar uma issue ou um epic, criar branch, commitar ou abrir PR;
- for decidir se um PR publica versão e com que nível, ou mexer em release, changelog ou tag;
- for fechar uma issue.

Perguntas e investigações sem mudança de arquivo não precisam do skill. Valem sempre, mesmo sem ele:
nenhuma mudança versionada sem issue; nunca `--no-verify`; ninguém edita a versão nem o changelog à mão.

Próprio deste repositório:

- Issues e PRs têm a seção "Segurança e contrato do AGENTS.md". O label `security` marca impacto em autenticação, tokens, dados de saúde, atualização assinada ou segredos.
- A versão do app é a de `[workspace.package]` no `Cargo.toml` e das entradas dos crates no `Cargo.lock`, escrita pela automação de release.
- Um PR por epic.
- Antes de abrir PR: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`.

## Proibido

- Webview para login, ou guardar senha da pessoa.
- Gravar dado de paciente em disco ou em log.
- Desligar a verificação do envelope de atualização, mesmo em debug.
- Commitar segredo (chave privada, token, certificado).
- Chamar a API fora de `medvita_api`, ou tocar tokens fora de `medvita_auth`.
- Trocar o pin exato do `gpui-kit` por faixa (`^`, `~`).
