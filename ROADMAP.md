## Things that should always be being worked on
- Fixing bugs
- Optimisations (that are still readable!)
- READMEs
## Things that are currently being implemented
I am currently adding better error handling to Ouroboros. 
This includes not panicking anytime something goes unexpected (which would be ideal for obvious reasons), and better diagnostics for when things do fail.
I am also making it so that it is nigh impossible for a userspace program to crash the entire kernel, and adding rollback features for when such an error does occur.

The other thing I am currently building is a better userspace library.
Whilst programs should be able to access syscalls directly if they so wish, this is not always ideal, and so I am working on building abstractions such as being able to refer to the filesystem in an easier manner, or having easier calls to allocate RAM.
The userspace library will also need some documentation of its own, to make it simpler for programmers to know what they are dealing with. I wouldn't make a start on this until the userspace library itself is complete.

I don't have a way to determine objectively when either of these things are complete, but that is a decision that will have to be made.

## Next major feature
The next major feature will be adding graphics to Ouroboros. This includes having a window manager, GUI, and allowing userspace programs to be able to access graphics.
The planned abstraction is to have the window manager have ultimate control over the framebuffer, with other programs being allocated a window of a given size, and a framebuffer for it.
The window manager would then collate all framebuffers, and draw them in their correct positions, stopping each program from having access to the entire screen.
Programs should be able to access their framebuffer directly, but a graphics library will be optional as part of the system library. 
This would include things like buttons, rectangles &c.
Programs should be ignorant of how their framebuffer gets written to the GPU, and I don't think this will compromise the amount of features a program can access.

For the default GUI, I was planning to go with a customisable Windows 95 aesthetic (as default), but it should be designed so that you could plug and place whatever you want.
## Future planned major features
- The ability to program code on the OS (hopefully through Rust, C, Python, or the proprietary language I am making for the OS)
- Productivity suite
- Audio
- Networking
- Being able to run the OS natively (on my Raspberry Pi at the very least)