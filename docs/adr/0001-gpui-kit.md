# ADR 0001 — Interface com GPUI Kit, versão fixa

- Situação: aceita (2026-11-20)
- Issue: #3

## Contexto

O Medvita Desktop é um app nativo em Rust para Windows (macOS e Linux depois). A escolha de UI foi GPUI, o framework do Zed (renderização na GPU, Direct3D 11 no Windows), com o GPUI Kit da Longbridge (antigo `gpui-component`): mais de 75 componentes, entre eles barra de título, menu lateral, barra de status, tabela virtualizada, formulários, diálogos, notificações e temas.

Em 2026-11:

- o crate `gpui` do crates.io parou na 0.2.2, de 2025-10, com a API antiga;
- o Zed publica o GPUI só no próprio repositório;
- o GPUI Kit 0.10.0 depende dos snapshots `gpui-pre` (=0.3.12, um snapshot do Zed `d2a4fb6`) e é lançado toda semana, com quebras.

## Decisão

- Uma única dependência de UI: `gpui-kit = "=0.10.0"`, com versão **exata**. Tudo vem por `gpui_kit::*`, `gpui_kit::component` e `gpui_kit::assets`; o app não depende de `gpui` nem de `gpuikit`.
- Toolchain `1.98.1` (a do Zed para esse snapshot), fixada em `rust-toolchain.toml`, com o `Cargo.lock` versionado.
- O Dependabot agrupa a família `gpui-*`. Atualizar o kit é uma issue própria: ler o changelog, ajustar o código, `cargo test`, e conferir as telas e o build de release.
- Só `medvita_ui` e o binário dependem do kit. Os demais crates seguem testáveis sem GPU e sobrevivem a uma troca de UI.
- Os textos ficam em `rust-i18n`, a mesma cópia que o kit usa, e `set_locale("pt-BR")` traduz também os componentes.

## Consequências

- Atualizar o kit dá trabalho de propósito: cada versão pode quebrar a API, e o pin exato impede que isso aconteça por acidente.
- O GPUI exige D3D11/DirectComposition. É preciso confirmar cedo que ele roda em PCs modestos de consultório e por Área de Trabalho Remota.
- O kit embute o manifesto do Windows; o app só acrescenta o ícone e a versão (`build.rs`).
- A macro `test` do GPUI vem junto com `use gpui_kit::*`. Módulos de teste importam pelo nome (docs/development.md).
- Alternativas descartadas:
  - Tauri/webview: o app seria HTML de novo, sem ganho sobre o sistema web;
  - egui/iced: menos componentes de app de gestão;
  - GPUI direto do repositório do Zed: exige copiar os `[patch]` do workspace do Zed.
