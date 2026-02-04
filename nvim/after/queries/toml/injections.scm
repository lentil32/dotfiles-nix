; extends

(pair
  (bare_key) @_key
  (string) @injection.content @injection.language
  (#eq? @_key "run")
  (#is-mise?)
  (#match? @injection.language "^['\"]{3}\n*#!(/\\w+)+/env\\s+\\w+")
  (#gsub! @injection.language "^.*#!/.*/env%s+([^%s]+).*" "%1")
  (#offset! @injection.content 0 3 0 -3)
)

(pair
  (bare_key) @_key
  (string) @injection.content @injection.language
  (#eq? @_key "run")
  (#is-mise?)
  (#match? @injection.language "^['\"]{3}\n*#!(/\\w+)+\\s*\n")
  (#gsub! @injection.language "^.*#!/.*/([^/%s]+).*" "%1")
  (#offset! @injection.content 0 3 0 -3)
)

(pair
  (bare_key) @_key
  (string) @injection.content
  (#eq? @_key "run")
  (#is-mise?)
  (#match? @injection.content "^['\"]{3}\n*.*")
  (#not-match? @injection.content "^['\"]{3}\n*#!")
  (#offset! @injection.content 0 3 0 -3)
  (#set! injection.language "bash")
)

(pair
  (bare_key) @_key
  (string) @injection.content
  (#eq? @_key "run")
  (#is-mise?)
  (#not-match? @injection.content "^['\"]{3}")
  (#offset! @injection.content 0 1 0 -1)
  (#set! injection.language "bash")
)
