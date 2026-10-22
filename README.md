# Ouroboros

This project is a small, intentionally minimal AArch64 kernel for QEMU's `virt` machine. It is designed as a learning project and a working prototype, not a polished production operating system.

## Quick start

To use it, create a 512 MiB disk image named `ouroboros.img` in the repository root before booting. If it is missing, the kernel may fail to boot or the filesystem setup can behave incorrectly.

```bash
truncate -s 512M ouroboros.img
./build-and-run.sh
```

## This is what each bit does

This project is easier to read if you treat it as a bunch of tiny bitfields and numeric contracts. The code is intentionally explicit about the flags it is using.

### 1. Syscall numbers

All user/kernel transitions use `svc` instructions. The kernel matches the encoded immediate in the exception handler in `src/main.rs`.

These are the current values:

- `#1`: exit current task
- `#2`: yield current task
- `#3`: write bytes to the UART
- `#4`: allocate user pages
- `#5`: read one byte from the UART
- `#6`: create a file at a path with initial data
- `#7`: read a file at a path into a buffer
- `#8`: replace a file's contents at a path
- `#9`: delete a file at a path
- `#10`: stat a path
- `#11`: create a directory
- `#12`: remove an empty directory
- `#13`: spawn a new user task from a `.run` path
- `#42`: test/debug syscall used as a placeholder - may or may not be removed in the future

The whole userspace ABI is a table of small integer calls. That is the kernel's only interface with userspace.

### 2. UART status bits

The UART implementation is reading raw MMIO registers and checking a few flags.

In `Uart::put_byte`, it waits while bit 5 of the status register is set:

- bit 5 = transmit FIFO full

If it is set, the code cannot send another byte.

In `Uart::try_get_byte`, it checks bit 4:

- bit 4 = receive data available / ready-to-read

If that bit is clear, there is no byte waiting. Otherwise it reads the byte from the receive register.


The kernel does not use a driver abstraction; it is reading the hardware register bits by name.

### 3. Page table entry bits

The page table entries are raw `u64` descriptors. The code does not use a fancy struct for them; it just builds the descriptor value directly.

The important bit-level behavior is:

- bit 0: valid bit
  - if set, the descriptor is valid and the MMU will walk it
  - if clear, it is invalid and treated as empty

- lower-level table entries:
  - bit 0 = valid
  - bits 1..?? = descriptor type / pointing to next table
  - `new_table()` ORs in `0b11`, which makes the descriptor both present and table-like

- block/page entries:
  - the raw address bits are masked and reinserted
  - bit 0 is still the valid bit
  - the table/page descriptor uses the lower bits to distinguish block vs page descriptors

- `AttrIndx`:
  - stored in the attribute index field of the descriptor
  - `ATTR_NORMAL = 0`
  - `ATTR_DEVICE = 1`

- `AP` (access permissions):
  - `0b00` = privileged-only access
  - `0b01` = EL0 read/write
  - `0b11` = EL0 read-only

- `SH` (shareability):
  - `0b11` is used for the user and kernel maps that are expected to be sharable across CPUs

- `AF` (access flag):
  - bit 10 is set to mark the entry as having been accessed

- `PXN` / `UXN`:
  - set when execution is forbidden for that page at a given exception level
  - this is how the kernel marks pages as non-executable when needed

This is just the AArch64 descriptor format made visible in Rust code.

### 4. Page table layout and the meaning of the tables

The kernel builds a very small static multilevel map:

- `PAGE_TABLE_L0` chooses the top-level range
- `PAGE_TABLE_L1_LOW` maps the lower 512 GiB
- `PAGE_TABLE_L2_DEVICE` holds device mappings
- `PAGE_TABLE_L2_RAM` holds kernel RAM mappings
- `PAGE_TABLE_L2_VMAP` holds the user virtual map
- `PAGE_TABLE_L3` and `PAGE_TABLE_L3_VMAP` hold final page entries

The code is intentionally explicit about where things go:

- `0x0800_0000` = device region
- `0x0900_0000` = UART / device region
- `0x4000_0000..` = kernel RAM
- `0x8000_0000..` = userspace virtual map


### 5. Syscall file-type values

The filesystem API returns a `FileStat` with a small kind field.

The project uses the following values:

- `1` = file
- `2` = directory

This is the kernel's simple contract for userspace:

- `kind == 1` => `FileKind::File`
- `kind == 2` => `FileKind::Directory`

### 6. RUN file permission bits

The custom `.run` loader uses a tiny flag field.

The bit constants are:

- `RUN_R = 1`
- `RUN_W = 2`
- `RUN_X = 4`


The user mapping code then converts those into permission bits for the page tables:

- writable pages use `AP = 0b01`
- read-only pages use `AP = 0b11`
- NX/XN is set when the page should not execute


### 7. SPSR bits

The task context stores `spsr` values, and the two important ones are:

- `SPSR_EL0T = 0b0000`
- `SPSR_EL1H = 0b0101`

These are the exception-level state values selected for the saved task context:

- `EL0T` means the task resumes in EL0 with a trap-return path
- `EL1H` means kernel mode / EL1 handler state

The bit pattern matters because this is what tells the CPU what mode to restore when control returns to a task.

