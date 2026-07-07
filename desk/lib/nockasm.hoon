::  nockasm: parse Nock Assembly source to a canonical Nock 4K formula
::
::    a thin macro expander from .nasm source (as a cord) to the
::    corresponding Nock formula (as a noun). a port of nockasm.py;
::    the python's test suite is the conformance oracle, and beneath
::    both, the Nock 4K specification is the truth.
::
::    usage:
::      > =nasm -build-file %/lib/nockasm/hoon
::      > (expand:nasm '(%inc (%self))')
::      [4 0 1]
::      > .*([10 41 99] (expand:nasm ':subject {.a .b} .b'))
::      [42 99]
::
::    as a compiler target (doc/compiler-target.md): $nasm is a
::    versioned public IR. +parse takes source to IR; +lower takes IR
::    to canonical nock; +render takes IR to canonical .nasm text,
::    byte-identical to the python renderer, under the round-trip law
::    (expand (render sch ast)) === (lower sch ast).
::
::      > (render:nasm (parse:nasm ':subject {.a .b}  (%eq  .a  .b)'))
::      ':subject {.a .b}\0a(%eq .a .b)\0a'
::
::    syntax:
::      ; comments to end-of-line
::
::      ; optional axis schema for the subject (right-leaning by hoon
::      ; convention: {.a .b .c} -> .a=2, .b=6, .c=7; nesting allowed):
::        :subject {.tag .data}
::
::      ; named opcodes:
::        (%slot N)       -> [0 N]
::        (%self)         -> [0 1]     ; whole subject
::        (%battery)      -> [0 2]     ; standard core battery axis
::        (%payload)      -> [0 3]     ; standard core payload axis
::        (%sample)       -> [0 6]     ; standard gate sample axis
::        (%context)      -> [0 7]     ; standard gate context axis
::        (%crash)        -> [0 0]     ; nock crash idiom
::        (%const X)      -> [1 X]
::        (%arm X)        -> [1 X]     ; intent-marker: callable formula
::        (%eval S F)     -> [2 S F]
::        (%isa F)        -> [3 F]
::        (%inc F)        -> [4 F]
::        (%eq F G)       -> [5 F G]
::        (%if C T E)     -> [6 C T E]
::        (%comp F G)     -> [7 F G]
::        (%push F G)     -> [8 F G]
::        (%call N F)     -> [9 N F]
::        (%edit N V F)   -> [10 [N V] F]
::        (%hint T F)     -> [11 T F]      ; static hint
::        (%hintd T C F)  -> [11 [T C] F]  ; dynamic hint
::
::      ; structural macros:
::        #let .name = EXPR in EXPR
::          ; pushes EXPR via opcode 8, binds .name to axis 2 in the
::          ; body; names already in scope shift rightward via +peg
::        #match EXPR { PAT => EXPR ... _ => EXPR }
::          ; evaluates EXPR once via opcode 8, then nested opcode-6
::          ; dispatch against each literal PAT; _ default required
::
::      ; axis references:  .name -> [0 axis] from the current schema
::      ; raw cells:        [a b c]  (elements expanded, never lifted)
::      ; atom literals:    42  1.000  0x2a  0x1.0000  'cord'
::
::    argument kinds: 'f' formula positions lift bare atoms to
::    [1 atom]; 'n' noun-literal positions take any noun unchanged;
::    'a' axis positions require an atom. raw cells are structural:
::    sub-expressions expand but atoms never lift, which is the
::    escape hatch into raw nock (and gives you cons-formula
::    distribution for free).
::
::    errors crash with a tagged trace: %unbound-axis,
::    %unknown-opcode, %op-arity, %let-shadows, %match-needs-default,
::    %duplicate-schema-name, and friends. parse errors crash from
::    +scan with {line column}.
::
|%
+|  %types
::
::  $nasm: parsed nockasm expression
::
::    %atom   integer, hex, or cord literal (value already packed)
::    %axis   .name reference into the subject schema
::    %cell   raw [a b c] cell; elements expanded structurally
::    %op     (%opcode args) application
::    %let    #let p = q in r
::    %match  #match p { q... _ => r }
::
+$  nasm
  $~  [%atom 0]
  $%  [%atom p=@]
      [%axis p=@t]
      [%cell p=(list nasm)]
      [%op p=@t q=(list nasm)]
      [%let p=@t q=nasm r=nasm]
      [%match p=nasm q=(list mcas) r=nasm]
  ==
