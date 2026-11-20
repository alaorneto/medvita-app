# ADR 0002 — Login por OIDC com PKCE e retorno por loopback

- Situação: aceita (2026-11-20)
- Issue: #6 (lado do servidor: alaorneto/skeleton#603, #604; alaorneto/sk_med_core#61)

## Contexto

O app precisa de um login integrado ao sistema web: o mesmo usuário, com MFA, SSO por organização (OIDC, SAML, gov.br) e LDAP, sem tratar senhas. O servidor (Skeleton) tinha só a sessão de navegador e tokens pessoais `skt_`, que são emitidos à mão, não se renovam e duram até 365 dias. A versão instalada do allauth (65.21.1) traz o `allauth.idp.oidc`, ainda desligado, e o AGENTS.md do núcleo proíbe reimplementar OAuth/OIDC.

## Decisão

- OIDC Authorization Code com PKCE S256, `state` e `nonce`, no **navegador do sistema**, com retorno para `http://127.0.0.1:<porta efêmera>/callback`, como manda a RFC 8252 para apps nativos.
- Nunca webview: o navegador do sistema já tem a sessão, os gerenciadores de senha e os fatores da pessoa, e o app não consegue ler o que ela digita.
- Cliente público `medvita-desktop`, escopos `openid profile email`.
- Refresh token rotacionado, guardado no Gerenciador de Credenciais do Windows com persistência local. O access token fica só em memória. A renovação é uma de cada vez.
- O logout revoga os dois tokens (RFC 7009) e limpa o cofre.
- Requisições ao token endpoint e à revogação são POSTs de formulário próprios, sobre o mesmo `reqwest` do app, e não o crate `oauth2`:
  - o `oauth2` 5 traz o `reqwest` 0.12 (o app usa o 0.13);
  - descarta o `id_token` no cliente básico;
  - recusa URL `http` na revogação, o que impede o desenvolvimento em loopback.
  - Os três pedidos são pequenos e estão cobertos por testes contra um servidor simulado.
- `openidconnect` também foi descartado: traz `rsa 0.9`, com aviso de segurança ativo (o `cargo deny` reprovaria). A validação do `id_token` por TLS (OIDC Core §3.1.3.7) basta.
- Um modo desenvolvedor, só em build de debug, aceita um token `skt_` colado, para trabalhar antes de o servidor ter o OIDC.

## Alternativas descartadas

- **Device code (RFC 8628):** exige o mesmo trabalho no servidor e é menos fluido (digitar um código). Fica como opção para ambientes sem navegador.
- **Token pessoal colado em produção:** sem renovação, com vida longa e emitido à mão.
- **Esquema de URL próprio (`medvita://`):** o GPUI não implementa `register_url_scheme` no Windows, e outro app pode registrar o mesmo esquema.

## Consequências

- O servidor precisa de #603, #604 (Skeleton) e #61 (sk_med_core) para o login funcionar de ponta a ponta. Até lá, use o modo desenvolvedor ou o servidor simulado (`tools/mock-server`).
- Antivírus ou firewalls corporativos podem bloquear o loopback. A mensagem de erro diz isso.
- Se o servidor emitir refresh sem validade, uma sessão dura indefinidamente até ser revogada. A validade é pedida em #603.
