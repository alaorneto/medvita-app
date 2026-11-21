# ADR 0003 — Atualização com Velopack, servida pelo servidor do Medvita

- Situação: aceita (2026-11-20)
- Issues: #8, #9 (lado do servidor: alaorneto/skeleton#606)

## Contexto

O app precisa se atualizar sozinho, sem que a pessoa baixe instaladores. O repositório do código é privado, então as GitHub Releases dele não são públicas, e um token embutido no app seria um segredo distribuído. Por decisão do dono, as atualizações vêm do próprio servidor do Medvita.

Opções avaliadas em 2026-11:

- Velopack: instalador por usuário, deltas, canais, MSI opcional, CI documentado;
- cargo-packager-updater: parado desde 2025-07;
- self_update: troca só o binário, sem instalador;
- o atualizador do Zed: interno, não é biblioteca.

## Decisão

- **Velopack 1.2.161** (`velopack` com `public-utils`; `vpk` na mesma versão no CI), com `packId` `Medvita`. Ele instala em `%LocalAppData%\Medvita`, sem administrador.
- **Fonte própria** (`MedvitaServerSource: UpdateSource`) que lê `<servidor>/clients/medvita-desktop/<canal>/`, com o canal Medvita (`stable`/`beta`) no caminho. O download usa o `reqwest` do app (certificados e proxy do Windows), e não o `ureq` do Velopack.
- **Envelope assinado** do feed:
  - Ed25519 sobre o JSON canônico de `{client_id, channel, feed_sha256, signed_at, expires_at}`, com os mesmos primitivos e formatos de chave do índice de releases do núcleo;
  - chave pública commitada no app (`include_str!`);
  - validade de 120 dias, re-assinado todo mês;
  - recusa de `signed_at` mais antigo que o último aceito (anti-rollback).
  - O Velopack confere o SHA-256 de cada pacote contra o feed verificado.
- **Download anônimo:** um cliente que não consegue mais fazer login (por exemplo, porque o protocolo mudou) ainda precisa se atualizar. Nenhum arquivo tem dado pessoal; a integridade vem da assinatura, não do acesso.
- **Versão mínima** por canal, numa política servida pelo servidor (`/api/v1/clients/medvita-desktop/policy`). Abaixo dela, a tela de atualização obrigatória bloqueia o uso.
- **Modelo de pull**, como o deployment agent do núcleo: o CI anexa os pacotes à GitHub Release privada, e cada servidor sincroniza com um token de leitura do operador e verifica antes de publicar.

## Consequências

- O servidor precisa de #606: sincronização, verificação, endpoints de arquivo e de política.
- O dono gera e guarda o par de chaves (docs/releasing.md). Perder a chave privada obriga a distribuir manualmente um app com a chave nova.
- Sem Authenticode, o SmartScreen avisa no primeiro download do instalador. As atualizações seguintes não passam pelo SmartScreen.
- O Velopack 1.2.161 usa o zip 8.6, com avisos de segurança sobre extração de arquivos hostis. Eles estão aceitos no `deny.toml`, porque só pacotes verificados chegam à extração. Revisar quando o Velopack estável passar ao zip 9.
