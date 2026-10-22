#!/usr/bin/env python3
"""Populate an OuroFS raw disk image with host-built files.

Current OuroFS format: 4096-byte blocks, 128-byte inodes, 64-byte directory
entries, 12 direct blocks per file/directory. This script intentionally keeps
those limits aligned with src/filesystem.rs.
"""

from __future__ import annotations

import argparse
import os
import struct
import sys
from pathlib import Path

BLOCK_SIZE = 4096
SUPERBLOCK_BLOCK = 0
INODE_BITMAP_BLOCK = 1
BLOCK_BITMAP_START = 2
BLOCK_BITMAP_BLOCKS = 4
INODE_SIZE = 128
INODE_COUNT = 1024
INODE_TABLE_BLOCK = 6
INODE_TABLE_BLOCKS = 32
DATA_START_BLOCK = 38
ROOT_INODE = 0
SUPERBLOCK_MAGIC = 0x4F55524F
SUPERBLOCK_VERSION = 1
INODE_FREE = 0
INODE_FILE = 1
INODE_DIRECTORY = 2
DIR_ENTRY_SIZE = 64
DIR_NAME_MAX = 56
MAX_FILE_SIZE = 12 * BLOCK_SIZE
MAX_BLOCKS = BLOCK_BITMAP_BLOCKS * BLOCK_SIZE * 8


class OuroFSError(Exception):
    pass


