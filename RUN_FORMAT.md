# Ouroboros `.run` format v1

A `.run` executable is a small AArch64 executable container defined by Ouroboros.
Rust/LLVM still produces an ELF during compilation; `ouro-pack.py` converts the
ELF's `PT_LOAD` segments into `.run`.

## Header (32 bytes, little-endian)

| Offset | Size | Field |
|---:|---:|---|
| 0x00 | 4 | magic = `RUN1` |
| 0x04 | 2 | version = 1 |
| 0x06 | 2 | architecture = 1 (AArch64) |
| 0x08 | 2 | header_size = 32 |
| 0x0A | 2 | segment_count |
| 0x0C | 8 | entry virtual address |
| 0x14 | 4 | flags (currently 0) |
| 0x18 | 8 | total image size |

## Segment (48 bytes each)

| Offset | Size | Field |
|---:|---:|---|
| +0x00 | 8 | virtual address |
| +0x08 | 8 | file offset |
| +0x10 | 8 | bytes present in file |
| +0x18 | 8 | bytes required in memory |
| +0x20 | 4 | flags: R=1, W=2, X=4 |
| +0x24 | 4 | alignment hint |
| +0x28 | 8 | reserved |

The loader allocates enough pages for `memory_size`, clears them to zero, copies
`file_size` bytes, and maps them with the requested read/write/execute permissions.

For v1, segments must start on 4 KiB boundaries and live inside Ouroboros' user VM
range. Up to 16 loadable segments are supported.
