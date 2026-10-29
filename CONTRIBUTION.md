If you are reading this, you are wondering how to contribute to the Ouroboros operating system.
Thank you. Thank you so, so much.
There are two main ways that you could contribute to Ouroboros, either by working on the kernel (according to the roadmap), or by writing programs for it.
Both are still very much in their infancy, so do expect things to change, and do expect this not to update at the rate the code may.
Though I hesitate to say it, these are at the moment _hazy_ guidelines. 
I might not follow them exactly, you might forget to, whatever. This is a pet project, not the Linux Foundation (yet?).

## LLM usage.
You are allowed to use LLMs, so long as all committed code has been reviewed and tested by yourself.
If you are just committing vibecoded slop, I will have to ban you from committing.
As far as I am concerned, LLMs are to be used as a tool. They are **NOT** there to replace you, and so you damn well better be thinking for yourself.

## Firstly, what can be contributed?
Anything, so long as it is not directly against the roadmap / design philosophy, or added with malicious intent.
It must also be to a good code quality, which is subject to review.
You are more than welcome to improve any spaghetti code of mine you may find, or add entirely new features that are beneficial.
Your code must be transparent, and understandable. You must update any READMEs that your code invalidates or adds to.
You should not be changing any format specifications or client facing parts of the operating system without checking.
These could invalidate the setups of people, and so code or configuration may need to be changed just because one person decided that .run files were designed badly.

## My personal way of writing code
Names of things are always in _camelCase_. I am aware that this is not rust convention, but sue me (please don't I am a broke 14 y/o with no life).
Comments are a thing that I personally forget to add. If they help you, feel free to add them. Feel free to add them to my bits of code!
One thing that I do use comments for is to seperate sections by what they are doing, such as seperating RAM from Scheduler.
These could just as well be put into their own files, but I believe that it is safer (if more unwieldy), to have everything required to boot the kernel to some degree in one file.
Anything after that, that are not technically necessary, like the GPU or the filesystem can be put into their own file.