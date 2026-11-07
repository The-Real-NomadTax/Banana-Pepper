# Debug experiment: skips GameInitDongle; accepts only already-plaintext script buffers.
set startup-with-shell off
set print thread-events off
set environment LD_PRELOAD /shim.so
break *0x81bab40
break *0x827e11b if *(unsigned char*)*(unsigned int*)($esp+0x54)==47 && *(unsigned char*)(*(unsigned int*)($esp+0x54)+1)==47 && *(unsigned char*)(*(unsigned int*)($esp+0x54)+2)==10
commands 2
silent
set $pc=0x827e121
continue
end
break *0x8324378
commands 3
silent
printf "SHADER COMPILER LOG:\n"
x/s 0x8b51f00
continue
end
run
set $pc=*(unsigned int*)$esp
set $esp=$esp+4
set $eax=0
call (int)setvbuf(*(void**)&stdout,(void*)0,2,0)
set *(unsigned char*)0x83344b0=0xe9
set *(unsigned int*)0x83344b1=(unsigned int)(void*)printf-0x83344b5
set *(int*)0x8b510d4=3
continue
bt
info registers
x/12wx $esp

x/s 0x8b51f00
thread apply all bt 4
