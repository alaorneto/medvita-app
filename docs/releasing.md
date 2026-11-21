# Publicar uma versão

A versão é decidida pelo dev-workflow:

1. Um PR com o label `release` publica, no merge, tudo o que acumulou desde a última tag.
2. O workflow "Versão e release" escreve a versão no `Cargo.toml` e no `Cargo.lock`, atualiza o `CHANGELOG.md`, cria a tag `vX.Y.Z` e a GitHub Release.
3. Ele dispara o `publish.yml` (`release.dispatch_workflow`).

Ninguém edita versão nem changelog à mão.

## Pré-requisitos do dono (uma vez)

1. **Par de chaves de atualização.** Rode, num computador confiável:

   ```powershell
   cargo xtask keygen --private-out C:\caminho\fora\do\repo\medvita-update-private.pem
   ```

   O comando escreve a chave pública em `crates/medvita_updater/keys/update-public.key`; commite-a numa issue `chore`. Depois:

   ```powershell
   gh secret set DESKTOP_SIGNING_KEY --env release < C:\caminho\fora\do\repo\medvita-update-private.pem
   ```

   - Guarde uma cópia offline da chave privada e apague o arquivo.
   - Perder a chave obriga a publicar um app com a chave nova, e as instalações antigas não aceitam essa atualização.
   - Cadastre a mesma chave pública no trust store do servidor (alaorneto/skeleton#606).
2. **Ambiente `release`** no GitHub (Settings → Environments), com o segredo `DESKTOP_SIGNING_KEY` e, se houver, `WINDOWS_SIGN_PARAMS`.
3. **Variável `MEDVITA_DEFAULT_SERVER_URL`** (Settings → Variables): o endereço de produção, compilado no binário.
4. **Authenticode (opcional):** com um certificado de assinatura de código, crie `WINDOWS_SIGN_PARAMS` com os parâmetros do `signtool` (ex.: `/a /f cert.pfx /p … /fd sha256 /tr http://timestamp.digicert.com /td sha256`). Sem ele, o SmartScreen avisa no primeiro download.
5. **No servidor:** registrar o cliente `medvita-desktop` (#603) e configurar a sincronização deste repositório (#606).

## O que o `publish.yml` faz

1. Faz checkout da tag. Falha se a chave pública ainda for `UNSET` ou se a versão do `Cargo.toml` não for a do input.
2. Roda `cargo build --release --locked -p medvita` com `MEDVITA_DEFAULT_SERVER_URL` e `GIT_HASH`, e copia só o `medvita.exe` para `dist/`.
3. Instala o .NET 8 e o `vpk` na mesma versão do crate `velopack`.
4. Roda `vpk download github`, para o delta contra a versão anterior (na primeira versão não há anterior).
5. Roda `vpk pack` (`packId` `Medvita`, canal do Velopack `win`), com Authenticode se `WINDOWS_SIGN_PARAMS` existir.
6. Assina o envelope com `cargo xtask sign-feed` e o confere com `cargo xtask verify-feed`. Uma pré-release (versão com `-`) vai para o canal `beta`; as demais, para `stable`.
7. Anexa à release **só os arquivos desta versão**: os `nupkg`, `Setup.exe`, `Portable.zip`, `releases.win.json`, `assets.win.json`, `releases.win.sig.json` e `RELEASES`.

O `resign-feed.yml` roda todo dia 1º e re-assina o envelope da release mais recente. A validade é de 120 dias, então uma falha pontual não derruba as atualizações.

## Testar a atualização localmente

Precisa do .NET 8 SDK e do `vpk` (`dotnet tool install -g vpk --version 1.2.161`). Use um par de chaves **de teste**, nunca o de produção.

```powershell
$tmp = "$env:TEMP\medvita-update-test"; New-Item -ItemType Directory -Force $tmp | Out-Null
cargo xtask keygen --private-out $tmp\test-private.pem --public-out $tmp\test-public.pem
Copy-Item $tmp\test-public.pem crates\medvita_updater\keys\update-public.key   # NÃO commitar

# Versão 0.0.1, apontando para o servidor de teste:
$env:MEDVITA_DEFAULT_SERVER_URL = "http://127.0.0.1:8766/"
# (troque a versão do workspace para 0.0.1 só nesta cópia de trabalho; não commite)
cargo build --release -p medvita
New-Item -ItemType Directory -Force dist | Out-Null; Copy-Item target\release\medvita.exe dist\
vpk pack --packId Medvita --packVersion 0.0.1 --packDir dist --mainExe medvita.exe --channel win --outputDir $tmp\Releases

# Versão 0.0.2:
# (mude a versão para 0.0.2, recompile, copie para dist\ e rode vpk pack com --packVersion 0.0.2)
cargo xtask sign-feed --feed $tmp\Releases\releases.win.json --channel stable --key-file $tmp\test-private.pem
```

Sirva `$tmp\Releases` no layout `/clients/medvita-desktop/stable/` em `http://127.0.0.1:8766`, por exemplo copiando os arquivos para `$tmp\www\clients\medvita-desktop\stable\` e rodando `py -3 -m http.server 8766 --bind 127.0.0.1 -d $tmp\www`. Guarde o `Medvita-win-Setup.exe` da 0.0.1 antes de empacotar a 0.0.2: o `vpk pack` sobrescreve o arquivo. Depois:

1. **Instalar:** `Medvita-win-Setup.exe --silent` (da 0.0.1). O Velopack instala em `%LocalAppData%\Medvita` e cria atalhos no Menu Iniciar e na Área de Trabalho; no modo silencioso, não abre o app.
2. **Negativo primeiro:** sirva um `releases.win.sig.json` adulterado (por exemplo, com `expires_at` alterado), abra `%LocalAppData%\Medvita\Medvita.exe` e espere 10 s. O log (`%LocalAppData%\Medvita\logs`) registra `update feed refused … bad signature`, e `packages\` continua só com a 0.0.1.
3. **Atualização:** restaure o envelope válido e reabra o app. Em cerca de 10 s, o log mostra o delta 0.0.1 → 0.0.2 baixado e aplicado ao pacote, e a barra de status mostra "A versão 0.0.2 está pronta" com o botão "Reiniciar agora".
4. **Aplicar:** clique em "Reiniciar agora", ou feche e reabra o app (o Velopack aplica na abertura). O log mostra `Medvita starting version="0.0.2"`, e `current\sq.version` diz 0.0.2.

Ao terminar, desinstale com `%LocalAppData%\Medvita\Update.exe uninstall --silent` (remove a pasta, os atalhos e a entrada em Aplicativos). Descarte a cópia de trabalho com a versão e a chave de teste (o mais simples é fazer o teste num `git worktree` e removê-lo) e apague `$tmp`.

Para trocar a versão na cópia de teste, edite o `Cargo.toml` e as entradas do `Cargo.lock` diretamente: o `release.py bump` calcula a versão atual pela última tag e recusa arquivos que já foram alterados.

Roteiro executado em 2026-11-20: delta 0.0.1 → 0.0.2 aplicado na reabertura, envelope adulterado recusado e desinstalação limpa.