class OuroFSImage:
    def __init__(self, file_obj):
        self.file = file_obj
        self.total_blocks = os.fstat(file_obj.fileno()).st_size // BLOCK_SIZE
        if os.fstat(file_obj.fileno()).st_size % BLOCK_SIZE:
            raise OuroFSError("image size must be a multiple of 4096 bytes")
        if not (DATA_START_BLOCK < self.total_blocks <= MAX_BLOCKS):
            raise OuroFSError(
                f"image must contain 39..{MAX_BLOCKS} 4 KiB blocks "
                f"(currently {self.total_blocks})"
            )
        self.mounted = False

    def read_block(self, number: int) -> bytearray:
        if not 0 <= number < self.total_blocks:
            raise OuroFSError(f"block number outside image: {number}")
        self.file.seek(number * BLOCK_SIZE)
        data = self.file.read(BLOCK_SIZE)
        if len(data) != BLOCK_SIZE:
            raise OuroFSError(f"short read from block {number}")
        return bytearray(data)

    def write_block(self, number: int, data: bytes | bytearray) -> None:
        if not 0 <= number < self.total_blocks:
            raise OuroFSError(f"block number outside image: {number}")
        if len(data) != BLOCK_SIZE:
            raise OuroFSError("internal error: block write must be exactly 4096 bytes")
        self.file.seek(number * BLOCK_SIZE)
        written = self.file.write(data)
        if written != BLOCK_SIZE:
            raise OuroFSError(f"short write to block {number}")

    @staticmethod
    def bit_is_set(bitmap: bytes | bytearray, index: int) -> bool:
        return (bitmap[index // 8] & (1 << (index % 8))) != 0

    @staticmethod
    def set_bit(bitmap: bytearray, index: int) -> None:
        bitmap[index // 8] |= 1 << (index % 8)

    @staticmethod
    def clear_bit(bitmap: bytearray, index: int) -> None:
        bitmap[index // 8] &= ~(1 << (index % 8)) & 0xFF

    def format(self) -> None:
        """Create an empty OuroFS, matching FileSystem::format/initialiseRoot."""
        zero = bytes(BLOCK_SIZE)
        # Match the kernel formatter: invalidate the superblock, then clear
        # bitmap/table metadata. Unallocated data blocks need not be wiped.
        self.write_block(SUPERBLOCK_BLOCK, zero)
        for number in range(INODE_BITMAP_BLOCK, DATA_START_BLOCK):
            self.write_block(number, zero)

        inode_bitmap = self.read_block(INODE_BITMAP_BLOCK)
        self.set_bit(inode_bitmap, ROOT_INODE)
        self.write_block(INODE_BITMAP_BLOCK, inode_bitmap)

        # The kernel allocator takes the first available data block: block 38.
        root_data_block = DATA_START_BLOCK
        bitmap_index = root_data_block // 8
        bitmap_block_number = BLOCK_BITMAP_START + bitmap_index // BLOCK_SIZE
        bitmap_offset = bitmap_index % BLOCK_SIZE
        block_bitmap = self.read_block(bitmap_block_number)
        block_bitmap[bitmap_offset] |= 1 << (root_data_block % 8)
        self.write_block(bitmap_block_number, block_bitmap)
        self.write_block(root_data_block, self.pack_dir_entry(ROOT_INODE, INODE_DIRECTORY, b".")
                         + self.pack_dir_entry(ROOT_INODE, INODE_DIRECTORY, b"..")
                         + bytes(BLOCK_SIZE - 2 * DIR_ENTRY_SIZE))

        root = {
            "mode": INODE_DIRECTORY,
            "links": 2,
            "size": 2 * DIR_ENTRY_SIZE,
            "direct": [root_data_block] + [0] * 11,
            "indirect": 0,
            "double_indirect": 0,
        }
        self.write_inode(ROOT_INODE, root)

        # Superblock is committed last, as in the kernel formatter.
        superblock = bytearray(BLOCK_SIZE)
        struct.pack_into("<IIIQ", superblock, 0,
                         SUPERBLOCK_MAGIC, SUPERBLOCK_VERSION,
                         BLOCK_SIZE, self.total_blocks)
        struct.pack_into("<I", superblock, 20, INODE_COUNT)
        struct.pack_into("<Q", superblock, 24, INODE_BITMAP_BLOCK)
        struct.pack_into("<Q", superblock, 32, BLOCK_BITMAP_START)
        struct.pack_into("<Q", superblock, 40, BLOCK_BITMAP_BLOCKS)
        struct.pack_into("<Q", superblock, 48, INODE_TABLE_BLOCK)
        struct.pack_into("<Q", superblock, 56, INODE_TABLE_BLOCKS)
        struct.pack_into("<Q", superblock, 64, DATA_START_BLOCK)
        struct.pack_into("<I", superblock, 72, ROOT_INODE)
        self.write_block(SUPERBLOCK_BLOCK, superblock)
        self.file.flush()
        os.fsync(self.file.fileno())
        self.mount()

    def mount(self) -> None:
        block = self.read_block(SUPERBLOCK_BLOCK)
        fields = {
            "magic": struct.unpack_from("<I", block, 0)[0],
            "version": struct.unpack_from("<I", block, 4)[0],
            "block_size": struct.unpack_from("<I", block, 8)[0],
            "total_blocks": struct.unpack_from("<Q", block, 12)[0],
            "inode_count": struct.unpack_from("<I", block, 20)[0],
            "inode_bitmap_block": struct.unpack_from("<Q", block, 24)[0],
            "block_bitmap_start": struct.unpack_from("<Q", block, 32)[0],
            "block_bitmap_blocks": struct.unpack_from("<Q", block, 40)[0],
            "inode_table_start": struct.unpack_from("<Q", block, 48)[0],
            "inode_table_blocks": struct.unpack_from("<Q", block, 56)[0],
            "data_start": struct.unpack_from("<Q", block, 64)[0],
            "root_inode": struct.unpack_from("<I", block, 72)[0],
        }
        expected = {
            "magic": SUPERBLOCK_MAGIC,
            "version": SUPERBLOCK_VERSION,
            "block_size": BLOCK_SIZE,
            "total_blocks": self.total_blocks,
            "inode_count": INODE_COUNT,
            "inode_bitmap_block": INODE_BITMAP_BLOCK,
            "block_bitmap_start": BLOCK_BITMAP_START,
            "block_bitmap_blocks": BLOCK_BITMAP_BLOCKS,
            "inode_table_start": INODE_TABLE_BLOCK,
            "inode_table_blocks": INODE_TABLE_BLOCKS,
            "data_start": DATA_START_BLOCK,
            "root_inode": ROOT_INODE,
        }
        if fields != expected:
            raise OuroFSError(
                "image does not contain a valid compatible OuroFS superblock; "
                "use --format only if you intend to erase its filesystem"
            )
        self.mounted = True

    def inode_location(self, inode_number: int) -> tuple[int, int]:
        if not 0 <= inode_number < INODE_COUNT:
            raise OuroFSError(f"invalid inode number: {inode_number}")
        return INODE_TABLE_BLOCK + inode_number // (BLOCK_SIZE // INODE_SIZE), \
            (inode_number % (BLOCK_SIZE // INODE_SIZE)) * INODE_SIZE

    def read_inode(self, inode_number: int) -> dict:
        block_number, offset = self.inode_location(inode_number)
        block = self.read_block(block_number)
        mode, links = struct.unpack_from("<HH", block, offset)
        size = struct.unpack_from("<Q", block, offset + 8)[0]
        direct = list(struct.unpack_from("<12I", block, offset + 16))
        indirect = struct.unpack_from("<I", block, offset + 64)[0]
        double_indirect = struct.unpack_from("<I", block, offset + 68)[0]
        return {
            "mode": mode, "links": links, "size": size, "direct": direct,
            "indirect": indirect, "double_indirect": double_indirect,
        }

    def write_inode(self, inode_number: int, inode: dict) -> None:
        block_number, offset = self.inode_location(inode_number)
        block = self.read_block(block_number)
        # repr(C) Inode: u16 mode, u16 links, 4 bytes alignment padding,
        # u64 size, 12*u32 direct, u32 indirect, u32 doubleIndirect,
        # 52 reserved bytes and 4 trailing alignment bytes.
        record = bytearray(INODE_SIZE)
        struct.pack_into("<HH", record, 0, inode["mode"], inode["links"])
        struct.pack_into("<Q", record, 8, inode["size"])
        direct = inode["direct"]
        if len(direct) != 12:
            raise OuroFSError("internal error: inode must contain 12 direct pointers")
        struct.pack_into("<12I", record, 16, *direct)
        struct.pack_into("<I", record, 64, inode.get("indirect", 0))
        struct.pack_into("<I", record, 68, inode.get("double_indirect", 0))
        block[offset:offset + INODE_SIZE] = record
        self.write_block(block_number, block)

    def alloc_inode(self) -> int:
        bitmap = self.read_block(INODE_BITMAP_BLOCK)
        for number in range(1, INODE_COUNT):
            if not self.bit_is_set(bitmap, number):
                self.set_bit(bitmap, number)
                self.write_block(INODE_BITMAP_BLOCK, bitmap)
                self.write_inode(number, {
                    "mode": INODE_FREE, "links": 0, "size": 0,
                    "direct": [0] * 12, "indirect": 0, "double_indirect": 0,
                })
                return number
        raise OuroFSError("no free inodes remain")

    def free_inode(self, number: int) -> None:
        if number == ROOT_INODE:
            raise OuroFSError("cannot free the root inode")
        bitmap = self.read_block(INODE_BITMAP_BLOCK)
        self.clear_bit(bitmap, number)
        self.write_block(INODE_BITMAP_BLOCK, bitmap)
        self.write_inode(number, {
            "mode": INODE_FREE, "links": 0, "size": 0,
            "direct": [0] * 12, "indirect": 0, "double_indirect": 0,
        })

    def block_bitmap_location(self, number: int) -> tuple[int, int, int]:
        byte_index = number // 8
        return (BLOCK_BITMAP_START + byte_index // BLOCK_SIZE,
                byte_index % BLOCK_SIZE, number % 8)

    def alloc_block(self) -> int:
        for number in range(DATA_START_BLOCK, self.total_blocks):
            bitmap_block_number, bitmap_offset, bit_index = self.block_bitmap_location(number)
            bitmap = self.read_block(bitmap_block_number)
            mask = 1 << bit_index
            if bitmap[bitmap_offset] & mask:
                continue
            bitmap[bitmap_offset] |= mask
            self.write_block(bitmap_block_number, bitmap)
            self.write_block(number, bytes(BLOCK_SIZE))
            return number
        raise OuroFSError("no free data blocks remain")

    def free_block(self, number: int) -> None:
        if not DATA_START_BLOCK <= number < self.total_blocks:
            raise OuroFSError(f"cannot free invalid data block {number}")
        bitmap_block_number, bitmap_offset, bit_index = self.block_bitmap_location(number)
        bitmap = self.read_block(bitmap_block_number)
        mask = 1 << bit_index
        if bitmap[bitmap_offset] & mask == 0:
            raise OuroFSError(f"data block {number} is already free")
        bitmap[bitmap_offset] &= ~mask & 0xFF
        self.write_block(bitmap_block_number, bitmap)

    @staticmethod
    def pack_dir_entry(inode_number: int, kind: int, name: bytes) -> bytes:
        if not name or len(name) > DIR_NAME_MAX or b"/" in name or b"\0" in name:
            raise OuroFSError(f"invalid OuroFS directory name: {name!r}")
        entry = bytearray(DIR_ENTRY_SIZE)
        struct.pack_into("<IBBH", entry, 0, inode_number, kind, len(name), 0)
        entry[8:8 + len(name)] = name
        return bytes(entry)

    def directory_entries(self, inode_number: int) -> list[tuple[bytes, int, int]]:
        directory = self.read_inode(inode_number)
        if directory["mode"] != INODE_DIRECTORY:
            raise OuroFSError("path component is not a directory")
        if directory["size"] % DIR_ENTRY_SIZE or directory["size"] > 12 * BLOCK_SIZE:
            raise OuroFSError("directory has an invalid size")
        count = directory["size"] // DIR_ENTRY_SIZE
        entries = []
        for index in range(count):
            block_index = index // (BLOCK_SIZE // DIR_ENTRY_SIZE)
            offset = (index % (BLOCK_SIZE // DIR_ENTRY_SIZE)) * DIR_ENTRY_SIZE
            if block_index >= 12:
                raise OuroFSError("directory exceeds direct block limit")
            block_number = directory["direct"][block_index]
            if not DATA_START_BLOCK <= block_number < self.total_blocks:
                raise OuroFSError("directory references an invalid data block")
            block = self.read_block(block_number)
            inode, kind, name_length = struct.unpack_from("<IBB", block, offset)
            if not 0 < name_length <= DIR_NAME_MAX or inode >= INODE_COUNT:
                raise OuroFSError("directory contains a malformed entry")
            name = bytes(block[offset + 8:offset + 8 + name_length])
            entries.append((name, inode, kind))
        return entries

    def find_entry(self, directory_inode: int, name: bytes) -> tuple[int, int] | None:
        for entry_name, inode_number, kind in self.directory_entries(directory_inode):
            if entry_name == name:
                return inode_number, kind
        return None

    @staticmethod
    def split_path(path: str) -> list[bytes]:
        if not path.startswith("/") or "\0" in path:
            raise OuroFSError(f"destination path must be absolute: {path!r}")
        parts = [part.encode("utf-8") for part in path.split("/") if part]
        if any(part in (b".", b"..") for part in parts):
            raise OuroFSError("destination paths must not contain '.' or '..' components")
        if any(len(part) > DIR_NAME_MAX for part in parts):
            raise OuroFSError(f"destination component exceeds {DIR_NAME_MAX} bytes")
        if any(b"\0" in part for part in parts):
            raise OuroFSError("NUL is not allowed in a path")
        return parts

    def resolve(self, path: str) -> int:
        current = ROOT_INODE
        for component in self.split_path(path):
            entry = self.find_entry(current, component)
            if entry is None:
                raise OuroFSError(f"path does not exist: {path}")
            current, _kind = entry
            if self.read_inode(current)["mode"] == INODE_FREE:
                raise OuroFSError(f"path contains a free inode: {path}")
        return current

    def add_directory_entry(self, directory_inode: int, name: bytes, child_inode: int, kind: int) -> None:
        parent = self.read_inode(directory_inode)
        if parent["mode"] != INODE_DIRECTORY:
            raise OuroFSError("parent path is not a directory")
        if self.find_entry(directory_inode, name) is not None:
            raise OuroFSError(f"directory entry already exists: {name.decode(errors='replace')}")
        if parent["size"] % DIR_ENTRY_SIZE or parent["size"] >= 12 * BLOCK_SIZE:
            raise OuroFSError("parent directory is full or malformed")
        index = parent["size"] // DIR_ENTRY_SIZE
        block_index = index // (BLOCK_SIZE // DIR_ENTRY_SIZE)
        offset = (index % (BLOCK_SIZE // DIR_ENTRY_SIZE)) * DIR_ENTRY_SIZE
        if block_index >= 12:
            raise OuroFSError("parent directory exceeds direct block limit")
        if offset == 0 and parent["direct"][block_index] == 0:
            block_number = self.alloc_block()
            parent["direct"][block_index] = block_number
            # The Rust implementation writes the parent inode after the entry.
        block_number = parent["direct"][block_index]
        if not DATA_START_BLOCK <= block_number < self.total_blocks:
            raise OuroFSError("parent directory references an invalid data block")
        block = self.read_block(block_number)
        entry = self.pack_dir_entry(child_inode, kind, name)
        block[offset:offset + DIR_ENTRY_SIZE] = entry
        self.write_block(block_number, block)
        parent["size"] += DIR_ENTRY_SIZE
        self.write_inode(directory_inode, parent)

    def create_directory(self, parent_inode: int, name: bytes) -> int:
        parent = self.read_inode(parent_inode)
        if parent["mode"] != INODE_DIRECTORY:
            raise OuroFSError("parent path is not a directory")
        if self.find_entry(parent_inode, name) is not None:
            raise OuroFSError(f"directory entry already exists: {name.decode(errors='replace')}")
        if parent["size"] % DIR_ENTRY_SIZE or parent["size"] >= 12 * BLOCK_SIZE:
            raise OuroFSError("parent directory is full or malformed")
        inode_number = self.alloc_inode()
        data_block = self.alloc_block()
        directory_block = bytearray(BLOCK_SIZE)
        directory_block[0:DIR_ENTRY_SIZE] = self.pack_dir_entry(inode_number, INODE_DIRECTORY, b".")
        directory_block[DIR_ENTRY_SIZE:2 * DIR_ENTRY_SIZE] = self.pack_dir_entry(parent_inode, INODE_DIRECTORY, b"..")
        self.write_block(data_block, directory_block)
        self.write_inode(inode_number, {
            "mode": INODE_DIRECTORY, "links": 2, "size": 2 * DIR_ENTRY_SIZE,
            "direct": [data_block] + [0] * 11, "indirect": 0, "double_indirect": 0,
        })
        self.add_directory_entry(parent_inode, name, inode_number, INODE_DIRECTORY)
        parent = self.read_inode(parent_inode)
        parent["links"] += 1
        self.write_inode(parent_inode, parent)
        return inode_number

    def ensure_directory(self, path_parts: list[bytes]) -> int:
        current = ROOT_INODE
        for component in path_parts:
            existing = self.find_entry(current, component)
            if existing is None:
                current = self.create_directory(current, component)
            else:
                current = existing[0]
                if self.read_inode(current)["mode"] != INODE_DIRECTORY:
                    raise OuroFSError(f"path component is not a directory: {component!r}")
        return current

    def write_file(self, inode_number: int, data: bytes) -> None:
        if len(data) > MAX_FILE_SIZE:
            raise OuroFSError(
                f"file is {len(data)} bytes; current OuroFS limit is {MAX_FILE_SIZE} bytes"
            )
        inode = self.read_inode(inode_number)
        if (inode["mode"] != INODE_FILE or inode["indirect"] != 0
                or inode["double_indirect"] != 0 or inode["size"] > MAX_FILE_SIZE):
            raise OuroFSError("destination is not a compatible regular file")
        old_count = (inode["size"] + BLOCK_SIZE - 1) // BLOCK_SIZE
        new_count = (len(data) + BLOCK_SIZE - 1) // BLOCK_SIZE
        if old_count > 12:
            raise OuroFSError("existing file exceeds direct-block limit")
        old_blocks = inode["direct"][:old_count]
        for number in old_blocks:
            if not DATA_START_BLOCK <= number < self.total_blocks:
                raise OuroFSError("existing file references an invalid data block")
        if any(inode["direct"][old_count:]):
            raise OuroFSError("existing file has unexpected extra direct pointers")
        new_blocks = old_blocks[:min(old_count, new_count)]
        while len(new_blocks) < new_count:
            new_blocks.append(self.alloc_block())
        for index, block_number in enumerate(new_blocks):
            chunk = data[index * BLOCK_SIZE:(index + 1) * BLOCK_SIZE]
            block = bytearray(BLOCK_SIZE)
            block[:len(chunk)] = chunk
            self.write_block(block_number, block)
        old_unused = old_blocks[new_count:]
        inode["size"] = len(data)
        inode["direct"] = new_blocks + [0] * (12 - len(new_blocks))
        self.write_inode(inode_number, inode)
        for block_number in old_unused:
            self.free_block(block_number)

    def create_file(self, parent_inode: int, name: bytes, data: bytes) -> int:
        if self.find_entry(parent_inode, name) is not None:
            raise OuroFSError(f"file already exists: {name.decode(errors='replace')}")
        parent = self.read_inode(parent_inode)
        if parent["mode"] != INODE_DIRECTORY:
            raise OuroFSError("parent path is not a directory")
        if parent["size"] % DIR_ENTRY_SIZE or parent["size"] >= 12 * BLOCK_SIZE:
            raise OuroFSError("parent directory is full or malformed")
        inode_number = self.alloc_inode()
        self.write_inode(inode_number, {
            "mode": INODE_FILE, "links": 1, "size": 0,
            "direct": [0] * 12, "indirect": 0, "double_indirect": 0,
        })
        self.write_file(inode_number, data)
        self.add_directory_entry(parent_inode, name, inode_number, INODE_FILE)
        return inode_number

    def put_file(self, destination: str, source_path: Path) -> None:
        if not self.mounted:
            self.mount()
        data = source_path.read_bytes()
        if len(data) > MAX_FILE_SIZE:
            raise OuroFSError(
                f"{source_path} is {len(data)} bytes; current OuroFS limit is {MAX_FILE_SIZE} bytes"
            )
        parts = self.split_path(destination)
        if not parts:
            raise OuroFSError("cannot install a regular file at filesystem root")
        parent_inode = self.ensure_directory(parts[:-1])
        name = parts[-1]
        existing = self.find_entry(parent_inode, name)
        if existing is None:
            inode_number = self.create_file(parent_inode, name, data)
            action = "installed"
        else:
            inode_number, _kind = existing
            inode = self.read_inode(inode_number)
            if inode["mode"] != INODE_FILE:
                raise OuroFSError(f"destination exists but is not a regular file: {destination}")
            self.write_file(inode_number, data)
            action = "updated"
        # Read back the file through its disk metadata before claiming success.
        if self.read_file(inode_number) != data:
            raise OuroFSError(f"read-back verification failed for {destination}")
        self.file.flush()
        os.fsync(self.file.fileno())
        print(f"{action}: {destination} ({len(data)} bytes, inode {inode_number})")

    def read_file(self, inode_number: int) -> bytes:
        inode = self.read_inode(inode_number)
        if inode["mode"] != INODE_FILE or inode["size"] > MAX_FILE_SIZE:
            raise OuroFSError("cannot read: not a supported regular file")
        size = inode["size"]
        result = bytearray()
        count = (size + BLOCK_SIZE - 1) // BLOCK_SIZE
        for index in range(count):
            block_number = inode["direct"][index]
            if not DATA_START_BLOCK <= block_number < self.total_blocks:
                raise OuroFSError("file references an invalid data block")
            result.extend(self.read_block(block_number)[:min(BLOCK_SIZE, size - len(result))])
        return bytes(result)


def parse_put(value: str) -> tuple[str, Path]:
    if "=" not in value:
        raise argparse.ArgumentTypeError("--put syntax is DESTINATION=HOST_FILE")
    destination, source = value.split("=", 1)
    if not destination or not source:
        raise argparse.ArgumentTypeError("--put syntax is DESTINATION=HOST_FILE")
    return destination, Path(source)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Install host-built files into a raw OuroFS disk image."
    )
    parser.add_argument("image", type=Path, help="raw disk image used by QEMU")
    parser.add_argument(
        "--create-mib", type=int, metavar="SIZE",
        help="create and format a new image of SIZE MiB; fails if it already exists",
    )
    parser.add_argument(
        "--format", action="store_true",
        help="ERASE OuroFS metadata and format the existing image (destructive)",
    )
    parser.add_argument(
        "--put", action="append", type=parse_put, default=[], metavar="DEST=HOST_FILE",
        help="install or update a file; may be repeated, and creates parent directories",
    )
    args = parser.parse_args()

    try:
        if args.create_mib is not None:
            if args.format:
                raise OuroFSError("use --create-mib without --format; new images are formatted automatically")
            if args.create_mib <= 0:
                raise OuroFSError("image size must be positive")
            if args.image.exists():
                raise OuroFSError(f"refusing to overwrite existing image: {args.image}")
            args.image.parent.mkdir(parents=True, exist_ok=True)
            with args.image.open("xb") as new_image:
                new_image.truncate(args.create_mib * 1024 * 1024)
            print(f"created raw disk image: {args.image} ({args.create_mib} MiB)")
            should_format = True
        else:
            if not args.image.exists():
                raise OuroFSError(
                    f"disk image does not exist: {args.image}; use --create-mib SIZE for a new image"
                )
            should_format = args.format

        mode = "r+b"
        with args.image.open(mode) as image_file:
            fs = OuroFSImage(image_file)
            if should_format:
                fs.format()
                print(f"formatted OuroFS: {fs.total_blocks} blocks x {BLOCK_SIZE} bytes")
            else:
                fs.mount()
            for destination, source in args.put:
                if not source.is_file():
                    raise OuroFSError(f"host source file does not exist: {source}")
                fs.put_file(destination, source)
            image_file.flush()
            os.fsync(image_file.fileno())
        return 0
    except (OuroFSError, OSError, struct.error) as error:
        print(f"ouro-image.py: error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