::  $mcas: one #match case: pattern and body
::
::    a named type rather than an inline pair: (list [p=nasm q=nasm])
::    inside the recursive $% sends the nest-checker into a stack
::    overflow (%over); routing the recursion through a named hold
::    keeps it finite.
::
+$  mcas  [p=nasm q=nasm]
::  $sema: subject axis schema, as parsed from :subject
::
+$  sema
  $~  [%leaf '']
  $%  [%leaf p=@t]
      [%pair p=sema q=sema]
  ==
::  $marm: one parsed #match arm: a case or the _ default
::
+$  marm
  $~  [%def [%atom 0]]
  $%  [%def p=nasm]
      [%case p=nasm q=nasm]
  ==
::
+|  %parsing
::
::  +woc: whitespace or ; comments, zero or more
::
++  woc
  %-  star
  ;~  pose
    (cold ~ (mask "\09\0a\0d "))
    (cold ~ ;~(plug mic (star ;~(less (just '\0a') next))))
  ==
::  +tkn: token rule — skip leading whitespace/comments
::
++  tkn  |*(sef=rule ;~(pfix woc sef))
::  +nym: a name: [A-Za-z_][A-Za-z0-9_-]*
::
++  nym
  %+  cook  |=([c=@ t=tape] (crip [c t]))
  ;~(plug ;~(pose alf cab) (star ;~(pose aln cab hep)))
::  +dem-lit: decimal literal with . or _ separators (unvalidated
::  grouping, per the python): 1.000 -> 1000
::
++  dem-lit
  %+  cook
    |=  [c=@ t=tape]
    %+  roll  `tape`[c t]
    |=  [c=@tD a=@]
    ?:  |(=('.' c) =('_' c))  a
    (add (mul 10 a) (sub c '0'))
  ;~(plug nud (star ;~(pose nud dot cab)))
::  +hex-lit: 0x-prefixed hex literal with . or _ separators
::
++  hex-lit
  %+  cook
    |=  [c=@ t=tape]
    %+  roll  `tape`[c t]
    |=  [c=@tD a=@]
    ?:  |(=('.' c) =('_' c))  a
    (add (mul 16 a) (hex-val c))
  ;~(pfix (jest '0x') ;~(plug hix (star ;~(pose hix dot cab))))
++  hix  (mask "0123456789abcdefABCDEF")
++  hex-val
  |=  c=@tD
  ^-  @
  ?:  &((gte c '0') (lte c '9'))  (sub c '0')
  ?:  &((gte c 'a') (lte c 'f'))  (add 10 (sub c 'a'))
  ?:  &((gte c 'A') (lte c 'F'))  (add 10 (sub c 'A'))
  !!
::  +qut-lit: 'cord' literal, packed little-endian (any character
::  but the closing quote, newlines included; no escapes)
::
++  qut-lit
  %+  cook  |=(t=tape `@`(crip t))
  (ifix [soq soq] (star ;~(less soq next)))
::  +expr: one nockasm expression
::
++  expr
  %+  knee  *nasm
  |.  ~+
  ;~  pose
    (stag %atom hex-lit)
    (stag %atom dem-lit)
    (stag %atom qut-lit)
    (stag %axis ;~(pfix dot nym))
    raw-cel
    op-app
    mac-let
    mac-match
  ==
::  +raw-cel: [a b c ...] — structural cell of >=2 expressions
::
++  raw-cel
  %+  cook
    |=  l=(list nasm)
    ?.  (gte (lent l) 2)
      ~|(%raw-cell-needs-two-elements !!)
    `nasm`[%cell l]
  (ifix [sel (tkn ser)] (plus (tkn expr)))
::  +op-app: (%opcode args ...)
::
++  op-app
  %+  cook  |=([op=@t as=(list nasm)] `nasm`[%op op as])
  %+  ifix  [pal (tkn par)]
  ;~(plug (tkn ;~(pfix cen nym)) (star (tkn expr)))
