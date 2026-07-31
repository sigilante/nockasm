::  nockasm: shared types for the nockasm target IR
::
::    the spec surface of doc/compiler-target.md, factored so that
::    downstream toolchains (compilers emitting $nasm, debuggers,
::    formatters) vendor the types without the implementation.
::    /lib/nockasm is the reference implementation over these types
::    and imports this file; it depends on nothing else.
::
|%
+|  %vocabulary
::  +nasm-of: the nockasm expression vocabulary, parameterized over
::  its own recursion. $nasm below is the plain self-instantiation;
::  an annotated instantiation (a debugger's span-carrying AST, a
::  provenance-carrying emitter IR) ties the knot through its own
::  wrapper instead -- under an explicit $~ bunt, or the
::  nest-checker's bunt derivation recurses forever (%over):
::
::      +$  noted
::        $~  [*note [%atom 0]]
::        [=note node=(nasm-of noted)]
::
::  and is thereby a first-class citizen of the vocabulary rather
::  than a fork of it: every case, including any appended later, is
::  inherited by construction.
::
::    %atom   integer, hex, or cord literal (value already packed)
::    %axis   .name reference into the subject schema
::    %cell   raw [a b c] cell; elements expanded structurally
::    %op     (%opcode args) application
::    %let    #let p = q in r
::    %match  #match p { q... _ => r }
::    %nock   (%nock p): an already-formed Nock formula embedded as
::            a raw noun. boundary embedding for foreign-produced
::            formulas (FFI glue, precompiled fragments) -- not a
::            general escape hatch. expansion is identity: the
::            expander never recurses into, validates, or rewrites
::            p; well-formedness is the producer's responsibility
::            and tooling treats the payload as opaque.
::
++  nasm-of
  |$  [self]
  $%  [%atom p=@]
      [%axis p=@t]
      [%cell p=(list self)]
      [%op p=@t q=(list self)]
      [%let p=@t q=self r=self]
      [%match p=self q=(list (mcas-of self)) r=self]
      [%nock p=*]
  ==
::  +mcas-of: one #match case, pattern and body, parameterized like
::  +nasm-of. a named builder rather than an inline pair: an inline
::  (list [p=self q=self]) inside the recursive $% sends the
::  nest-checker into a stack overflow (%over); routing the
::  recursion through a named hold keeps it finite.
::
++  mcas-of
  |$  [self]
  [p=self q=self]
::
+|  %types
::  $nasm: a parsed nockasm expression: the plain self-instantiation
::  of the vocabulary, and the IR type of the compiler-target
::  contract.
::
+$  nasm  $~([%atom 0] (nasm-of nasm))
::  $mcas: one #match case of the plain instantiation
::
+$  mcas  (mcas-of nasm)
::  $sema: subject axis schema, as declared by :subject or
::  constructed directly as data by an emitter (a compiler holding a
::  binary layout tree and a name->axis map projects them into $sema
::  mechanically, never via text). names, structure, and axes only
::  -- never type information of any kind; a downstream type system
::  projects onto $sema by dropping types.
::
::    %leaf   a named position: .name bound to this axis
::    %pair   nested cell structure, to arbitrary depth
::    %hole   an anonymous position: subject structure with no name
::            bound (renders as _). machine-generated schemas mirror
::            subject shape and leave unnamed axes as holes.
::
::  uniqueness contract: a name may appear at most once in a schema.
::  uniqueness is the producer's obligation -- an emitter uniquifies
::  before constructing $sema; the expander rejects duplicates at
::  resolution time (%duplicate-schema-name crash) and defines no
::  shadowing semantics. holes are not names and repeat freely.
::
+$  sema
  $~  [%leaf '']
  $%  [%leaf p=@t]
      [%pair p=sema q=sema]
      [%hole ~]
  ==
::  $opco: the named-opcode vocabulary of %op forms, as a closed
::  term set. load-bearing: +expa-op refines against $opco and
::  dispatches exhaustively, so appending a term here without
::  handling it there is a compile error. $nasm still deliberately
::  keeps %op's head wide (p=@t) -- parse stays permissive, and an
::  unknown opcode is a crash at expansion time (%unknown-opcode),
::  matching the python oracle, not a parse-time type error. %nock
::  is not here: it is a vocabulary case of its own, not an %op.
::
+$  opco
  $?  %self  %battery  %payload  %sample  %context  %crash
      %slot  %const  %arm  %eval  %isa  %inc  %eq  %if
      %comp  %push  %call  %edit  %hint  %hintd
  ==
::
+|  %version
::  +nasm-version: version of the IR node set, lowering equations,
::  and canonical rendering rules. append-only.
::  2: the %nock opaque-embed case, $sema %hole anonymous positions,
::  and the lift fallback moving from structural raw cells to %nock
::  (pure vocabulary extension: every v1 value lowers and renders
::  exactly as before).
::
++  nasm-version  2
--
