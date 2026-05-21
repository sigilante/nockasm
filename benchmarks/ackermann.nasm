;  ackermann: ack(m, n) on a [m n] subject.
;
;  Faithful transcription of urbit/benchmark/desk/bar/ackermann.nock,
;  drilled into named opcodes. The core has just two members: the
;  main arm (axis 4) — which self-recurses via %call 4 — and the +dec
;  arm wrapper (axis 5/10).
;
;  Note: in this |^ trap the payload is [m n], so axis 6 = m and
;  axis 7 = n. The %sample and %context aliases line up by axis but
;  not by their usual gate-call semantics — they are just convenient
;  names for [0 6] and [0 7] here.
;
;  Subject: a cell [m n].
;  Result : ack(m, n).

(%push
  (%const
    [ ;  Main arm (axis 4 of [core m_n]). Standard Ackermann recursion.
      (%if (%eq (%const 0) (%sample))
           ;  m == 0  ->  +(n)
           (%inc (%context))
           (%if (%eq (%const 0) (%context))
                ;  n == 0  ->  $(m (dec m), n 1)
                (%call 4
                       (%edit 3
                              [ (%push (%call 5 (%self))
                                       (%call 2 (%edit 6 (%slot 14) (%battery))))
                                (%const 1) ]
                              (%self)))
                ;  else    ->  $(m (dec m), n $(n (dec n)))
                (%call 4
                       (%edit 3
                              [ (%push (%call 5 (%self))
                                       (%call 2 (%edit 6 (%slot 14) (%battery))))
                                (%call 4
                                       (%edit 7
                                              (%push (%call 5 (%self))
                                                     (%call 2 (%edit 6 (%slot 15) (%battery))))
                                              (%self))) ]
                              (%self)))))

      ;  +dec arm wrapper (axis 5 of [core m_n]). Same body as dec.nasm.
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
               (%self) ]) ])

  (%call 4 (%self)))
