This, as you might have guessed, is a README of the Ouroboros operating.
I will first explain how this kernel is run on my system (or if you are savvy-enough, on any system). Then will follow a breakdown of the kernel, and then design paradigms and aspirations.

## How to run Ouroboros (my way)
Ouroboros is designed for Arm Cortex CPUs, with the Arm Cortex-A76 the focus (although it runs on any such CPU at least as early as the Arm Cortex-A53 being the earliest I have tested). I do not plan to support x86-64 CPUs, RISC V CPUs or any other aarch64 CPUs.
I personally use QEMU to run Ouroboros, with my script found in [`build-and-run.sh`](build-and-run.sh)
This is a bash script designed to run on raspbian, with my setup. If you aren't compiling the kernel, and just want to run it, you only need:
> qemu-system-aarch64 -M virt -global virtio-mmio.force-legacy=false -cpu cortex-a76 -m 512M -monitor stdio -kernel compiled/ouroboros -drive if=none,file=ouroboros.img,format=raw,id=ouroDisk -device virtio-blk-device,drive=ouroDisk -device virtio-gpu-device -nographic

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

#### L2_RAM
Here we assign the the L3 Table at [0] and then 255 more pages of L2 RAM for kernel use after that.

#### L2_VMAP
Much like the L3 Table is at L2_RAM[0], the VMAP L3 Table is at L2_VMAP[0]. We do not assign any more pages here, however.

#### L3_RAM
For this table, we assign 512 4KiB pages, to be used by the kernel and by tasks that Ouroboros is running.

### VirtIO GPU (This is still WIP)
First, we initialise a new GPU struct from [`src/virtio_gpu.rs`](src/virtio_gpu.rs).
I will not explain the full workings of that script here, only what is used directly by the kernel.
That information will be in a seperate README.
We then probe the GPU for some metadata, namely the magic, version, and what device it is.
This is because (although it is coded to read from the right address), we do need to be robust, and check that we are not making any assumptions.
Then, the GPU is initialised. 
The kernel informs us whether or not this succeeded, but continues no matter what.
We then fetch the info about the display the GPU is outputting to.
If there is no display, that would not be a kernel issue perhaps, because the GPU is designed to be able to withstand this.

### Filesystem and Drive
Ouroboros makes use of a drive format I designed, aptly named "OuroFS". 
This is simply for development ease on my part, and I do plan to later expand Ouroboros to be able to use other formats, such as ext4 or FAT drives.
Documentation on OuroFS and [`src/filesystem.rs`](src/filesystem.rs) will be in a seperate README.
If there is no drive, QEMU prematurely throws an error, so this is not a failcase the kernel handles.
If the drive is not formatted to OuroFS, we format it.
Otherwise, we mount it directly.
The kernel then prints some diagnostic information on the drive, such as its size, and the block size it has been formatted with.

The kernel then automatically loads user-init.run (our shell program) for the drive. 
It does try to continue execution, but Ouroboros doesn't actually do much without a shell. You effectively end up with an initialised, yet idle CPU.
The kernel does **NOT** load the shell into the stack. This is only 16KiB in size, and the kernel crashes due to a stack overflow.
It stores the program in a 49.1 KiB buffer (the old maximum size for a .run executable).
If this buffer is exceeded, the shell will half-load, but cut off at the max size of the buffer.
The kernel will inform you if the provided shell does not fit inside the buffer.

### Scheduler
We then initialise the scheduler.
The scheduler holds:
- A TaskTable of Tasks
- The usize ID of the current task
- The current state of the task
- Which task is in foreground (if the task has spawned another task that it has passed to)
The scheduler has options to:
- Add a new task (a task that will execute as part of the kernel)
- Add a new user task (a task that will execute in userspace)
- Search for the next task that is ready to execute
- Start a task
- Tick (Switch tasks and update states)
Used by syscalls, and not by the scheduler without interrupts, there are also options to:
- Yield current task (pass execution to another task, and not kill the current task)
- Exit task (pass execution to another task, and kill the current task)
- Switch to a task (i.e., put a task in foreground)
- Spawn a foreground task (which uses the function above).
When the scheduler is first initialised, two tasks are provided.
- The idle task(kernel level, spins CPU loop. This is a fallback to avoid stopping execution)
- user-init.run (user level, this is our shell task)
The scheduler is biased against executing the idle task unless all other tasks are occupied or dead.

### GIC (Generic Interrupt Controller)
We next initialise the GIC.
This takes interrupts from peripheries, including the timer, and passes them on to the kernel.
This allows us to use our scheduling system.

### Timer
We then read the metadata about the timer provided (its frequency and counter).
We set the timer to fire once every 100ms, and enable it.
This allows us to use the timer interrupts to tick the scheduler.

### CPU IRQs and wfe
Finally, we enable IRQs. 
This lets the kernel uses the interrupts from the timer, and the scheduler starts execution.
The kernel is designed to run in a loop afterwards. This is so that execution does not stop if the scheduler fails to load.

With this done, the initialisation has finished its job.
Information about how the shell works, or how to write programs for Ouroboros, will be in seperate READMEs.

## Design philosophy
Ouroboros is designed to be a powerful, yet transparent OS.
I find that modern OSes do not lend themselves very well to tinkerers like me, or people who want to know how things work.
Ouroboros, conversely, is designed to be readable to anyone who can read Rust.
That is one reason why this README goes more indepth than perhaps it should.
By providing little abstraction at each layer, it is possible to trace the execution of a program.
This not only aids understanding, but also forces programmers to acknowledge constraints directly.
Optimisations, I believe, should be key, rather than placing emphasis on bloat and ease of development.
This is why, what seem like strict constraints, such as 512MiB of RAM and storage, and a max file size of 12 pages, should be quite luxurious for those using Ouroboros.