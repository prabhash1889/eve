# Microsoft Store packaging (MSIX)

The Store build of Eve is an offline-first, trimmed edition:

- **Backend:** `local-parakeet` + `store-edition` Cargo features. No whisper is
  compiled in; Groq stays in the binary but is never the default and is hidden
  in the UI. Defaults: `transcriptionBackend = local`, Parakeet selected.
- **Frontend:** built with `VITE_EVE_EDITION=store`, which hides the Groq key,
  backend pickers, and the Local models catalog (`src/lib/edition.ts`).
- **Model:** NVIDIA Parakeet TDT 0.6B v2 is bundled as an app resource and loaded
  from the read-only resource dir, so transcription works with no download and no
  API key. English only.

## One-time setup

### 1. Fetch the Parakeet weights into the resource dir

The weights are gitignored (too large for the repo). Place these four files at
`src-tauri/resources/models/parakeet-tdt-0.6b-v2/`:

| File | Source (Hugging Face `istupakov/parakeet-tdt-0.6b-v2-onnx`) |
| --- | --- |
| `encoder-model.int8.onnx` | `resolve/main/encoder-model.int8.onnx` |
| `decoder_joint-model.int8.onnx` | `resolve/main/decoder_joint-model.int8.onnx` |
| `vocab.txt` | `resolve/main/vocab.txt` |
| `config.json` | `resolve/main/config.json` |

These are the same URLs/checksums as the in-app catalog in
`src-tauri/src/models.rs` - if you already downloaded Parakeet in the app, copy
them from `%APPDATA%/com.eve.dictation/models/parakeet-tdt-0.6b-v2/`.

### 2. Install the Windows 10/11 SDK

`scripts/build-msix.mjs` needs `makeappx.exe` (the "MSIX Packaging Tools" /
"Windows App Certification Kit" SDK component). `signtool.exe` from the same SDK
is only needed for local sideload testing.

### 3. Reserve the app name in Partner Center

Partner Center > your app > **Product identity** gives you three values. Export
them before packaging (they get written into the manifest):

```sh
export MSIX_IDENTITY_NAME="Publisher.EveVoiceDictation"   # "Package/Identity/Name"
export MSIX_PUBLISHER="CN=ABCD1234-..."                    # "Package/Identity/Publisher"
export MSIX_PUBLISHER_DISPLAY="Your Name"                  # "Publisher display name"
```

## Build

```sh
npm run build:store     # release payload -> src-tauri/target/release (exe + resources + dlls)
npm run build:msix      # pack -> build/<version>/Eve-<version>-store.msix
```

`build:store` refuses to run if the Parakeet files (step 1) are missing.

## Before submitting

- **Replace the tiles.** `build:msix` uses the app icon as a placeholder for
  every tile. The Store validates dimensions - drop correctly-sized PNGs into
  `build/msix-layout/Assets` (StoreLogo 50x50, Square150x150, Square44x44,
  Wide310x150, SplashScreen 620x300) and re-pack, or generate a full tile set.
- **runFullTrust justification.** Eve uses global keyboard/mouse hooks + keystroke
  injection, so the manifest declares the restricted `runFullTrust` capability.
  Partner Center asks you to justify it: it is push-to-talk trigger detection and
  paste-into-focused-app, not keylogging (the hook only flips atomics - see
  `src-tauri/src/hooks.rs`).
- **Privacy policy** is mandatory (the app captures microphone audio). State that
  transcription is on-device by default and that the optional Groq path (not
  exposed in the Store build) would send audio to Groq.

## Local sideload test (optional)

MSIX must be signed to install outside the Store. Create a self-signed cert whose
subject matches `MSIX_PUBLISHER`, sign with `signtool`, trust the cert, then
`Add-AppxPackage`. The Store re-signs on submission, so skip signing when your
only goal is to upload to Partner Center.
