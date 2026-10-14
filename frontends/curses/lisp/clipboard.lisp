(defpackage :lem-curses/clipboard
  (:use :cl)
  (:import-from :lem-curses/implementation
   :curses))
(in-package :lem-curses/clipboard)

;;; Whose clipboard Lem uses (ADR 0019, 0020): the desktop's, through the
;;; OS agent, when the agent has one. Otherwise copying goes through the
;;; relay to the terminal's clipboard (OSC 52), where the terminal takes it
;;; — over SSH, the clipboard on the user's own machine — and pasting
;;; yanks Lem's own kill ring.

(defmethod lem-if:clipboard-copy ((curses curses) text)
  (unless (lem-os-agent:set-clipboard-text text)
    (call-next-method)))

(defmethod lem-if:clipboard-paste ((curses curses))
  (if (lem-os-agent:capable-p :clipboard)
      (lem-os-agent:clipboard-text)
      (call-next-method)))
