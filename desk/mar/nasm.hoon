::  nasm: Nock Assembly source
::
::    the noun form is the source text (a cord), exactly parallel to
::    %hoon: storage and transport stay textual, and expansion to a
::    Nock formula is a library concern (/lib/nockasm, +expand), not
::    a storage concern. this keeps the mark honest -- a .nasm file
::    with a syntax error is still a file -- and keeps one canonical
::    noun form across Urbit (clay), NockApp (hoonc loads non-hoon
::    files as octs of the same bytes), and any text tooling.
::
|_  own=@t
::
++  grow
  |%
  ++  mime  `^mime`[/text/x-nockasm (as-octs:mimes:html own)]
  ++  txt   (to-wain:format own)
  --
++  grab
  |%
  ++  mime  |=([p=mite q=octs] q.q)
  ++  noun  @t
  ++  txt   of-wain:format
  --
++  grad  %txt
--