::  +mac-let: #let .name = EXPR in EXPR
::
++  mac-let
  %+  cook  |=([n=@t v=nasm b=nasm] `nasm`[%let n v b])
  ;~  plug
    ;~(pfix (jest '#let') (tkn ;~(pfix dot nym)))
    ;~(pfix (tkn tis) (tkn expr))
    ;~(pfix (tkn kwd-in) (tkn expr))
  ==
::  +kwd-in: the keyword 'in', not a prefix of a longer name
::
++  kwd-in
  ;~(sfix (jest 'in') ;~(less ;~(pose aln cab hep) (easy ~)))
::  +mac-match: #match EXPR { PAT => EXPR ... _ => EXPR }
::
++  mac-match
  %+  cook  make-match
  ;~  plug
    ;~(pfix (jest '#match') (tkn expr))
    %+  ifix  [(tkn kel) (tkn ker)]
    (star (tkn arm-rule))
  ==
++  arm-rule
  ;~  pose
    (stag %def ;~(pfix cab (tkn (jest '=>')) (tkn expr)))
    (stag %case ;~(plug expr ;~(pfix (tkn (jest '=>')) (tkn expr))))
  ==
::  +make-match: split parsed arms into cases and the required
::  default, preserving case order
::
++  make-match
  |=  [s=nasm items=(list marm)]
  ^-  nasm
  =/  cases=(list mcas)  ~
  =/  def=(unit nasm)  ~
  |-  ^-  nasm
  ?~  items
    ?~  def  ~|(%match-needs-default !!)
    [%match s (flop cases) u.def]
  ?-  -.i.items
    %def   ?^  def  ~|(%match-duplicate-default !!)
           $(items t.items, def `p.i.items)
    %case  $(items t.items, cases [[p.i.items q.i.items] cases])
  ==
::  +sch-rule: :subject schema — .name leaf or {schema+} group;
::  flat groups cons right-leaning per hoon convention
::
++  sch-rule
  %+  knee  *sema
  |.  ~+
  ;~  pose
    (stag %leaf ;~(pfix dot nym))
    %+  cook
      |=  l=(lest sema)
      |-  ^-  sema
      ?~  t.l  i.l
      [%pair i.l $(l t.l)]
    (ifix [kel (tkn ker)] (plus (tkn sch-rule)))
  ==
::  +apex: whole program — optional :subject schema, one expression
::
++  apex
  ;~  sfix
    ;~  plug
      (punt (tkn ;~(pfix (jest ':subject') (tkn sch-rule))))
      (tkn expr)
    ==
    woc
  ==
::  +parse: cord -> [schema ast]; crashes on syntax error
::
++  parse
  |=  src=@t
  ^-  [sch=(unit sema) ast=nasm]
  (scan (trip src) apex)
::
+|  %expansion
::
::  +expand: the library's front door: .nasm source -> nock formula
::
++  expand
  |=  src=@t
  ^-  *
  =/  [sch=(unit sema) ast=nasm]  (parse src)
  =/  axes=(map @t @ud)
    ?~(sch ~ (sch-axes u.sch 1))
  (expa ast axes)
::  +sch-axes: resolve a schema to name->axis, rooted at base
::
++  sch-axes
  |=  [s=sema base=@ud]
  ^-  (map @t @ud)
  ?-  -.s
    %leaf  (my [p.s base]~)
    %pair
      =/  hed  $(s p.s, base (mul 2 base))
      =/  tal  $(s q.s, base +((mul 2 base)))
      =/  dup  ~(tap by (~(int by hed) tal))
      ?^  dup  ~|([%duplicate-schema-name p.i.dup] !!)
      (~(uni by hed) tal)
  ==
::  +shift-axes: when the subject becomes [new old], every old
::  axis n moves to (peg 3 n)
::
++  shift-axes
  |=  axes=(map @t @ud)
  ^-  (map @t @ud)
  (~(run by axes) |=(a=@ud (peg 3 a)))
::  +lift: a bare atom in formula position becomes [1 atom]
::
++  lift  |=(n=* ?@(n [1 n] n))
::  +form: expand in formula position (lifting)
::
++  form
  |=  [a=nasm axes=(map @t @ud)]
  ^-  *
  (lift (expa a axes))
::  +ax-arg: expand in axis position (must be an atom)
::
++  ax-arg
  |=  [a=nasm axes=(map @t @ud)]
  ^-  @
  =/  v  (expa a axes)
  ?^  v  ~|([%axis-arg-must-be-atom v] !!)
  v
::  +expa: expand an expression against the current axis map
::
++  expa
  |=  [ast=nasm axes=(map @t @ud)]
  ^-  *
  ?-  -.ast
    %atom  p.ast
  ::
    %axis
      =/  a  (~(get by axes) p.ast)
      ?~  a
        ~|  [%unbound-axis p.ast %declared ~(tap in ~(key by axes))]
        !!
      [0 u.a]
  ::
    %cell
      =/  l  (turn p.ast |=(a=nasm (expa a axes)))
      |-  ^-  *
      ?~  l  ~|(%empty-raw-cell !!)
      ?~  t.l  i.l
      [i.l $(l t.l)]
  ::
    %op  (expa-op p.ast q.ast axes)
  ::
    %let
      ::  value compiled against the old subject; body sees the
      ::  binding at axis 2 and everything else pegged under 3
      ::
      =/  v  (form q.ast axes)
      =/  new  (shift-axes axes)
      ?:  (~(has by new) p.ast)
        ~|([%let-shadows p.ast] !!)
      [8 v (form r.ast (~(put by new) p.ast 2))]
  ::
    %match
      ::  scrutinee lifted once via opcode 8 to axis 2; each arm is
      ::  an opcode-6 dispatch on opcode-5 equality with the literal
      ::  pattern; patterns expand unlifted (they are compared as
      ::  values, wrapped [1 pat] below)
      ::
      =/  s  (form p.ast axes)
      =/  new  (shift-axes axes)
      =/  res  `*`(form r.ast new)
      =/  cs  (flop q.ast)
      |-  ^-  *
      ?~  cs  [8 s res]
      =/  pav  (expa p.i.cs new)
      =/  bov  (form q.i.cs new)
      $(cs t.cs, res [6 [5 [1 pav] [0 2]] bov res])
  ==
::  +expa-op: expand one (%opcode ...) application. argument kinds
::  follow the python's OPS table: +form for 'f', +expa for 'n',
::  +ax-arg for 'a'; arity is enforced by the list pattern
::
++  expa-op
  |=  [op=@t as=(list nasm) axes=(map @t @ud)]
  ^-  *
  ~|  [%opcode op %args (lent as)]
  ?+  op  ~|(%unknown-opcode !!)
    %self     ?>(?=(~ as) [0 1])
    %battery  ?>(?=(~ as) [0 2])
    %payload  ?>(?=(~ as) [0 3])
    %sample   ?>(?=(~ as) [0 6])
    %context  ?>(?=(~ as) [0 7])
    %crash    ?>(?=(~ as) [0 0])
    %slot     ?>(?=([* ~] as) [0 (ax-arg i.as axes)])
    %const    ?>(?=([* ~] as) [1 (expa i.as axes)])
    %arm      ?>(?=([* ~] as) [1 (expa i.as axes)])
    %eval     ?>(?=([* * ~] as) [2 (form i.as axes) (form i.t.as axes)])
    %isa      ?>(?=([* ~] as) [3 (form i.as axes)])
    %inc      ?>(?=([* ~] as) [4 (form i.as axes)])
    %eq       ?>(?=([* * ~] as) [5 (form i.as axes) (form i.t.as axes)])
    %if       ?>  ?=([* * * ~] as)
              [6 (form i.as axes) (form i.t.as axes) (form i.t.t.as axes)]
    %comp     ?>(?=([* * ~] as) [7 (form i.as axes) (form i.t.as axes)])
    %push     ?>(?=([* * ~] as) [8 (form i.as axes) (form i.t.as axes)])
    %call     ?>(?=([* * ~] as) [9 (ax-arg i.as axes) (form i.t.as axes)])
    %edit     ?>  ?=([* * * ~] as)
              [10 [(ax-arg i.as axes) (form i.t.as axes)] (form i.t.t.as axes)]
    %hint     ?>(?=([* * ~] as) [11 (expa i.as axes) (form i.t.as axes)])
    %hintd    ?>  ?=([* * * ~] as)
              [11 [(expa i.as axes) (form i.t.as axes)] (form i.t.t.as axes)]
  ==
::
+|  %target
::
::    the compiler-target contract (doc/compiler-target.md): $nasm as
::    a versioned IR, +lower to canonical nock, +render to canonical
::    .nasm text. the round-trip law, checked by the differential
::    suite against the python implementation:
::
::      (expand (render sch ast))  ===  (lower sch ast)
::
::    +render is byte-identical to the python renderer: every layout
::    decision is a pure function of the IR value, the indent, and
::    the reserve (characters an enclosing form will append to the
::    final line), with a 76-column limit.
::
::  +nasm-version: version of the IR node set, lowering equations,
::  and canonical rendering rules. append-only.
::
++  nasm-version  1
::  +lower: IR -> canonical nock formula
::
++  lower
  |=  [sch=(unit sema) ast=nasm]
  ^-  *
  (expa ast ?~(sch ~ (sch-axes u.sch 1)))
::  +render: IR -> canonical .nasm source
::
++  render
  |=  [sch=(unit sema) ast=nasm]
  ^-  @t
  =/  lines=(list tape)
    %+  weld
      ^-  (list tape)
      ?~(sch ~ [(weld ":subject " (sema-text u.sch)) ~])
    (rend ast 0 0)
  (crip (zing (turn lines |=(t=tape (snoc t '\0a')))))
::  +sema-text: schemas always render wide; right spines flatten
::
++  sema-text
  |=  s=sema
  ^-  tape
  ?:  ?=(%leaf -.s)
    ['.' (trip p.s)]
  =/  elems
    =/  cur=sema  s
    =|  acc=(list sema)
    |-  ^-  (list sema)
    ?.  ?=(%pair -.cur)  (flop `(list sema)`[cur acc])
    $(acc [p.cur acc], cur q.cur)
  :(weld "\{" (join-tapes " " (turn elems sema-text)) "}")
::  +atom-text: cord form iff >=2 bytes, all printable ascii, no
::  quote; else dotted decimal
::
++  atom-text
  |=  n=@
  ^-  tape
  ?:  ?&  (gte n 256)
          %+  levy  (trip n)
          |=(c=@tD &((gte c 32) (lte c 126) !=(c 39)))
      ==
    "'{(trip n)}'"
  (dotted n)
::  +dotted: decimal with dots every three digits
::
++  dotted
  |=  n=@
  ^-  tape
  %-  regroup
  ?:  =(0 n)  "0"
  =|  acc=tape
  |-  ^-  tape
  ?:  =(0 n)  acc
  $(acc [(add '0' (mod n 10)) acc], n (div n 10))
++  regroup
  |=  s=tape
  ^-  tape
  =/  r  (flop s)
  =|  out=tape
  =/  i  0
  |-  ^-  tape
  ?~  r  out
  =?  out  &(!=(i 0) =(0 (mod i 3)))  ['.' out]
  $(out [i.r out], r t.r, i +(i))
::  +wide: single-line form, or ~ (#let / #match have no wide form)
::
++  wide
  |=  e=nasm
  ^-  (unit tape)
  ?-  -.e
    %atom  `(atom-text p.e)
    %axis  `['.' (trip p.e)]
    %cell
      ?~  ws=(wide-all p.e)  ~
      `:(weld "[" (join-tapes " " u.ws) "]")
    %op
      ?~  q.e  `:(weld "(%" (trip p.e) ")")
      ?~  ws=(wide-all q.e)  ~
      `:(weld "(%" (trip p.e) " " (join-tapes " " u.ws) ")")
    %let    ~
    %match  ~
  ==
++  wide-all
  |=  l=(list nasm)
  ^-  (unit (list tape))
  ?~  l  `~
  =/  w  (wide i.l)
  ?~  w  ~
  =/  r  $(l t.l)
  ?~  r  ~
  `[u.w u.r]
::  +rend: render at an indent as a list of lines (indent included).
::  res is the reserve: characters an enclosing form will append to
::  this expression's final line (closing delimiters), so that width
::  decisions account for them and no emitted line exceeds 76.
::
++  rend
  |=  [e=nasm ind=@ud res=@ud]
  ^-  (list tape)
  =/  pad  (reap ind ' ')
  =/  w  (wide e)
  ?:  &(?=(^ w) (lte :(add ind (lent u.w) res) 76))
    [(weld pad u.w) ~]
  ?-  -.e
    %atom  [(weld pad (atom-text p.e)) ~]
    %axis  [(weld pad ['.' (trip p.e)]) ~]
  ::
    %cell
      ?>  ?=([* * *] p.e)
      =/  fst  (rend i.p.e (add ind 2) 0)
      ?>  ?=(^ fst)
      =/  l0  :(weld pad "[ " (slag (add ind 2) i.fst))
      %+  amend-last
        %+  weld  [l0 t.fst]
        =/  els  `(list nasm)`t.p.e
        |-  ^-  (list tape)
        ?~  els  ~
        ?~  t.els  (rend i.els (add ind 2) +(res))
        (weld (rend i.els (add ind 2) 0) $(els t.els))
      "]"
  ::
    %op
      ?~  q.e  [:(weld pad "(%" (trip p.e) ")") ~]
      %+  amend-last
        %+  weld  [:(weld pad "(%" (trip p.e)) ~]
        =/  args  `(list nasm)`q.e
        |-  ^-  (list tape)
        ?~  args  ~
        ?~  t.args  (rend i.args (add ind 2) +(res))
        (weld (rend i.args (add ind 2) 0) $(args t.args))
      ")"
  ::
    %let
      =/  head  :(weld pad "#let ." (trip p.e) " =")
      =/  one=(unit tape)
        ?~  vw=(wide q.e)  ~
        `:(weld head " " u.vw " in")
      %+  weld
        ^-  (list tape)
        ?:  &(?=(^ one) (lte (lent u.one) 76))
          [u.one ~]
        ;:  weld
          [head ~]
          (rend q.e (add ind 2) 0)
          [(weld pad "in") ~]
        ==
      (rend r.e ind res)
  ::
    %match
      =/  one=(unit tape)
        ?~  sw=(wide p.e)  ~
        `:(weld pad "#match " u.sw " \{")
      =/  head=(list tape)
        ?:  &(?=(^ one) (lte (lent u.one) 76))
          [u.one ~]
        ;:  weld
          [(weld pad "#match") ~]
          (rend p.e (add ind 2) 0)
          [(weld pad "\{") ~]
        ==
      ;:  weld
        head
        =/  cs  `(list mcas)`q.e
        |-  ^-  (list tape)
        ?~  cs  ~
        (weld (rend-case `p.i.cs q.i.cs (add ind 2)) $(cs t.cs))
        (rend-case ~ r.e (add ind 2))
        [(weld pad "}") ~]
      ==
  ==
::  +rend-case: one #match arm; ~ pattern means the _ default
::
++  rend-case
  |=  [pat=(unit nasm) bod=nasm ind=@ud]
  ^-  (list tape)
  =/  pad  (reap ind ' ')
  =/  pw=(unit tape)  ?~(pat `"_" (wide u.pat))
  =/  bw  (wide bod)
  ?:  ?&  ?=(^ pw)  ?=(^ bw)
          (lte (lent :(weld pad u.pw " => " u.bw)) 76)
      ==
    [:(weld pad u.pw " => " u.bw) ~]
  ?:  &(?=(^ pw) (lte (lent :(weld pad u.pw " =>")) 76))
    (weld [:(weld pad u.pw " =>") ~] (rend bod (add ind 2) 0))
  =/  pl  ?~(pat [(weld pad "_") ~] (rend u.pat ind 3))
  (weld (amend-last pl " =>") (rend bod (add ind 2) 0))
::  +amend-last: append a suffix to the final line
::
++  amend-last
  |=  [ls=(list tape) suf=tape]
  ^-  (list tape)
  ?~  ls  [suf ~]
  ?~  t.ls  [(weld i.ls suf) ~]
  [i.ls $(ls t.ls)]
++  join-tapes
  |=  [sep=tape ts=(list tape)]
  ^-  tape
  ?~  ts  ~
  |-  ^-  tape
  ?~  t.ts  i.ts
  :(weld i.ts sep $(ts t.ts))
--
