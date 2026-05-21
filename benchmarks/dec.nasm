;  dec: naive unsigned decrement.
;
;  Faithful transcription of urbit/benchmark/desk/bar/dec.nock, drilled
;  fully into named opcodes. Uses %arm (intent-marked %const) for the
;  formulas later invoked via %call, %crash for [0 0], and the standard
;  axis aliases %self / %battery / %payload / %sample / %context.
;
;  Subject: a single atom m  (asserted nonzero).
;  Result : m - 1.

(%push
  ;  Push the |^ core onto the subject stack; new subject = [core m].
  (%const
    [ ;  main arm at axis 4: build the dec sub-core via the core tail,
      ;  then call its first arm with m installed at sample.
      (%push (%call 5 (%self))
             (%call 2 (%edit 6 (%context) (%battery))))

      ;  core tail at axis 5: a formula that, when invoked, autocons
      ;  [dec-arm subject] to assemble a gate-shaped sub-core.
      (%push (%const 0)
             [ (%arm
                 ;  +dec arm body. Subject when called is [arm payload]
                 ;  with the argument at sample, original at axis 30.
                 (%if (%eq (%const 0) (%sample))
                      (%crash)
                      (%push (%const 0)
                             (%push
                               (%arm
                                 ;  inner |- loop on candidate b at sample:
                                 ;  if a == +(b) return b, else recurse b+1
                                 (%if (%eq (%slot 30) (%inc (%sample)))
                                      (%sample)
                                      (%call 2 (%edit 6
                                                       (%inc (%sample))
                                                       (%self)))))
                               (%call 2 (%self))))))
               (%self) ]) ])

  ;  Call the main arm at axis 4 of the new subject.
  (%call 4 (%self)))
