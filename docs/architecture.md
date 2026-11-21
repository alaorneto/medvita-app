# Arquitetura do Medvita Desktop

O Medvita Desktop é um cliente nativo do servidor do Medvita (Skeleton + apps do `sk_med_core`). Ele não tem banco de dados nem regra de negócio própria: tudo passa pela API HTTP do servidor (`/api/v1/`), com o login feito no próprio sistema web.

## Crates

```
crates/medvita            binário: hook do Velopack → logging → configurações → runtime → janela
crates/medvita_ui         telas GPUI Kit, ponte tokio ↔ GPUI, tema, assets, módulos
crates/medvita_api        cliente HTTP tipado da API (sem GPUI)
crates/medvita_auth       OIDC + PKCE + loopback, renovação, revogação, cofre (sem GPUI)
crates/medvita_updater    Velopack, fonte do servidor, envelope Ed25519 (sem GPUI)
crates/medvita_platform   configurações, caminhos, logging, build_info, SecretStore (sem GPUI)
xtask                     chaves e assinatura do feed de atualização
```

As regras de fronteira estão no [AGENTS.md](../AGENTS.md): só `medvita_ui` e o binário dependem de GPUI; só `medvita_api` fala com a API; só `medvita_auth` toca tokens e o cofre; só `medvita_updater` baixa e aplica atualizações. Os crates sem GPUI são testados sem janela nem GPU.

## Inicialização

1. `VelopackApp::build().run()` — primeira linha do `main`. Durante instalação, atualização e desinstalação, o Velopack roda os hooks aqui e pode encerrar o processo; ele também aplica, na abertura, uma atualização já baixada.
2. `AppPaths::discover()` e `logging::init()` — logs diários em `%LOCALAPPDATA%\Medvita\logs` (14 arquivos).
3. Runtime tokio (2 workers) e o cofre (`medvita_auth::default_store`, Gerenciador de Credenciais do Windows).
4. `medvita_ui::run` — `gpui_kit::application()`, `gpui_kit::init`, locale `pt-BR`, janela com `TitleBar::window_options()` e a view `MedvitaRoot`.

Arquivos do app:

| Onde | O quê |
|---|---|
| `%APPDATA%\Medvita\settings.json` | Servidor, tema, canal de atualização, última organização por servidor (só o UUID) |
| `%LOCALAPPDATA%\Medvita\logs\` | Logs (sem corpo, query string ou token) |
| `%LOCALAPPDATA%\Medvita\state\` | `signed_at` do último envelope de atualização aceito, por canal |
| `%LOCALAPPDATA%\Medvita\current\` | Instalação do Velopack (substituída a cada atualização) |
| Gerenciador de Credenciais | Refresh token, serviço `Medvita Desktop`, conta = origem do servidor |

Nenhum dado de paciente vai para disco.

## Ponte tokio ↔ GPUI

O GPUI tem executores próprios; o `reqwest` precisa de tokio. `medvita_ui::bridge` (adaptado do `gpui_tokio` do Zed, ver [NOTICE](../NOTICE)) guarda o runtime num `Global` e oferece:

- `bridged(handle, future)`: roda o future no tokio e devolve um future que qualquer executor aguarda. Ao ser descartado, aborta a tarefa tokio (descartar um `JoinHandle` não abortaria). É assim que "Cancelar" encerra o login: a `Task` do GPUI é descartada, a tarefa tokio é abortada e o listener de loopback fecha.
- `bridged_blocking(handle, work)`: trabalho bloqueante (chamadas ao Velopack) no pool de bloqueio do tokio.

As views usam `cx.spawn_in(window, async move |this, cx| { let r = bridge::bridged(&handle, …).await; this.update_in(cx, …) })`.

## Sessão e telas

`MedvitaRoot` mostra uma tela por fase da máquina de estados `session::SessionController` (pura, testada sem GPUI):

```
Booting ─► NeedsServer ─► (servidor válido) ─► Booting
Booting ─► SignedOut ─► SigningIn ─► LoadingProfile ─► SelectingOrganization ─► Ready
Booting ─► LoadingProfile (sessão restaurada do cofre)
LoadingProfile ─► Unavailable (rede, manutenção) ─► LoadingProfile (Tentar de novo)
qualquer fase logada ─► SignedOut (sessão expirada ou Sair)
```

Por cima das fases há duas sobreposições: Configurações e "Atualização obrigatória".

- Barra de título (`TitleBar` do kit): logo, organização atual (clicar troca, quando há mais de uma), nome da pessoa, Configurações, Sair.
- Barra de status (`StatusBar`): servidor, estado da atualização, versão.

## Workspace e módulos

`Workspace` é o espaço logado: `Sidebar` com os módulos e a view do módulo ativo. Um módulo implementa `modules::Module`:

```rust
pub trait Module: 'static {
    fn id(&self) -> &'static str;
    fn title(&self) -> SharedString;
    fn icon(&self) -> IconName;
    fn app_label(&self) -> Option<&'static str>; // app do servidor que o módulo exige
    fn build_view(&self, context: &ModuleContext, window: &mut Window, cx: &mut App) -> AnyView;
}
```

Para acrescentar um módulo:

1. Escreva a view (e, se for o caso, os endpoints em `medvita_api` sob `client.org(id).app("<label>")`).
2. Implemente `Module` e inclua-o em `modules::registry()`.
3. Coloque os textos em `crates/medvita_ui/locales/pt-BR.yml`.

O módulo aparece quando o servidor informa o app como ativo na organização (`GET /api/v1/o/<org>/apps/active/`, alaorneto/skeleton#605). Em servidores que ainda não têm esse endpoint, todos aparecem. Hoje existem Início e os placeholders Pacientes, Agenda, Prontuário e Financeiro.

## API

`medvita_api::ApiClient` conhece as convenções do servidor (`docs/api.md` do Skeleton):

- **Erros:** `{code, detail, fields?}` vira `ApiError`, sempre com o `X-Request-ID`. Validação, conflito, 412/428, 429 com `Retry-After`, 503 `maintenance` com `ends_at`, `token_not_allowed`, `mfa_required` e `organization_suspended` têm variantes próprias.
- **Listas:** `Page<T>` (`{count,page,page_size,results}`) e `CursorPage<T>` (`{results,next_cursor}`).
- **Concorrência e idempotência:** `Versioned<T>` traz o `ETag`, que volta em `WriteOptions::if_match`. `IdempotencyKey` vira o cabeçalho `Idempotency-Key`.
- **Escopo:** `client.org(uuid).app("med_patients").get("patients/", &query)` monta `/api/v1/o/<uuid>/apps/med_patients/patients/`. O caminho nunca sai do prefixo `/api/v1/` do servidor.
- **Token:** num `401 invalid_token`, o cliente pede uma renovação ao `AccessTokenProvider` e repete a requisição uma única vez. Um segundo 401 vira `Unauthorized`, e a tela volta ao login.
- **Privacidade:** o log de cada requisição traz só método, caminho sem query, status, duração e `X-Request-ID`. Erros de decodificação não citam o conteúdo da resposta.

## Login e atualização

Ver [authentication.md](authentication.md) e [updates.md](updates.md).
