set startup-with-shell off
set print thread-events off
set follow-fork-mode parent
set detach-on-fork on
set environment LD_PRELOAD /shim.so
break *0x8253c78
run
set $stream=*(unsigned int*)$esp
printf "inflate input stream:\n"
x/14wx $stream
x/16bx *(void**)$stream
nexti
printf "inflate returned=%d\n", $eax
x/s *(void**)($stream+24)
bt 10
quit
