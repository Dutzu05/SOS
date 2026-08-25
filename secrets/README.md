# Shared auth secret

`sos_core/src/auth.rs` embeds `shared.key` into the binary at compile time
via `include_bytes!`, so `sos_cli` and `sos_fw` agree on the same secret
without either of them baking a literal into version control.

The file is git-ignored on purpose — it must never be committed. Every
person or CI job that builds this workspace needs their own local copy at
`secrets/shared.key`, and it must be exactly `TOKEN_LEN` bytes (see
`sos_core/src/auth.rs`; currently 16).

Generate one:

```bash
# from the repo root
head -c 16 /dev/urandom > secrets/shared.key
```

On Windows PowerShell:

```powershell
[System.IO.File]::WriteAllBytes("secrets/shared.key", (New-Object byte[] 16 | %{ Get-Random -Max 256 }))
```

Whatever generates it, both the ground-station build (`sos_cli`) and the
firmware build (`sos_fw`) must be compiled with the **same** `secrets/shared.key`
file present — if they diverge, authentication will always fail. There is
no default: the build fails with a clear "file not found" error until this
file exists.
