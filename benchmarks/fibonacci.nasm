;  fibonacci: naive recursive Fibonacci of a single-atom subject.
;
;  Faithful transcription of urbit/benchmark/desk/bar/fibonacci.nock,
;  drilled into named opcodes. Core layout differs from factorial:
;  only two arms in the |^ core (+add at axis 4, +dec at axis 10);
;  the main body (axis 11) is the |= anonymous gate that recurses on
;  itself via %context-based self-call.
;
;  Subject: a single atom n.
;  Result : fib(n)  (slow — exponential).

(%push
  (%const
    [ ;  +add arm (axis 4 of [core m]). Same +add as in add.nasm; calls
      ;  +dec via axis 10.
      (%push (%const [0 0])
             [ (%arm
                 (%if (%eq (%const 0) (%slot 12))
                      (%slot 13)
                      (%call 2
                        (%edit 6
                          [ (%push (%call 10 (%context))
                                   (%call 2 (%edit 6 (%slot 28) (%battery))))
                            (%inc (%slot 13)) ]
                          (%self)))))
               (%self) ])

      ;  +dec arm (axis 10 of [core m]). Same body as dec.nasm.
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
               (%self) ])

      ;  Main body (axis 11): build the |= gate, call it with m at sample.
      (%push (%push (%const 0)
                    [ (%arm
                        ;  Gate body: if n==1 -> 1; if n==2 -> 1;
                        ;  else (add fib(n-1) fib(n-2)).
                        (%if (%eq (%sample) (%const 1))
                             (%const 1)
                             (%if (%eq (%sample) (%const 2))
                                  (%const 1)
                                  ;  add of two recursive fib calls.
                                  (%push (%call 4 (%context))
                                         (%call 2
                                           (%edit 6
                                             [ ;  fib(n-1):
                                               (%comp (%payload)
                                                      (%call 2
                                                        (%edit 6
                                                          (%push (%call 10 (%context))
                                                                 (%call 2 (%edit 6 (%slot 14) (%battery))))
                                                          (%self))))
                                               ;  fib(n-2):
                                               (%comp (%payload)
                                                      (%call 2
                                                        (%edit 6
                                                          (%push (%call 10 (%context))
                                                                 (%call 2
                                                                   (%edit 6
                                                                     (%comp (%payload)
                                                                            (%push (%call 10 (%context))
                                                                                   (%call 2 (%edit 6 (%slot 14) (%battery)))))
                                                                     (%battery))))
                                                          (%self)))) ]
                                             (%battery)))))))
                      (%self) ])
             (%call 2 (%edit 6 (%context) (%battery)))) ])

  (%call 11 (%self)))
