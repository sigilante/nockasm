;  factorial: tail-recursive factorial of a single-atom subject.
;
;  Faithful transcription of urbit/benchmark/desk/bar/factorial.nock,
;  drilled into named opcodes. The core has three explicit arms — +mul
;  (axis 4), +add (axis 20), +dec (axis 21) — and a main body (axis 11)
;  that builds an anonymous |= gate and invokes it on m.
;
;  Subject: a single atom m.
;  Result : m! (use small m; pure Nock multiplication is slow).

(%push
  (%const
    [ ;  +mul arm (axis 4 of [core m]). Standard `|: [a=`@`1 b=`@`1]`
      ;  pattern: push the [1 1] default sample, then autocons
      ;  [mul-body subject].
      (%push (%const [1 1])
             [ (%arm
                 ;  +mul body. Push c=0 accumulator, then push the |- arm
                 ;  and call it.
                 (%push (%const 0)
                        (%push (%arm
                                 ;  |- loop on a (axis 60) with c (axis 6).
                                 ;  if a == 0  -> return c
                                 ;  else recurse with a := dec(a), c := add(b, c)
                                 (%if (%eq (%const 0) (%slot 60))
                                      (%sample)
                                      (%call 2
                                        (%edit 60
                                          (%push (%call 21 (%slot 31))
                                                 (%call 2 (%edit 6 (%slot 124) (%battery))))
                                          (%edit 6
                                            (%push (%call 20 (%slot 31))
                                                   (%call 2 (%edit 6 [(%slot 125) (%slot 14)] (%battery))))
                                            (%self))))))
                               (%call 2 (%self)))))
               (%self) ])

      ;  Arms cell at axis 5 of [core m]:
      ;    head (axis 10 of [core m]) = +add arm
      ;    tail (axis 11 of [core m]) starts with `8 [1 0] ...` — the
      ;    +dec arm wrapped as a noun-positioned formula.
      ;  NOTE: axis 11 is also where the main body lives — it shares a
      ;  prefix with the +dec wrapper, hence the structural mix here.
      [ (%push (%const [0 0])
               [ (%arm
                   ;  +add body — uses +dec at axis 21.
                   (%if (%eq (%const 0) (%slot 12))
                        (%slot 13)
                        (%call 2
                          (%edit 6
                            [ (%push (%call 21 (%context))
                                     (%call 2 (%edit 6 (%slot 28) (%battery))))
                              (%inc (%slot 13)) ]
                            (%self)))))
                 (%self) ])

        ;  +dec arm (axis 21 of [core m]) — identical body to dec.nasm.
        (%push (%const 0)
               [ (%arm
                   (%if (%eq (%const 0) (%sample))
                        (%crash)
                        (%push (%const 0)
                               (%push (%arm
                                        (%if (%eq (%slot 30) (%inc (%sample)))
                                             (%sample)
                                             (%call 2 (%edit 6
                                                              (%inc (%sample))
                                                              (%self)))))
                                      (%call 2 (%self))))))
                 (%self) ]) ]

      ;  Main body (also at axis 11): build the anonymous |= gate, then
      ;  call its battery with m installed at sample.
      (%push (%push (%const 0)
                    [ (%arm
                        ;  |= gate body: =/ t=1; |- ...
                        (%push (%const 1)
                               (%push (%arm
                                        ;  |- loop on n (axis 30), t (sample).
                                        ;  if n == 1 -> t
                                        ;  else recurse n := dec(n), t := mul(t, n)
                                        (%if (%eq (%slot 30) (%const 1))
                                             (%sample)
                                             (%call 2
                                               (%edit 30
                                                 (%push (%call 21 (%slot 31))
                                                        (%call 2 (%edit 6 (%slot 62) (%battery))))
                                                 (%edit 6
                                                   (%push (%call 4 (%slot 31))
                                                          (%call 2 (%edit 6 [(%slot 14) (%slot 62)] (%battery))))
                                                   (%self))))))
                                      (%call 2 (%self)))))
                      (%self) ])
             (%call 2 (%edit 6 (%context) (%battery)))) ])

  (%call 11 (%self)))
