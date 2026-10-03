(defpackage :lem-ratatui/clipboard
  (:use :cl)
  (:import-from :lem-ratatui/implementation
   :ratatui))
(in-package :lem-ratatui/clipboard)

;;; Whose clipboard Lem uses (ADR 0019): the desktop's, through the OS
;;; agent, when the agent has one; otherwise the display's, through the
;;; relay. The display's is where the terminal is, which over SSH is the
;;; only clipboard on the user's own machine.

(defmethod lem-if:clipboard-copy ((ratatui ratatui) text)
  (unless (lem-os-agent:set-clipboard-text text)
    (call-next-method)))

(defmethod lem-if:clipboard-paste ((ratatui ratatui))
  (if (lem-os-agent:capable-p :clipboard)
      (lem-os-agent:clipboard-text)
      (call-next-method)))
