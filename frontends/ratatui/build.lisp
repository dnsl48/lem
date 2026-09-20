(ql:quickload :lem-ratatui)

(lem:init-at-build-time)

;; Written beside the system rather than into the repo root, so the PoC
;; leaves nothing outside frontends/ratatui/.
(sb-ext:save-lisp-and-die (asdf:system-relative-pathname :lem-ratatui "lem-ratatui-lisp")
                          :toplevel #'lem-ratatui:main
                          :executable t)
