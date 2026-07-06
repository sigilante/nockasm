::  tests for /lib/nockasm
::
::    representative cases from the python conformance suite
::    (test_nockasm.py); run with -test %/tests/lib/nockasm ~
::    the full differential suite (every python unit test plus the
::    urbit/benchmark transcriptions) lives in test_hoon.py at the
::    repository root and runs via `urbit eval`, no ship required.
::
/+  *test, nockasm
|%
+|  %helpers
++  expand  expand:nockasm
::  +nock: run a formula against a subject (typed wrapper over .*)
::
++  nock  |=([sub=* fol=*] .*(sub fol))
::
+|  %atoms
++  test-decimal
  (expect-eq !>(`*`42) !>((expand '42')))
++  test-decimal-separators
  (expect-eq !>(`*`1.000) !>((expand '1.000')))
++  test-hex
  (expect-eq !>(`*`42) !>((expand '0x2a')))
++  test-cord
  (expect-eq !>(`*`1.953.718.630) !>((expand '\'fast\'')))
::
+|  %opcodes
++  test-inc
  (expect-eq !>(`*`[4 0 1]) !>((expand '(%inc (%slot 1))')))
++  test-eq
  (expect-eq !>(`*`[5 [0 2] 0 3]) !>((expand '(%eq (%slot 2) (%slot 3))')))
++  test-if
  (expect-eq !>(`*`[6 [0 1] [1 0] 1 1]) !>((expand '(%if (%slot 1) 0 1)')))
++  test-edit
  %+  expect-eq
    !>(`*`[10 [6 4 0 1] 0 1])
  !>((expand '(%edit 6 (%inc (%slot 1)) (%slot 1))'))
++  test-hint-static
  %+  expect-eq
    !>(`*`[11 1.953.718.630 0 1])
  !>((expand '(%hint \'fast\' (%slot 1))'))
++  test-hint-dynamic
  %+  expect-eq
    !>(`*`[11 [1.953.718.630 1 0] 0 1])
  !>((expand '(%hintd \'fast\' 0 (%slot 1))'))
++  test-axis-aliases
  %+  expect-eq
    !>(`*`[[0 1] [0 2] [0 3] [0 6] [0 7] 0 0])
  !>((expand '[(%self) (%battery) (%payload) (%sample) (%context) (%crash)]'))
::
+|  %schemas
++  test-schema-flat
  (expect-eq !>(`*`[0 6]) !>((expand ':subject {.a .b .c}  .b')))
++  test-schema-nested
  (expect-eq !>(`*`[0 5]) !>((expand ':subject {{.a .b} .c}  .b')))
::
+|  %macros
++  test-let
  %+  expect-eq
    !>(`*`[8 [4 0 1] 5 [0 2] 0 3])
  !>((expand ':subject .x  #let .d = (%inc .x) in (%eq .d .x)'))
++  test-let-nested
  =/  src
    '''
    :subject .x
    #let .a = (%inc .x) in
    #let .b = (%inc .a) in
    (%eq .a .b)
    '''
  %+  expect-eq
    !>(`*`[8 [4 0 1] 8 [4 0 2] 5 [0 6] 0 2])
  !>((expand src))
++  test-match
  =/  src
    '''
    :subject {.tag .data}
    #match .tag { 1 => (%inc .data)  _ => 0 }
    '''
  %+  expect-eq
    !>(`*`[8 [0 2] 6 [5 [1 1] 0 2] [4 0 7] 1 0])
  !>((expand src))
::
+|  %execution
++  test-run-let
  =/  src
    '''
    :subject {.before .target .after}
    #let .next = (%inc .target) in
      [.before .next .after]
    '''
  %+  expect-eq
    !>(`*`[10 42 99])
  !>((nock [10 41 99] (expand src)))
++  test-run-distribution
  %+  expect-eq
    !>(`*`[4 6])
  !>((nock [3 5] (expand ':subject {.a .b}  [(%inc .a) (%inc .b)]')))
::
+|  %errors
++  test-unbound-axis
  (expect-fail |.((expand '(%inc .x)')))
++  test-unknown-opcode
  (expect-fail |.((expand '(%nope 1)')))
++  test-wrong-arity
  (expect-fail |.((expand '(%inc 1 2)')))
++  test-match-needs-default
  (expect-fail |.((expand ':subject .x #match .x { 1 => 0 }')))
++  test-let-shadow
  (expect-fail |.((expand ':subject .x #let .x = 1 in .x')))
++  test-duplicate-schema-name
  (expect-fail |.((expand ':subject {.a .a} .a')))
++  test-trailing-tokens
  (expect-fail |.((expand '42 42')))
--
