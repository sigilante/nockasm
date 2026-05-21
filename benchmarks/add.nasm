;  add: unsigned addition of [a b].
;
;  Faithful transcription of urbit/benchmark/desk/bar/add.nock, drilled
;  fully into named opcodes.
;
;  Subject: a cell [a b].
;  Result : a + b.

(%push
  (%const
    [ ;  +add arm (axis 4 of [core a_b]). Pushes [0 0] as sample
      ;  placeholder, then autocons [add-body subject] to make a gate.
      (%push (%const [0 0])
             [ (%arm
                 ;  +add body. Subject when entered: [body sample payload].
                 ;  axis 12 = a, axis 13 = b, axis 28 = (axis 14 of payload).
                 (%if (%eq (%const 0) (%slot 12))
                      (%slot 13)
                      (%call 2
                        (%edit 6
                          [ (%push (%call 10 (%context))
                                   (%call 2 (%edit 6 (%slot 28) (%battery))))
                            (%inc (%slot 13)) ]
                          (%self)))))
               (%self) ])

      ;  +dec arm (axis 10 of [core a_b]). Same shape as dec.nasm.
      (%push (%const 0)
             [ (%arm
                 (%if (%eq (%const 0) (%sample))
                      (%crash)
                      (%push (%const 0)
                             (%push
                               (%arm
                                 (%if (%eq (%slot 30) (%inc (%sample)))
                                      (%sample)
                                      (%call 2 (%edit 6
                                                       (%inc (%sample))
                                                       (%self)))))
                               (%call 2 (%self))))))
               (%self) ])

      ;  Main body: push +add core, then call its arm with [m n] sample.
      (%push (%call 4 (%self))
             (%call 2 (%edit 6 [(%slot 14) (%slot 15)] (%battery)))) ])

  ;  Invoke main body at axis 11 of [core a_b].
  (%call 11 (%self)))
