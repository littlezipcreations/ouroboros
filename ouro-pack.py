#!/usr/bin/env python3

import argparse
import struct
import sys
from pathlib import Path

PAGE_SIZE = 4096

RUN_MAGIC = b"RUN1"
RUN_VERSION = 1
RUN_ARCH_AARCH64 = 1

RUN_HEADER_SIZE = 32
RUN_SEGMENT_SIZE = 48
MAX_SEGMENTS = 16

HEADER_FMT = "<4sHHHHQIQ"
SEGMENT_FMT = "<QQQQIIQ"

PF_X = 1
PF_W = 2
PF_R = 4


def align_down(value: int, alignment: int) -> int:
    return value & ~(alignment - 1)


def align_up(value: int, alignment: int) -> int:
    return (value + alignment - 1) & ~(alignment - 1)


def parse_elf64_aarch64(data: bytes):
    if len(data) < 64:
        raise ValueError("ELF file is too small")

    ident = data[:16]

    if ident[:4] != b"\x7fELF":
        raise ValueError("not an ELF file")

    if ident[4] != 2:
        raise ValueError("expected ELF64")

    if ident[5] != 1:
        raise ValueError("expected little-endian ELF")

    e_machine = struct.unpack_from("<H", data, 0x12)[0]

    if e_machine != 183:
        raise ValueError(
            f"expected AArch64 ELF (machine 183), got {e_machine}"
        )

    e_entry = struct.unpack_from("<Q", data, 0x18)[0]
    e_phoff = struct.unpack_from("<Q", data, 0x20)[0]
    e_phentsize = struct.unpack_from("<H", data, 0x36)[0]
    e_phnum = struct.unpack_from("<H", data, 0x38)[0]

    if e_phentsize < 56:
        raise ValueError("ELF program header is too small")

    if e_phnum > MAX_SEGMENTS:
        raise ValueError(
            f"too many program headers: {e_phnum} "
            f"(max {MAX_SEGMENTS})"
        )

    segments = []

    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize

        if off + 56 > len(data):
            raise ValueError(
                f"program header {i} lies outside the ELF"
            )

        p_type, p_flags = struct.unpack_from(
            "<II",
            data,
            off,
        )

        p_offset = struct.unpack_from(
            "<Q",
            data,
            off + 8,
        )[0]

        p_vaddr = struct.unpack_from(
            "<Q",
            data,
            off + 16,
        )[0]

        p_filesz = struct.unpack_from(
            "<Q",
            data,
            off + 32,
        )[0]

        p_memsz = struct.unpack_from(
            "<Q",
            data,
            off + 40,
        )[0]

        p_align = struct.unpack_from(
            "<Q",
            data,
            off + 48,
        )[0]

        if p_type != 1:
            continue

        if p_memsz == 0:
            continue

        if p_filesz > p_memsz:
            raise ValueError(
                f"LOAD segment {i}: filesz > memsz"
            )

        if p_offset + p_filesz > len(data):
            raise ValueError(
                f"LOAD segment {i}: file data lies outside ELF"
            )

        # Original ELF memory range.
        original_start = p_vaddr
        original_end = p_vaddr + p_memsz

        segments.append({
            "vaddr": p_vaddr,
            "mem_end": original_end,
            "filesz": p_filesz,
            "memsz": p_memsz,
            "flags": p_flags,
            "align": p_align,
            "data": data[
                p_offset:p_offset + p_filesz
            ],
        })

    if not segments:
        raise ValueError(
            "ELF contains no PT_LOAD segments"
        )

    # Make sure the actual ELF memory ranges don't overlap.
    original_ranges = sorted(
        (
            segment["vaddr"],
            segment["mem_end"],
        )
        for segment in segments
    )

    for (_, end), (next_start, _) in zip(
        original_ranges,
        original_ranges[1:],
    ):
        if next_start < end:
            raise ValueError(
                "ELF PT_LOAD segments overlap"
            )

    return e_entry, segments


