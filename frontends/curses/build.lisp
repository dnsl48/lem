(ql:quickload :lem-curses)

(lem:init-at-build-time)

;; Written to the frontend's dist/, which holds every build output, rather
;; than the repo root, so the PoC leaves nothing outside frontends/curses/.
(defparameter *image* (asdf:system-relative-pathname :lem-curses "dist/lem-curses-lisp"))

(ensure-directories-exist *image*)

(sb-ext:save-lisp-and-die *image*
                          :toplevel #'lem-curses:main
                          :executable t)
