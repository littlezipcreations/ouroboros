This, as you might have guessed, is a README of the Ouroboros operating.
I will first explain how this kernel is run on my system (or if you are savvy-enough, on any system). Then will follow a breakdown of the kernel, and then design paradigms and aspirations.

## How to run Ouroboros (my way)
Ouroboros is designed for Arm Cortex CPUs, with the Arm Cortex-A76 the focus (although it runs on any such CPU at least as early as the Arm Cortex-A53 being the earliest I have tested). I do not plan to support x86-64 CPUs, RISC V CPUs or any other aarch64 CPUs.
I personally use QEMU to run Ouroboros, with my script found in [`build-and-run.sh`](build-and-run.sh)
This is a bash script designed to run on raspbian, with my setup. If you aren't compiling the kernel, and just want to run it, you only need:
> qemu-system-aarch64 -M virt -global virtio-mmio.force-legacy=false -cpu cortex-a76 -m 512M -display sdl -serial file:/tmp/ouroboros-uart.log -monitor stdio -kernel compiled/ouroboros -drive if=none,file=ouroboros.img,format=raw,id=ouroDisk -device virtio-blk-device,drive=ouroDisk -device virtio-gpu-device -nographic
(This is presuming you are looking at the release repo, and not the source.)
Graphics for Ouroboros is not finished in this version, but it inits correctly, and so I will leave it in the kernel.
For this to work, you need a version of QEMU that can run aarch64 CPUs on your computer. This can be found:
>https://www.qemu.org/download/
but I will leave this to you to work out.

## What is Ouroboros actually doing?
This is the technical bit. I am going to be dissecting several thousand lines of Rust in this, hopefully somewhat readable, tone. This is not required reading, just something I wish were available for all software.

### Boot
The first thing that is executed is [`src/boot.S`](src/boot.S). This is a simple bit of assembly that initialises to the main Rust kernel. If this fails, it executes _wfe_ and hangs until killed. You notice that this is a recurring theme.

We then enter into the main file, from rust_start(). This is the most important function in the entire kernel. 
### UART
It first initialises UART, then tells us that it has started.
UART is (under the setup used for Ouroboros), an emulation of the ARM PL011 UART. 
The UART implementation and data structure used by Ouroboros is able to initialise itself, put a byte to UART, put multiple bytes to UART(this is not strictly necessary, but I use it as QoL), and attempt to fetch bytes from the keyboard. 
It also has a _Write_ implementation for itself, which allows it to recieve text as _&str_ rather than a set of bytes (this is also not strictly necessary).
This allows us to use the writeln! macro as well, which has proven very useful for debugging.

### FP/SIMD
Next, the kernel enables FP/SIMD(Floating Point arithmetic and Single Instruction multiple data).
This allows for floating point arithmetic, and quicker use of arrays (by allowing multiple sets of data to be referenced by a single CPU instruction).

### Exceptions
We then install the exception vector into RAM, the assembly for which can be found at [`src/exceptions.S`](src/exceptions.S). 
The data format that Rust expects is found in [`src/exception.rs`](src/exception.rs). 
There is not much to say about this assembly, as even though it is lengthy, it does much of the same thing.
If an exception is thrown, it takes over from Rust (which will have crashed), and saves the value of:
- Each register (0 through 31)
- The Stack Pointer (SP)
- The Exception Link Register (ELR, holds the address at which operation should resume)
- The Saved Program Status Register (SPSR, holds the processor state, including flags and exception level)
- The Exception Syndrome Register (ESR, describes the cause of the exception and some details about it)
- The Fault Address Register (FAR, holds the memory address associated with any memory related faults)
- The kernel_sp (the stack pointer according to the kernel, which is my software implementation.)
For any unhandled exception, the kernel hangs. 
I do aim to implement a form of recovery in the future, but if the OS and its programs are written well enough, all of this code should be inconsequential.
#### Exception SError
This is the simple one. It tells you that there is an SError and hangs. 
This is not particularly descriptive, but I have never encountered one.
Nevertheless, an SError is when the kernel encounters an exception that is not applicable.
#### Exception Fast Interrupt Request (FIQ)
This prints the ELR and FP before continuing execution.
The FIQ occurs to support interrupt handling with lower latency than tradition IRQ handling. 
The exact latency mostly depends on the CPU and hardware, and I have never encountered one either.
#### Exception Interrupt Request (IRQ)
The kernel ignores any IRQs without ID 30. 
They are effectively status flags from peripheries (of which Ouroboros does not support many).
ID 30 itself is significant, as this is the exception generated when the hardware timer reaches its deadline.
Ouroboros uses this specific IRQ to co-ordinate the scheduler, which is based off of the hardware timer.
#### Synchronous exceptions
These are caused by an instruction being executed, or an operation associated with an instruction.
This ranges from Aborts and illegal access, to syscalls.
If the exception class is 15, it moves to syscall implementation, which shall be covered later.
Otherwise, it prints a diagnostic of the error, including all the data held by [`src/exceptions.S`](src/exceptions.S).
There is also a helper function that decodes the ESR, to give a report of what the failure was, though this is information like "Data Abort" and not a complete diagnostic.

### Memory allocator
After this, the kernel initialises the memory allocator. 
This calculates the bitmap of available pages in RAM outside of what is necessary for the kernel. 
The kernel, at minimum, recieves addresses 0x0000_0000 to 0x4000_0000, and then RAM the remaining addresses until 0x6000_0000.
Physical RAM is allocated in 4KiB pages, and allocation is tracked by the aforementioned bitmap.
The bitmap goes inside kernel RAM and not free RAM, to prevent it from potential tampering or deallocations.

### MMU
We then initalise the MMU. This allows for the L0 to 3 levels of RAM to be assigned, as well as Virtual RAM(VRAM) to be set up.
The L0[0] table is the only one used, as we only reference higher levels of RAM, and only the first 512GiB of RAM (Ouroboros only supports 512MiB of physical RAM).
The L1_LOW table contains L2_DEVICE at [0], L2_RAM at [1], and L2_VMAP at [2]. This table is otherwise not used by processes.

#### L2_DEVICE
This contains links to addresses for UART(0x09000000)[72], and the GIC Interrupt Controller(0x08000000)[64].
These are necessary, as accessing these devices would otherwise be impossible after MMU initialisation.