def normalize_segments(segments):
    """
    Convert ELF PT_LOADs into page-aligned regions.

    Two ELF segments can legitimately occupy different portions
    of the same 4 KiB page. Ouroboros cannot map that page twice,
    so merge such regions and OR their permissions.
    """

    normalized = []

    for segment in segments:
        start = align_down(
            segment["vaddr"],
            PAGE_SIZE,
        )

        end = align_up(
            segment["mem_end"],
            PAGE_SIZE,
        )

        offset = segment["vaddr"] - start

        normalized.append({
            "start": start,
            "end": end,
            "flags": segment["flags"],
            "align": max(
                segment["align"] or PAGE_SIZE,
                PAGE_SIZE,
            ),
            "parts": [
                (
                    offset,
                    segment["filesz"],
                    segment["data"],
                )
            ],
        })

    normalized.sort(
        key=lambda segment: segment["start"]
    )

    merged = []

    for segment in normalized:
        if not merged:
            merged.append(segment)
            continue

        previous = merged[-1]

        if segment["start"] >= previous["end"]:
            merged.append(segment)
            continue

        # Page ranges overlap. Merge them.
        previous["end"] = max(
            previous["end"],
            segment["end"],
        )

        previous["flags"] |= segment["flags"]

        previous["align"] = max(
            previous["align"],
            segment["align"],
        )

        base_offset = (
            segment["start"]
            - previous["start"]
        )

        for offset, size, data in segment["parts"]:
            previous["parts"].append((
                base_offset + offset,
                size,
                data,
            ))

    return merged


def build_run(elf: bytes) -> bytes:
    entry, elf_segments = parse_elf64_aarch64(
        elf
    )

    segments = normalize_segments(
        elf_segments
    )

    if len(segments) > MAX_SEGMENTS:
        raise ValueError(
            "too many normalized load segments"
        )

    output = bytearray(
        RUN_HEADER_SIZE
        + RUN_SEGMENT_SIZE * len(segments)
    )

    records = []

    for segment in segments:
        memory_size = (
            segment["end"]
            - segment["start"]
        )

        # Build a zero-filled representation of the
        # complete page-aligned region.
        region = bytearray(memory_size)

        file_size = 0

        for offset, size, data in segment["parts"]:
            end = offset + size

            if end > len(region):
                raise ValueError(
                    "segment data exceeds memory region"
                )

            region[
                offset:end
            ] = data[:size]

            file_size = max(
                file_size,
                end,
            )

        # Keep the .run payload compact by discarding
        # the all-zero tail. The kernel already zeroes
        # the rest of the mapped memory.
        payload = region[:file_size]

        payload_offset = align_up(
            len(output),
            PAGE_SIZE,
        )

        output.extend(
            b"\x00"
            * (payload_offset - len(output))
        )

        output.extend(payload)

        flags = 0

        if segment["flags"] & PF_R:
            flags |= 1

        if segment["flags"] & PF_W:
            flags |= 2

        if segment["flags"] & PF_X:
            flags |= 4

        records.append((
            segment["start"],
            payload_offset,
            len(payload),
            memory_size,
            flags,
            segment["align"],
            0,
        ))

    image_size = len(output)

    header = struct.pack(
        HEADER_FMT,
        RUN_MAGIC,
        RUN_VERSION,
        RUN_ARCH_AARCH64,
        RUN_HEADER_SIZE,
        len(records),
        entry,
        0,
        image_size,
    )

    output[
        :RUN_HEADER_SIZE
    ] = header

    table_offset = RUN_HEADER_SIZE

    for record in records:
        output[
            table_offset:
            table_offset + RUN_SEGMENT_SIZE
        ] = struct.pack(
            SEGMENT_FMT,
            *record,
        )

        table_offset += RUN_SEGMENT_SIZE

    return bytes(output)


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Pack AArch64 ELF into an "
            "Ouroboros .run executable"
        )
    )

    parser.add_argument(
        "elf",
        type=Path,
    )

    parser.add_argument(
        "output",
        type=Path,
    )

    args = parser.parse_args()

    try:
        elf = args.elf.read_bytes()

        run = build_run(elf)

        args.output.write_bytes(run)

    except (
        OSError,
        ValueError,
        struct.error,
    ) as exc:
        print(
            f"ouro-pack: error: {exc}",
            file=sys.stderr,
        )
        return 1

    entry, elf_segments = parse_elf64_aarch64(
        elf
    )

    run_segments = normalize_segments(
        elf_segments
    )

    print(
        f"packed {args.elf} -> {args.output}"
    )

    print(
        f"entry:    {entry:#x}"
    )

    print(
        f"segments: {len(run_segments)}"
    )

    print(
        f"size:     {len(run)} bytes"
    )

    for i, segment in enumerate(
        run_segments
    ):
        flags = "".join(
            c
            for c, bit in (
                ("R", PF_R),
                ("W", PF_W),
                ("X", PF_X),
            )
            if segment["flags"] & bit
        )

        print(
            f"  [{i}] "
            f"VA {segment['start']:#x} "
            f"mem {segment['end'] - segment['start']:#x} "
            f"{flags or '-'}"
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())