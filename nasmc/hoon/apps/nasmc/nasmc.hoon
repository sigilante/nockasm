::  nasmc: the Nockasm compiler as a NockApp
::
::    modeled on hoonc: the host pokes %compile with .nasm source (or
::    a jamfile, for %lift) and an output path; the kernel answers
::    with a file-write effect and exits on the write ack. unlike
::    hoonc there is no %boot phase -- the expander is already
::    compiled into this kernel.
::
::    modes:
::      %jam     .nasm text -> jammed formula (raw formula, no vase)
::      %text    .nasm text -> canonical flat noun text
::      %render  .nasm text -> canonical .nasm source (formatter)
::      %lift    jamfile    -> canonical .nasm source
::
/+  nockasm
/=  *  /common/wrapper
=>
|%
+$  nasmc-state  [%0 ~]
++  moat  (keep nasmc-state)
+$  cause
  $%  [%compile mode=@tas tex=@ out=@t]
      [%file %write path=@t contents=@ success=?]
  ==
+$  effect
  $%  [%file %write path=@t contents=@]
      [%exit id=@]
  ==
::  +num-tape: plain decimal (canonical flat form has no dot grouping)
::
++  num-tape
  |=  n=@
  ^-  tape
  ?:  =(0 n)  "0"
  =|  acc=tape
  |-  ^-  tape
  ?:  =(0 n)  acc
  $(acc [(add '0' (mod n 10)) acc], n (div n 10))
::  +print-noun: canonical flat right-spine form, matching the python
::  reference (print_noun in nockasm.py)
::
++  print-noun
  |=  n=*
  ^-  tape
  ?@  n  (num-tape n)
  =/  elems
    =/  cur  `*`n
    =|  acc=(list *)
    |-  ^-  (list *)
    ?@  cur  (flop `(list *)`[cur acc])
    $(acc [-.cur acc], cur +.cur)
  =/  parts  (turn elems print-noun)
  :(weld "[" (join-tapes:nockasm " " parts) "]")
--
%-  (moat |)
^-  fort:moat
|_  k=nasmc-state
::
++  load
  |=  old=nasmc-state
  ^-  nasmc-state
  old
::
++  peek
  |=  =path
  ^-  (unit (unit *))
  ?+  path  ~
    [%version ~]  ``nasm-version:nockasm
  ==
::
++  poke
  |=  [=wire eny=@ our=@ux now=@da dat=*]
  ^-  [(list effect) nasmc-state]
  =/  cause=(unit cause)  ((soft cause) dat)
  ?~  cause
    ~&  "nasmc: invalid cause"
    !!
  ?-    -.u.cause
      %file
    ?:  success.u.cause
      ~&  "nasmc: output written to {<path.u.cause>}"
      [[%exit 0]~ k]
    ~&  "nasmc: failed to write {<path.u.cause>}"
    [[%exit 1]~ k]
  ::
      %compile
    =/  res
      %-  mule  |.
      ^-  @
      ?+    mode.u.cause  ~|([%nasmc-unknown-mode mode.u.cause] !!)
          %jam
        (jam (expand:nockasm tex.u.cause))
      ::
          %text
        %-  crip
        (weld (print-noun (expand:nockasm tex.u.cause)) "\0a")
      ::
          %render
        (render:nockasm (parse:nockasm tex.u.cause))
      ::
          %lift
        (nasm-from-jam:nockasm tex.u.cause)
      ==
    ?:  ?=(%| -.res)
      ~&  "nasmc: compile failed"
      %.  [[%exit 1]~ k]
      (slog p.res)
    [[%file %write out.u.cause p.res]~ k]
  ==
--
