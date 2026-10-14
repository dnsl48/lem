(defpackage :lem-curses/tests/main
  (:use :cl :rove))
(in-package :lem-curses/tests/main)

(deftest curses-interface-selection
  (ok (member :lem-curses *features*)
      "loading lem-curses exposes its conditional configuration feature")
  (let* ((args (lem:parse-args '("--interface" "curses")))
         (implementation
           (lem-core::get-default-implementation
            :implementation (lem:command-line-arguments-interface args))))
    (ok (typep implementation 'lem-curses/implementation:curses)
        "the public CLI selector constructs the curses frontend")
    (ok (eq :curses (lem-core::implementation-name implementation))
        "the selected frontend identifies itself as curses")))

(defun cli-route (args)
  "Classify ARGS with Lem's parser, independently of the Rust launcher."
  (multiple-value-bind (parsed parse-error) (ignore-errors (lem:parse-args args))
    (cond (parse-error :malformed)
          ((or (lem:command-line-arguments-help parsed)
               (lem:command-line-arguments-version parsed))
           :information)
          (t :editor))))

;; These are the same routing vectors as the launcher's tests/cli.rs.
;; Keep the two suites aligned when adding an option: Rust decides whether
;; to start a session, while this suite checks what Lem will actually do.
(deftest informational-arguments
  (dolist (args '(("-h")
                  ("--help")
                  ("-v")
                  ("--version")
                  ("README.md" "--help")
                  ("--" "--version")
                  ("--version" "--help")
                  ("--help" "--eval")))
    (ok (eq :information (cli-route args)) (format nil "~S" args))))

(deftest editor-arguments
  (dolist (args '(("-e" "--help")
                  ("--eval" "--version")
                  ("-i" "-h")
                  ("--interface" "-v")
                  ("--log-filename" "--help")
                  ("-e")
                  ("--eval")
                  ("--")
                  ("README.md")))
    (ok (eq :editor (cli-route args)) (format nil "~S" args)))
  (ok (equal '("--")
             (lem:command-line-arguments-filenames (lem:parse-args '("--"))))
      "a double dash is a filename, not an end-of-options marker"))

(deftest malformed-arguments
  (dolist (args '(("-i")
                  ("--interface")
                  ("--log-filename")
                  ("--help" "--interface")
                  ("--version" "--log-filename")
                  ("README.md" "-i")))
    (ok (eq :malformed (cli-route args)) (format nil "~S" args))))
