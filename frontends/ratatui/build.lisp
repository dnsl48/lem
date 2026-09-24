(ql:quickload :lem-ratatui)

(lem:init-at-build-time)

;; Written to the frontend's dist/, which holds every build output, rather
;; than the repo root, so the PoC leaves nothing outside frontends/ratatui/.
(defparameter *image* (asdf:system-relative-pathname :lem-ratatui "dist/lem-ratatui-lisp"))

(ensure-directories-exist *image*)

(sb-ext:save-lisp-and-die *image*
                          :toplevel #'lem-ratatui:main
                          :executable t)
