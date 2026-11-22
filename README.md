# Medvita Desktop

Aplicativo de desktop do **Medvita**, o sistema de gestão médica da Katuá Tech construído sobre o Skeleton e os apps do `sk_med_core`. Escrito em Rust com [GPUI Kit](https://gpui-kit.com/).

- Usa só a API HTTP do servidor do Medvita.
- Entra pelo login do sistema web: o app abre o navegador, a pessoa entra como de costume (inclusive com MFA e SSO) e volta ao app.
- Atualiza-se sozinho, a partir do servidor do Medvita.
- Plataforma: Windows 10 ou mais novo.

## Estado

Em construção: o esqueleto e a infraestrutura estão no epic [#1](https://github.com/alaorneto/medvita-app/issues/1). As telas de negócio (pacientes, agenda, prontuário, financeiro) vêm depois.

## Documentação

| Documento | Conteúdo |
|---|---|
| [AGENTS.md](AGENTS.md) | Regras do repositório para pessoas e agentes |
| `docs/architecture.md` | Crates, fronteiras, fluxo de inicialização |
| `docs/development.md` | Como preparar o Windows, compilar, testar e rodar |
| `docs/authentication.md` | Login OIDC com PKCE e o que o servidor precisa oferecer |
| `docs/updates.md` | Protocolo de atualização entre o app e o servidor |
| `docs/releasing.md` | Como uma versão é empacotada, assinada e publicada |

## Licença

Proprietária. Veja [LICENSE](LICENSE).
