# Login (OIDC com PKCE pelo navegador)

O app entra com o mesmo login do sistema web. A pessoa autentica nas páginas do servidor (com MFA, SSO, LDAP ou gov.br, conforme a organização), e o app recebe tokens por OIDC. Ele nunca vê a senha e nunca usa webview ([ADR 0002](adr/0002-oidc-loopback.md)).

## Fluxo

1. **Discovery:** `GET <servidor>/.well-known/openid-configuration`.
   - O issuer precisa ser o próprio servidor; compara-se sem barra final.
   - Todos os endpoints precisam estar na mesma origem e em `https`; `http` só vale em loopback.
   - Se o documento listar métodos de PKCE, `S256` tem de estar entre eles.
2. **Preparação** (`OidcClient::begin_login`):
   - abre um listener em `127.0.0.1` numa porta efêmera;
   - gera o verificador PKCE (32 bytes, S256), `state` e `nonce` (32 bytes cada);
   - monta a URL de autorização com `response_type=code`, `client_id=medvita-desktop`, `redirect_uri=http://127.0.0.1:<porta>/callback`, `scope=openid profile email`, `state`, `nonce`, `code_challenge` e `code_challenge_method=S256`.
3. **Navegador:** o app abre essa URL no navegador padrão (`cx.open_url`). A tela mostra "Continue no navegador", com Cancelar e "Abrir o navegador de novo".
4. **Retorno:**
   - O servidor redireciona para `/callback?code=…&state=…`. O listener só aceita esse caminho com o `state` esperado; qualquer outra requisição recebe uma página de erro, e o listener continua esperando.
   - `error=access_denied` encerra com "O login não foi autorizado".
   - Depois de 5 minutos sem retorno, o login expira.
5. **Troca** (`POST` no token endpoint, formulário): `grant_type=authorization_code`, `code`, o mesmo `redirect_uri`, `client_id` e `code_verifier`.
6. **ID token:**
   - Confere `iss`, `aud` (contém `medvita-desktop`), `exp` (com 2 minutos de tolerância) e `nonce`.
   - A assinatura não é verificada, porque o token veio direto do token endpoint por TLS (OIDC Core §3.1.3.7).
   - O perfil vem de `GET /api/v1/me/`.
7. **Sessão:**
   - O refresh token vai para o Gerenciador de Credenciais: serviço `Medvita Desktop`, conta = origem do servidor, persistência **local** (o padrão do Windows, *Enterprise*, faria roaming).
   - O access token fica só em memória.

## Renovação e fim da sessão

- **Renovação:** quando faltam menos de 60 s para o access token vencer, ou depois de um `401 invalid_token` da API, o app usa `grant_type=refresh_token`.
  - A renovação é **uma de cada vez** (mutex), porque o servidor rotaciona o refresh token e não detecta reuso: duas renovações paralelas fariam a segunda receber `invalid_grant`.
  - O refresh novo é gravado no cofre antes de o antigo ser descartado.
- **Refresh recusado** (`invalid_grant`): o cofre é limpo e o app volta ao login com "Sua sessão expirou".
- **Ao abrir:** se o cofre tem refresh token para o servidor, a sessão é restaurada sem navegador.
- **Sair:**
  - revoga o refresh e o access token (RFC 7009, `token_type_hint`), sem esperar o resultado;
  - apaga o cofre de qualquer jeito.
  - A revogação é um POST próprio, porque a biblioteca `oauth2` recusa URL `http` (desenvolvimento em loopback).

## O que o servidor precisa oferecer

Pedido em alaorneto/skeleton#603 e #604 (texto neutro), e em alaorneto/sk_med_core#61:

| Item | Valor |
|---|---|
| Provedor | `allauth.idp.oidc`, ligado no Skeleton |
| Cliente | `medvita-desktop`, público (sem segredo), `skip_consent` (cliente de primeira parte) |
| Redirect | `http://127.0.0.1/callback`, aceitando qualquer porta (RFC 8252 §7.3); o caminho é exato |
| Grants | `authorization_code` com PKCE **S256 obrigatório** (recusar `plain` e PKCE ausente), `refresh_token` |
| Escopos | `openid profile email` (sem `offline_access`: o refresh vem sempre) |
| Refresh | Rotação a cada uso, validade configurada (o padrão do allauth é infinita) |
| Revogação | Endpoint no discovery, aceitando `client_id` sem segredo |
| Invalidação | Troca de senha/MFA, desativação ou `auth_version` invalidam os tokens |
| API | Access token aceito em `/api/v1/` pelas mesmas regras dos tokens `skt_` (`token_methods`, 401 `invalid_token` com `WWW-Authenticate`), e `token_methods` nas views dos apps med |

Registro do cliente pelo operador (comando proposto em #603):

```sh
python manage.py native_client register --client-id medvita-desktop --name "Medvita Desktop" \
  --redirect-uri http://127.0.0.1/callback --skip-consent
```

## Modo desenvolvedor

Só em build de debug (`cfg(debug_assertions)`):

- Aceita um token pessoal `skt_<8 hex>_<segredo>` criado no sistema web e o guarda no cofre como `<servidor> (token pessoal)`.
- O token não se renova.
- Hoje o servidor só o aceita em `me/` e `organizations/`.
- O build de release não contém esse código.

## Testes

`cargo test -p medvita_auth` cobre:

- o fluxo completo contra um servidor OIDC simulado, com o listener de loopback real;
- `state` forjado, `access_denied`, timeout, `nonce` divergente;
- restauração, rotação e renovação única com chamadas paralelas;
- `invalid_grant`, e logout com revogação falhando.

Os testes unitários cobrem PKCE (vetor da RFC 7636), discovery e claims.
