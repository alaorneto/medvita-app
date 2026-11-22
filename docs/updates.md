# Atualização automática

O app usa o [Velopack](https://docs.velopack.io/) para instalar e se atualizar, a partir do **servidor do Medvita**. Este documento é o contrato entre o app e o servidor; a implementação do lado do servidor está em alaorneto/skeleton#606. O porquê das escolhas está na [ADR 0003](adr/0003-velopack-servidor.md).

## Visão geral

```
publish.yml ──► GitHub Release (privada) do medvita-app ──► sincronização do servidor ──► /clients/... ──► app
             nupkg, Setup.exe, releases.win.json,            (token do operador; verifica
             releases.win.sig.json                             o envelope e os SHA-256)
```

- O download é **anônimo**, para que uma instalação velha demais para fazer login ainda consiga se atualizar. Nenhum arquivo tem dado pessoal.
- A confiança vem da assinatura, não do canal: o app só usa um feed cujo envelope Ed25519 confere com a chave pública embutida, e o Velopack confere o SHA-256 de cada pacote contra esse feed.

## Arquivos no servidor

Base: `<servidor>/clients/medvita-desktop/<canal>/`, com canal `stable` ou `beta`.

| Caminho | Conteúdo |
|---|---|
| `releases.win.json` | Feed do Velopack, com os bytes exatos gerados pelo `vpk pack` |
| `releases.win.sig.json` | Envelope assinado do feed |
| `<arquivo>` | Pacotes (`Medvita-<v>-full.nupkg`, `Medvita-<v>-delta.nupkg`) e instaladores (`Medvita-win-Setup.exe`, `Medvita-win-Portable.zip`) |

Regras:

- Os arquivos ficam fora de `/api/v1/`, que só aceita JSON e limita o corpo a 1 MiB.
- O servidor serve os bytes sem alteração, com `Content-Length` e `ETag`. Redirecionar para um storage é permitido.
- 404 para o que não existe.
- O app recusa nomes de arquivo com `/`, `\`, `..`, `%`, `?` ou `#`.

## Envelope

```json
{
  "payload": {
    "client_id": "medvita-desktop",
    "channel": "stable",
    "feed_sha256": "<SHA-256 em hexadecimal dos bytes de releases.win.json>",
    "signed_at": "2026-11-20T12:00:00+00:00",
    "expires_at": "2027-03-20T12:00:00+00:00"
  },
  "signature": "<base64 padrão da assinatura Ed25519 do JSON canônico de payload>"
}
```

- **JSON canônico:** o mesmo do núcleo, `json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode("utf-8")` (`skeleton/system/packaging/primitives.py`). Em Rust: `medvita_updater::canonical`. O teste `tests/python_vector.rs` traz um envelope assinado pelo Python do Skeleton e verificado em Rust.
- **Chave:** Ed25519, chave pública em PEM SubjectPublicKeyInfo (`crates/medvita_updater/keys/update-public.key`), chave privada em PEM PKCS#8 (segredo `DESKTOP_SIGNING_KEY`). Os formatos são os de `initialize_keys` do núcleo.
- **Validade:** 120 dias. O workflow `resign-feed.yml` re-assina o envelope da release mais recente todo mês.

O app recusa o feed, e nada é baixado, quando:

- não há envelope;
- o envelope não é JSON válido ou tem campos a mais;
- a assinatura não confere com a chave embutida;
- `client_id` ou `channel` não são os esperados;
- `feed_sha256` não é o hash dos bytes recebidos;
- `expires_at` já passou;
- `signed_at` é anterior ao último envelope aceito neste computador, para o mesmo canal (`%LOCALAPPDATA%\Medvita\state\update-envelope.json`). Isso impede servir um feed antigo depois de um novo.

## Política de versão

`GET <servidor>/api/v1/clients/medvita-desktop/policy?version=<v>&os=win&channel=<canal>` (anônimo) responde:

```json
{"latest_version": "0.3.0", "minimum_version": "0.2.0", "update_required": false}
```

- O app considera a atualização obrigatória quando `update_required` é verdadeiro ou quando a versão atual é menor que `minimum_version` (SemVer).
- Nesse caso mostra a tela "Atualização obrigatória", que bloqueia o uso, baixa a versão nova e oferece "Reiniciar agora".
- 404 significa servidor sem política: nada é bloqueado.

## Comportamento do app

- **Desligado:** em `cargo run` (fora do instalador: `NotInstalled`) ou com a chave `UNSET`. O motivo aparece na barra de status e em Configurações → Sobre.
- **Verificação:** a primeira, 10 s depois de abrir; depois, a cada 6 horas; e em "Verificar atualização agora".
- **Download:** em segundo plano, com o progresso na barra de status.
- **Pronto para usar:**
  - a notificação "A versão X foi baixada" aparece;
  - "Reiniciar agora" aplica e reabre;
  - sem isso, o Velopack aplica sozinho na próxima abertura.
- **Canal:** Estável ou Beta, em Configurações. Ao voltar de Beta para Estável, o app aceita uma versão menor (`AllowVersionDowngrade`).
- **Rede:** o download usa o `reqwest` com o repositório de certificados e o proxy do Windows, sem tempo total (pacotes grandes), mas com tempo máximo entre leituras.

## Publicação

O passo a passo de quem publica está em [releasing.md](releasing.md).
